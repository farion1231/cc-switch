//! Native Command Code bridge.
//!
//! Claude requests are converted to Command Code's private \`/alpha/generate\`
//! envelope. The upstream NDJSON stream is normalized to OpenAI Chat SSE so
//! CC Switch can reuse its mature OpenAI -> Anthropic streaming converter and
//! existing usage accounting.

use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::proxy::sse::append_utf8_safe;
use crate::proxy::ProxyError;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use uuid::Uuid;

pub const COMMAND_CODE_VERSION: &str = "1.64.0";
const DEFAULT_MAX_TOKENS: u64 = 64_000;
const MAX_MAX_TOKENS: u64 = 200_000;

fn text_from_content(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(other) => other.to_string(),
    }
}

fn commandcode_content_parts(value: Option<&Value>) -> Vec<Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) if s.is_empty() => Vec::new(),
        Some(Value::String(s)) => vec![json!({"type": "text", "text": s})],
        Some(Value::Array(parts)) => {
            let mut out = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "text" | "input_text" | "output_text" => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            out.push(json!({"type": "text", "text": text}));
                        }
                    }
                    "image_url" | "input_image" | "image" => {
                        let url = part
                            .pointer("/image_url/url")
                            .and_then(Value::as_str)
                            .or_else(|| part.get("image_url").and_then(Value::as_str))
                            .or_else(|| part.get("image").and_then(Value::as_str));
                        if let Some(url) = url {
                            let mut image = json!({"type": "image", "image": url});
                            if let Some(rest) = url.strip_prefix("data:") {
                                if let Some((mime, _)) = rest.split_once(';') {
                                    image["mimeType"] = json!(mime);
                                }
                            }
                            out.push(image);
                        }
                    }
                    _ => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            out.push(json!({"type": "text", "text": text}));
                        }
                    }
                }
            }
            out
        }
        Some(other) => vec![json!({"type": "text", "text": other.to_string()})],
    }
}

fn resolve_commandcode_effort(body: &Value) -> Option<&'static str> {
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
    {
        return match effort {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        };
    }

    match body.pointer("/thinking/type").and_then(Value::as_str) {
        Some("adaptive") => Some("xhigh"),
        Some("enabled") => {
            let budget = body
                .pointer("/thinking/budget_tokens")
                .and_then(Value::as_u64);
            match budget {
                Some(v) if v < 4_000 => Some("low"),
                Some(v) if v < 16_000 => Some("medium"),
                Some(_) => Some("high"),
                None => Some("high"),
            }
        }
        _ => None,
    }
}

pub fn anthropic_to_commandcode(
    body: Value,
    session_id: Option<&str>,
) -> Result<Value, ProxyError> {
    let effort = resolve_commandcode_effort(&body);
    let chat = super::transform::anthropic_to_openai_with_reasoning_content(body, true)?;

    let model = chat.get("model").and_then(Value::as_str).ok_or_else(|| {
        ProxyError::TransformError("Command Code request is missing model".into())
    })?;
    let model = strip_one_m_suffix_for_upstream(model).to_string();

    let mut system_parts = Vec::new();
    let mut messages = Vec::new();
    let mut tool_names: HashMap<String, String> = HashMap::new();

    for message in chat
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");

        if role == "system" || role == "developer" {
            let text = text_from_content(message.get("content"));
            if !text.is_empty() {
                system_parts.push(text);
            }
            continue;
        }

        if role == "tool" {
            let call_id = message
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let tool_name = message
                .get("name")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| tool_names.get(call_id).cloned())
                .unwrap_or_else(|| "unknown".to_string());
            let text = text_from_content(message.get("content"));
            let output_type = if text.starts_with("Error:") {
                "error-text"
            } else {
                "text"
            };
            messages.push(json!({
                "role": "tool",
                "content": [{
                    "type": "tool-result",
                    "toolCallId": call_id,
                    "toolName": tool_name,
                    "output": {"type": output_type, "value": text}
                }]
            }));
            continue;
        }

        let mut parts = commandcode_content_parts(message.get("content"));

        if role == "assistant" {
            if let Some(reasoning) = message
                .get("reasoning_content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                parts.insert(0, json!({"type": "reasoning", "text": reasoning}));
            }

            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                    let empty_function = json!({});
                    let function = call.get("function").unwrap_or(&empty_function);
                    let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                    let args = function
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input = serde_json::from_str::<Value>(args)
                        .unwrap_or_else(|_| Value::String(args.to_string()));
                    if !id.is_empty() && !name.is_empty() {
                        tool_names.insert(id.to_string(), name.to_string());
                    }
                    parts.push(json!({
                        "type": "tool-call",
                        "toolCallId": id,
                        "toolName": name,
                        "input": input
                    }));
                }
            }
        }

        messages.push(json!({"role": role, "content": parts}));
    }

    let tools: Vec<Value> = chat
        .get("tools")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|tool| {
                    let function = tool.get("function")?;
                    let name = function.get("name")?.as_str()?;
                    let mut out = json!({
                        "type": "function",
                        "name": name,
                        "input_schema": function
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type":"object","properties":{}}))
                    });
                    if let Some(description) = function
                        .get("description")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        out["description"] = json!(description);
                    }
                    Some(out)
                })
                .collect()
        })
        .unwrap_or_default();

    let max_tokens = chat
        .get("max_tokens")
        .or_else(|| chat.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_TOKENS)
        .clamp(1, MAX_MAX_TOKENS);

    let mut params = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "system": system_parts.join("\n"),
        "max_tokens": max_tokens,
        "stream": true
    });
    if let Some(v) = chat.get("temperature").and_then(Value::as_f64) {
        params["temperature"] = json!(v);
    } else {
        params["temperature"] = json!(0.3);
    }
    if let Some(v) = chat.get("top_p").and_then(Value::as_f64) {
        params["top_p"] = json!(v);
    }
    if let Some(effort) = effort {
        params["reasoning_effort"] = json!(effort);
    }

    let thread_id = session_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    Ok(json!({
        "config": {
            "workingDir": ".",
            "date": chrono::Utc::now().format("%Y-%m-%d").to_string(),
            "environment": "cli",
            "structure": [],
            "isGitRepo": false,
            "currentBranch": "",
            "mainBranch": "",
            "gitStatus": "",
            "recentCommits": []
        },
        "memory": "",
        "taste": "",
        "skills": null,
        "permissionMode": "standard",
        "params": params,
        "threadId": thread_id
    }))
}

