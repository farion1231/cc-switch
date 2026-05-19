//! Codex Responses ↔ OpenAI Chat Completions conversion.
//!
//! This module is used when the Codex client talks to CC Switch through the
//! Responses API, while the selected upstream provider only exposes an
//! OpenAI-compatible Chat Completions endpoint.

use crate::proxy::{error::ProxyError, json_canonical::canonical_json_string};
use serde_json::{json, Value};

const EXTRA_CHAT_PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];
const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request.
pub fn responses_to_chat_completions(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let instructions = instruction_text(instructions);
        if !instructions.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }
    }

    if let Some(input) = body.get("input") {
        append_responses_input_as_chat_messages(input, &mut messages)?;
    }
    result["messages"] = json!(messages);

    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(max_tokens) = body.get("max_output_tokens") {
        if super::transform::is_openai_o_series(model) {
            result["max_completion_tokens"] = max_tokens.clone();
        } else {
            result["max_tokens"] = max_tokens.clone();
        }
    }
    if let Some(max_tokens) = body.get("max_tokens") {
        result["max_tokens"] = max_tokens.clone();
    }
    if let Some(max_tokens) = body.get("max_completion_tokens") {
        result["max_completion_tokens"] = max_tokens.clone();
    }

    for key in ["temperature", "top_p", "stream"] {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }

    if super::transform::supports_reasoning_effort(model) {
        if let Some(effort) = body.pointer("/reasoning/effort") {
            result["reasoning_effort"] = effort.clone();
        }
    }

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let tools: Vec<Value> = tools
            .iter()
            .filter_map(responses_tool_to_chat_tool)
            .collect();
        if !tools.is_empty() {
            result["tools"] = json!(tools);
        }
    }

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_chat(tool_choice);
    }

    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }

    Ok(result)
}

fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| part.as_str())
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

fn append_responses_input_as_chat_messages(
    input: &Value,
    messages: &mut Vec<Value>,
) -> Result<(), ProxyError> {
    let mut pending_tool_calls = Vec::new();

    match input {
        Value::String(text) => {
            messages.push(json!({
                "role": "user",
                "content": text
            }));
        }
        Value::Array(items) => {
            for item in items {
                append_responses_item_as_chat_message(item, messages, &mut pending_tool_calls)?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(input, messages, &mut pending_tool_calls)?;
        }
        _ => {}
    }

    flush_pending_tool_calls(messages, &mut pending_tool_calls);
    Ok(())
}

fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
) -> Result<(), ProxyError> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(item));
        }
        Some("function_call_output") => {
            flush_pending_tool_calls(messages, pending_tool_calls);
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let output = match item.get("output") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => canonical_json_string(v),
                None => String::new(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("reasoning") => {
            // Reasoning items are Responses-specific context. Chat-only providers
            // cannot consume encrypted reasoning state, so omit it.
        }
        Some("message") | None => {
            flush_pending_tool_calls(messages, pending_tool_calls);
            if item.get("role").is_some() || item.get("content").is_some() {
                messages.push(responses_message_item_to_chat_message(item));
            }
        }
        _ => {
            flush_pending_tool_calls(messages, pending_tool_calls);
            if item.get("role").is_some() || item.get("content").is_some() {
                messages.push(responses_message_item_to_chat_message(item));
            }
        }
    }

    Ok(())
}

fn flush_pending_tool_calls(messages: &mut Vec<Value>, pending_tool_calls: &mut Vec<Value>) {
    if pending_tool_calls.is_empty() {
        return;
    }

    messages.push(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls)
    }));
}

fn responses_message_item_to_chat_message(item: &Value) -> Value {
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let content = item
        .get("content")
        .map(|value| responses_content_to_chat_content(role, value))
        .unwrap_or(Value::Null);

    json!({
        "role": role,
        "content": content
    })
}

fn responses_content_to_chat_content(_role: &str, content: &Value) -> Value {
    if content.is_null() || content.is_string() {
        return content.clone();
    }

    let Some(parts) = content.as_array() else {
        return content.clone();
    };

    let mut chat_parts: Vec<Value> = Vec::new();
    let mut has_non_text_part = false;

    for part in parts {
        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match part_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "input_image" => {
                if let Some(image_url) = part.get("image_url") {
                    let image_url = if image_url.is_object() {
                        image_url.clone()
                    } else {
                        json!({ "url": image_url.as_str().unwrap_or_default() })
                    };
                    chat_parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url
                    }));
                    has_non_text_part = true;
                }
            }
            _ => {}
        }
    }

    if !has_non_text_part {
        return Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Value::Array(chat_parts)
}