fn normalized_finish_reason(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("stop") {
        "tool_calls" | "tool-calls" | "tool_use" => "tool_calls",
        "length" | "max_tokens" | "max-output-tokens" | "max_output_tokens" => "length",
        "content_filter" | "content-filter" => "content_filter",
        _ => "stop",
    }
}

fn usage_to_openai(raw: Option<&Value>) -> Option<Value> {
    let raw = raw?.as_object()?;
    let details = raw.get("inputTokenDetails").and_then(Value::as_object);
    let output_details = raw.get("outputTokenDetails").and_then(Value::as_object);

    let cache_read = details
        .and_then(|d| d.get("cacheReadTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write = details
        .and_then(|d| d.get("cacheWriteTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let no_cache = details
        .and_then(|d| d.get("noCacheTokens"))
        .and_then(Value::as_u64);

    let mut prompt_tokens = raw
        .get("inputTokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| no_cache.unwrap_or(0) + cache_read + cache_write);

    if prompt_tokens < cache_read + cache_write {
        prompt_tokens = no_cache.unwrap_or(0) + cache_read + cache_write;
    }

    let completion_tokens = raw
        .get("outputTokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            output_details
                .and_then(|d| d.get("textTokens"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);

    let mut usage = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": prompt_tokens + completion_tokens
    });

    if cache_read > 0 || cache_write > 0 {
        usage["prompt_tokens_details"] = json!({
            "cached_tokens": cache_read,
            "cache_write_tokens": cache_write
        });
    }
    if cache_read > 0 {
        usage["cache_read_input_tokens"] = json!(cache_read);
    }
    if cache_write > 0 {
        usage["cache_creation_input_tokens"] = json!(cache_write);
    }

    Some(usage)
}

fn event_usage(event: &Value) -> Option<Value> {
    usage_to_openai(event.get("totalUsage").or_else(|| event.get("usage")))
}

fn stream_error(event: &Value) -> io::Error {
    let message = event
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| event.get("message").and_then(Value::as_str))
        .unwrap_or("Command Code stream error");
    io::Error::other(message.to_string())
}

fn sse_chunk(
    id: &str,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Bytes {
    let mut chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason
        }]
    });
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    Bytes::from(format!("data: {}\n\n", chunk))
}

#[derive(Default)]
struct CommandCodeStreamState {
    sent_role: bool,
    next_tool_index: usize,
    tool_indexes: HashMap<String, usize>,
    latest_usage: Option<Value>,
    finished: bool,
}

fn event_to_openai_sse(
    event: &Value,
    state: &mut CommandCodeStreamState,
    id: &str,
    model: &str,
) -> Result<Vec<Bytes>, io::Error> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    let mut out = Vec::new();

    match kind {
        "text-delta" => {
            let text = event.get("text").and_then(Value::as_str).unwrap_or("");
            if !text.is_empty() {
                let mut delta = json!({"content": text});
                if !state.sent_role {
                    delta["role"] = json!("assistant");
                    state.sent_role = true;
                }
                out.push(sse_chunk(id, model, delta, None, None));
            }
        }
        "reasoning-delta" => {
            let text = event.get("text").and_then(Value::as_str).unwrap_or("");
            if !text.is_empty() {
                let mut delta = json!({"reasoning_content": text});
                if !state.sent_role {
                    delta["role"] = json!("assistant");
                    state.sent_role = true;
                }
                out.push(sse_chunk(id, model, delta, None, None));
            }
        }
        "tool-input-start" | "tool-use" => {
            let call_id = event
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| event.get("toolCallId").and_then(Value::as_str))
                .unwrap_or("");
            let name = event.get("toolName").and_then(Value::as_str).unwrap_or("");
            let index = *state
                .tool_indexes
                .entry(call_id.to_string())
                .or_insert_with(|| {
                    let current = state.next_tool_index;
                    state.next_tool_index += 1;
                    current
                });
            let mut delta = json!({
                "tool_calls": [{
                    "index": index,
                    "id": call_id,
                    "type": "function",
                    "function": {"name": name}
                }]
            });
            if !state.sent_role {
                delta["role"] = json!("assistant");
                state.sent_role = true;
            }
            out.push(sse_chunk(id, model, delta, None, None));
        }
        "tool-input-delta" | "tool-delta" => {
            let call_id = event
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| event.get("toolCallId").and_then(Value::as_str))
                .unwrap_or("");
            let index = state
                .tool_indexes
                .get(call_id)
                .copied()
                .unwrap_or_else(|| state.next_tool_index.saturating_sub(1));
            let args = event
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| event.get("text").and_then(Value::as_str))
                .unwrap_or("");
            if !args.is_empty() {
                out.push(sse_chunk(
                    id,
                    model,
                    json!({"tool_calls":[{"index": index, "function":{"arguments": args}}]}),
                    None,
                    None,
                ));
            }
        }
        "tool-call" => {
            let call_id = event
                .get("toolCallId")
                .and_then(Value::as_str)
                .or_else(|| event.get("id").and_then(Value::as_str))
                .unwrap_or("");
            if !state.tool_indexes.contains_key(call_id) {
                let index = state.next_tool_index;
                state.next_tool_index += 1;
                state.tool_indexes.insert(call_id.to_string(), index);
                let name = event.get("toolName").and_then(Value::as_str).unwrap_or("");
                let input = event
                    .get("input")
                    .or_else(|| event.get("args"))
                    .or_else(|| event.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let arguments = match input {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                let mut delta = json!({
                    "tool_calls":[{
                        "index": index,
                        "id": call_id,
                        "type":"function",
                        "function":{"name": name, "arguments": arguments}
                    }]
                });
                if !state.sent_role {
                    delta["role"] = json!("assistant");
                    state.sent_role = true;
                }
                out.push(sse_chunk(id, model, delta, None, None));
            }
        }
        "finish-step" => {
            if let Some(usage) = event_usage(event) {
                state.latest_usage = Some(usage);
            }
        }
        "finish" => {
            let usage = event_usage(event).or_else(|| state.latest_usage.clone());
            let reason = normalized_finish_reason(
                event
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("rawFinishReason").and_then(Value::as_str)),
            );
            out.push(sse_chunk(id, model, json!({}), Some(reason), usage));
            out.push(Bytes::from_static(b"data: [DONE]\n\n"));
            state.finished = true;
        }
        "error" => return Err(stream_error(event)),
        _ => {}
    }

    Ok(out)
}

pub fn create_openai_sse_stream_from_commandcode<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    model: String,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let id = format!("chatcmpl-{}", Uuid::new_v4());
        let mut buffer = String::new();
        let mut remainder = Vec::new();
        let mut state = CommandCodeStreamState::default();
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    yield Err(io::Error::other(error.to_string()));
                    return;
                }
            };
            append_utf8_safe(&mut buffer, &mut remainder, &bytes);

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().trim_end_matches('\r').to_string();
                buffer.drain(..=pos);
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                let event: Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                match event_to_openai_sse(&event, &mut state, &id, &model) {
                    Ok(items) => {
                        for item in items {
                            yield Ok(item);
                        }
                    }
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }
                if state.finished {
                    return;
                }
            }
        }

        if !buffer.trim().is_empty() && !state.finished {
            if let Ok(event) = serde_json::from_str::<Value>(buffer.trim()) {
                match event_to_openai_sse(&event, &mut state, &id, &model) {
                    Ok(items) => {
                        for item in items {
                            yield Ok(item);
                        }
                    }
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }
            }
        }

        if !state.finished {
            yield Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Command Code stream ended before finish event",
            ));
        }
    }
}

#[derive(Default)]
struct ToolAggregate {
    index: usize,
    id: String,
    name: String,
    arguments: String,
}