fn responses_function_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = match item.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    };

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments
        }
    })
}

fn responses_tool_to_chat_tool(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }

    if tool.get("function").is_some() {
        let mut chat_tool = tool.clone();
        if let Some(strict) = tool.get("strict").cloned() {
            if let Some(function) = chat_tool
                .get_mut("function")
                .and_then(|value| value.as_object_mut())
            {
                function.entry("strict".to_string()).or_insert(strict);
            }
            if let Some(obj) = chat_tool.as_object_mut() {
                obj.remove("strict");
            }
        }
        return Some(chat_tool);
    }

    let mut function = json!({
        "name": tool.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": tool.get("parameters").cloned().unwrap_or_else(|| json!({}))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }

    Some(json!({
        "type": "function",
        "function": function
    }))
}

fn responses_tool_choice_to_chat(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("function") => {
            json!({
                "type": "function",
                "function": {
                    "name": obj.get("name").and_then(|v| v.as_str()).unwrap_or("")
                }
            })
        }
        _ => tool_choice.clone(),
    }
}

/// Convert a non-streaming Chat Completions response into a Responses response.
pub fn chat_completion_to_response(body: Value) -> Result<Value, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProxyError::TransformError("No choices in chat response".to_string()))?;
    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices in chat response".to_string()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in chat choice".to_string()))?;

    let response_id = response_id_from_chat_id(body.get("id").and_then(|v| v.as_str()));
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created_at = body.get("created").and_then(|v| v.as_u64()).unwrap_or(0);
    let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

    let mut output = Vec::new();
    if let Some(reasoning_item) = chat_reasoning_to_response_output_item(message, &response_id) {
        output.push(reasoning_item);
    }
    if let Some(message_item) = chat_message_to_response_output_item(message, &response_id) {
        output.push(message_item);
    }
    output.extend(chat_tool_calls_to_response_output_items(message));

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": response_status_from_finish_reason(finish_reason),
        "model": model,
        "output": output,
        "usage": chat_usage_to_responses_usage(body.get("usage"))
    });

    if finish_reason == Some("length") {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }

    Ok(response)
}

fn chat_reasoning_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let reasoning = chat_reasoning_text(message)?;
    if reasoning.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("rs_{response_id}"),
        "type": "reasoning",
        "summary": [{
            "type": "summary_text",
            "text": reasoning
        }]
    }))
}

fn chat_reasoning_text(message: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(text) = message.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(reasoning) = message.get("reasoning") {
        for key in ["content", "text", "summary"] {
            if let Some(text) = reasoning.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
        if let Some((reasoning, _answer)) = split_leading_think_block(content) {
            if !reasoning.is_empty() {
                return Some(reasoning);
            }
        }
    }

    None
}

fn chat_message_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let mut content = Vec::new();

    if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
        let text = split_leading_think_block(text)
            .map(|(_reasoning, answer)| answer)
            .unwrap_or_else(|| text.to_string());
        if !text.is_empty() {
            content.push(json!({
                "type": "output_text",
                "text": text,
                "annotations": []
            }));
        }
    } else if let Some(parts) = message.get("content").and_then(|v| v.as_array()) {
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match part_type {
                "text" | "output_text" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "output_text",
                                "text": text,
                                "annotations": []
                            }));
                        }
                    }
                }
                "refusal" => {
                    if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "refusal",
                                "refusal": text
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(refusal) = message.get("refusal").and_then(|v| v.as_str()) {
        if !refusal.is_empty() {
            content.push(json!({
                "type": "refusal",
                "refusal": refusal
            }));
        }
    }

    if content.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("{response_id}_msg"),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    }))
}

pub(crate) fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    if !after_ws.starts_with(THINK_OPEN_TAG) {
        return None;
    }

    let body_start = leading_ws_len + THINK_OPEN_TAG.len();
    let close_relative = text[body_start..].find(THINK_CLOSE_TAG)?;
    let close_start = body_start + close_relative;
    let answer_start = close_start + THINK_CLOSE_TAG.len();

    Some((
        text[body_start..close_start].trim().to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

pub(crate) fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws
        .strip_prefix(THINK_OPEN_TAG)
        .map(|value| value.trim().to_string())
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}

fn chat_tool_calls_to_response_output_items(message: &Value) -> Vec<Value> {
    let mut output = Vec::new();

    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            output.push(chat_tool_call_to_response_item(tool_call, index));
        }
    } else if let Some(function_call) = message.get("function_call") {
        output.push(chat_legacy_function_call_to_response_item(function_call));
    }

    output
}

fn chat_tool_call_to_response_item(tool_call: &Value, index: usize) -> Value {
    let call_id = tool_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{index}"));
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = match function.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    };

    json!({
        "id": format!("fc_{call_id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": name,
        "arguments": arguments
    })
}

fn chat_legacy_function_call_to_response_item(function_call: &Value) -> Value {
    let call_id = function_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("call_0");
    let name = function_call
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = match function_call.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    };

    json!({
        "id": format!("fc_{call_id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": name,
        "arguments": arguments
    })
}

pub(crate) fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object() && !value.is_null()) else {
        return json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        });
    };

    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens
    });

    if let Some(cached) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(|v| v.as_u64())
    {
        result["input_tokens_details"] = json!({ "cached_tokens": cached });
    }

    if let Some(details) = usage.get("completion_tokens_details") {
        result["output_tokens_details"] = details.clone();
    }

    if let Some(cache_read) = usage.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = cache_read.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = cache_creation.clone();
    }

    result
}

pub(crate) fn response_id_from_chat_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("ccswitch");
    if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

pub(crate) fn response_status_from_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_request_to_chat_maps_messages_tools_and_limits() {
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are concise.",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "Weather?"},
                        {"type": "input_image", "image_url": "data:image/png;base64,abc"},
                        {"type": "input_text", "text": "Use Celsius."}
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"city\":\"Tokyo\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Sunny"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object"},
                "strict": true
            }],
            "tool_choice": {"type": "function", "name": "get_weather"},
            "max_output_tokens": 100,
            "reasoning": {"effort": "high"},
            "stream": true
        });

        let result = responses_to_chat_completions(input).unwrap();

        assert_eq!(result["model"], "gpt-5.4");
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(result["messages"][1]["role"], "user");
        assert_eq!(result["messages"][1]["content"][0]["type"], "text");
        assert_eq!(result["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(result["messages"][1]["content"][2]["type"], "text");
        assert_eq!(result["messages"][1]["content"][2]["text"], "Use Celsius.");
        assert_eq!(result["messages"][2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(result["messages"][3]["role"], "tool");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(result["tools"][0]["function"]["strict"], true);
        assert_eq!(result["tool_choice"]["function"]["name"], "get_weather");
        assert_eq!(result["max_tokens"], 100);
        assert_eq!(result["reasoning_effort"], "high");
    }

    #[test]
    fn responses_request_to_chat_keeps_multiple_tool_calls_adjacent_to_outputs() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "list_files",
                    "arguments": "{\"path\":\"src\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": ["main.rs", "lib.rs"]
                },
                {
                    "role": "user",
                    "content": "Continue"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["tool_calls"][1]["id"], "call_2");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_2");
        assert_eq!(messages[2]["content"], "[\"main.rs\",\"lib.rs\"]");
        assert_eq!(messages[3]["role"], "user");
    }

    #[test]
    fn chat_response_to_responses_maps_text_tool_calls_and_usage() {
        let input = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "I should check the weather before answering.",
                    "content": "Let me check.",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_tokens_details": {"cached_tokens": 3}
            }
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["id"], "resp_chatcmpl_1");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(
            result["output"][0]["summary"][0]["text"],
            "I should check the weather before answering."
        );
        assert_eq!(result["output"][1]["type"], "message");
        assert_eq!(result["output"][1]["content"][0]["text"], "Let me check.");
        assert_eq!(result["output"][2]["type"], "function_call");
        assert_eq!(result["output"][2]["call_id"], "call_1");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
        assert_eq!(result["usage"]["input_tokens_details"]["cached_tokens"], 3);
    }

    #[test]
    fn chat_response_to_responses_splits_inline_think_content() {
        let input = json!({
            "id": "chatcmpl_think",
            "object": "chat.completion",
            "created": 123,
            "model": "MiniMax-M2.7",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<think>\nI should answer with pong.\n</think>\n\npong"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30,
                "completion_tokens_details": {"reasoning_tokens": 18}
            }
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(
            result["output"][0]["summary"][0]["text"],
            "I should answer with pong."
        );
        assert_eq!(result["output"][1]["type"], "message");
        assert_eq!(result["output"][1]["content"][0]["text"], "pong");
        assert_eq!(
            result["usage"]["output_tokens_details"]["reasoning_tokens"],
            18
        );
    }

    #[test]
    fn chat_response_length_maps_to_incomplete_response() {
        let input = json!({
            "id": "chatcmpl_2",
            "model": "gpt-5.4",
            "choices": [{
                "message": {"role": "assistant", "content": "partial"},
                "finish_reason": "length"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
    }
}