pub fn ndjson_to_openai_response(body: &str, model: &str) -> Result<Value, ProxyError> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tools: HashMap<String, ToolAggregate> = HashMap::new();
    let mut next_tool_index = 0usize;
    let mut latest_usage: Option<Value> = None;
    let mut finish_reason = "stop";
    let mut finished = false;

    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let event: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "text-delta" => {
                if let Some(value) = event.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                }
            }
            "reasoning-delta" => {
                if let Some(value) = event.get("text").and_then(Value::as_str) {
                    reasoning.push_str(value);
                }
            }
            "tool-input-start" | "tool-use" => {
                let id = event
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("toolCallId").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string();
                let name = event
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                tools.entry(id.clone()).or_insert_with(|| {
                    let index = next_tool_index;
                    next_tool_index += 1;
                    ToolAggregate {
                        index,
                        id,
                        name,
                        arguments: String::new(),
                    }
                });
            }
            "tool-input-delta" | "tool-delta" => {
                let id = event
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("toolCallId").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string();
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("text").and_then(Value::as_str))
                    .unwrap_or("");
                if let Some(tool) = tools.get_mut(&id) {
                    tool.arguments.push_str(delta);
                }
            }
            "tool-call" => {
                let id = event
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("id").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string();
                let name = event
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = event
                    .get("input")
                    .or_else(|| event.get("args"))
                    .or_else(|| event.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let arguments = match input {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                let index = tools.get(&id).map(|t| t.index).unwrap_or_else(|| {
                    let index = next_tool_index;
                    next_tool_index += 1;
                    index
                });
                tools.insert(
                    id.clone(),
                    ToolAggregate {
                        index,
                        id,
                        name,
                        arguments,
                    },
                );
            }
            "finish-step" => {
                if let Some(usage) = event_usage(&event) {
                    latest_usage = Some(usage);
                }
            }
            "finish" => {
                if let Some(usage) = event_usage(&event) {
                    latest_usage = Some(usage);
                }
                finish_reason = normalized_finish_reason(
                    event
                        .get("finishReason")
                        .and_then(Value::as_str)
                        .or_else(|| event.get("rawFinishReason").and_then(Value::as_str)),
                );
                finished = true;
            }
            "error" => {
                return Err(ProxyError::TransformError(stream_error(&event).to_string()));
            }
            _ => {}
        }
    }

    if !finished {
        return Err(ProxyError::TransformError(
            "Command Code stream ended before finish event".to_string(),
        ));
    }

    let mut ordered_tools: Vec<_> = tools.into_values().collect();
    ordered_tools.sort_by_key(|tool| tool.index);

    let mut message = json!({"role": "assistant", "content": text});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !ordered_tools.is_empty() {
        message["tool_calls"] = Value::Array(
            ordered_tools
                .into_iter()
                .map(|tool| {
                    json!({
                        "id": tool.id,
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "arguments": tool.arguments
                        }
                    })
                })
                .collect(),
        );
        if message["content"].as_str().is_some_and(str::is_empty) {
            message["content"] = Value::Null;
        }
    }

    let mut response = json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }]
    });
    if let Some(usage) = latest_usage {
        response["usage"] = usage;
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_strips_one_m_and_clamps_max_tokens() {
        let body = json!({
            "model": "deepseek/deepseek-v4.1-flash[1M]",
            "max_tokens": 999999,
            "messages": [{"role":"user","content":"hi"}]
        });
        let cc = anthropic_to_commandcode(body, Some("session-1")).unwrap();
        assert_eq!(cc["params"]["model"], "deepseek/deepseek-v4.1-flash");
        assert_eq!(cc["params"]["max_tokens"], 200000);
    }

    #[test]
    fn usage_preserves_cache_buckets() {
        let raw = json!({
            "inputTokens": 120,
            "outputTokens": 8,
            "inputTokenDetails": {
                "noCacheTokens": 20,
                "cacheReadTokens": 90,
                "cacheWriteTokens": 10
            }
        });
        let usage = usage_to_openai(Some(&raw)).unwrap();
        assert_eq!(usage["prompt_tokens"], 120);
        assert_eq!(usage["completion_tokens"], 8);
        assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 90);
        assert_eq!(usage["prompt_tokens_details"]["cache_write_tokens"], 10);
    }

    #[test]
    fn nonstream_finish_has_usage() {
        let body = r#"{"type":"text-delta","text":"ok"}
{"type":"finish","finishReason":"stop","totalUsage":{"inputTokens":12,"outputTokens":3}}
"#;
        let response = ndjson_to_openai_response(body, "deepseek/test").unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "ok");
        assert_eq!(response["usage"]["total_tokens"], 15);
    }
}
