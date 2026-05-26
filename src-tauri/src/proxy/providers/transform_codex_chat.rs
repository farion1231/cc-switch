//! Codex Responses ↔ OpenAI Chat Completions conversion.
//!
//! This module is used when the Codex client talks to CC Switch through the
//! Responses API, while the selected upstream provider only exposes an
//! OpenAI-compatible Chat Completions endpoint.

use crate::proxy::{error::ProxyError, json_canonical::canonical_json_string};
use crate::provider::CodexChatReasoningConfig;
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

/// Appended to the first system message for Codex -> Chat Completions requests
/// to reinforce Codex-like agent behavior: continue multi-step tasks to
/// completion, call tools immediately instead of describing-and-waiting,
/// respect explicit sequencing (no parallel tool calls when ordered), and
/// output copyable Markdown task blocks when asked for a handoff or checklist.
const AGENT_LOOP_HINT: &str =
    "\n\nYou are running inside Codex. Follow Codex agent behavior, not generic chat behavior. Distinguish planning, execution, handoff, and review requests. For planning requests, clarify assumptions and keep scope tight. For execution, diagnosis, testing, fixing, or verification requests, use tools proactively and continue until you reach a concrete result; do not ask for confirmation unless required information is missing, the action is destructive, or a product decision is ambiguous. For handoff, task, checklist, or tool-instruction requests, provide a complete copyable Markdown block with goal, scope, non-goals, acceptance criteria, and verification when relevant. For review requests, lead with findings, risks, regressions, and missing tests. For multi-step work, continue the agent loop until complete; call tools in the same turn when needed, and respect explicit sequential-execution instructions.";

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request.
pub fn responses_to_chat_completions(
    body: Value,
    compatibility_mode: Option<&str>,
) -> Result<Value, ProxyError> {
    let is_deepseek = compatibility_mode == Some("deepseek_thinking");
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
        append_responses_input_as_chat_messages(input, &mut messages, is_deepseek)?;
    }

    // For Codex requests (detected by presence of `instructions`), append an
    // agent loop hint to the first system message to encourage immediate tool
    // calls instead of describing steps and waiting for confirmation.
    if body.get("instructions").is_some() {
        if let Some(first) = messages.first_mut() {
            if first.get("role").and_then(|v| v.as_str()) == Some("system") {
                if let Some(content) = first.get("content").and_then(|v| v.as_str()) {
                    if !content.contains(AGENT_LOOP_HINT) {
                        let new_content = format!("{}{}", content, AGENT_LOOP_HINT);
                        first["content"] = json!(new_content);
                    }
                }
            }
        }
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

    // In deepseek_thinking mode, temperature/top_p/penalty are invalid; omit them.
    let sampling_keys = if is_deepseek {
        &["stream"][..]
    } else {
        &["temperature", "top_p", "stream"][..]
    };
    for key in sampling_keys {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }

    // In deepseek_thinking mode, filter penalty fields from passthrough.
    let passthrough_keys: Vec<&str> = EXTRA_CHAT_PASSTHROUGH_FIELDS
        .iter()
        .filter(|&&key| {
            if is_deepseek {
                !matches!(key, "frequency_penalty" | "presence_penalty")
            } else {
                true
            }
        })
        .map(|&s| s)
        .collect();

    // Responses API does not have stream_options; inject include_usage for
    // Chat Completions so upstream returns token counts in SSE chunks
    if result.get("stream").and_then(|v| v.as_bool()) == Some(true) {
        result["stream_options"] = json!({"include_usage": true});
    }

    if super::transform::supports_reasoning_effort(model) || is_deepseek {
        if let Some(effort) = body.pointer("/reasoning/effort") {
            // In deepseek_thinking mode, map reasoning_effort:
            //   low/medium -> high, high -> high, xhigh/max -> max
            let mapped = if is_deepseek {
                match effort.as_str().unwrap_or("") {
                    "low" | "medium" | "high" => "high",
                    "xhigh" | "max" => "max",
                    _ => effort.as_str().unwrap_or("high"),
                }
            } else {
                effort.as_str().unwrap_or("")
            };
            result["reasoning_effort"] = json!(mapped);
        }
    }

    if is_deepseek {
        result["thinking"] = json!({ "type": "enabled" });
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
        if let Some(chat_tool_choice) = responses_tool_choice_to_chat(tool_choice) {
            result["tool_choice"] = chat_tool_choice;
        }
    }

    for key in &passthrough_keys {
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }

    Ok(result)
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request,
/// using provider-declared Codex Chat reasoning capabilities when available.
pub fn responses_to_chat_completions_with_reasoning(
    body: Value,
    reasoning_config: Option<&CodexChatReasoningConfig>,
) -> Result<Value, ProxyError> {
    // Map reasoning_config's deepseek mode to compatibility_mode
    let compatibility_mode = reasoning_config
        .and_then(|c| c.effort_value_mode.as_ref())
        .filter(|m| *m == "deepseek")
        .map(|_| "deepseek_thinking");
    responses_to_chat_completions(body, compatibility_mode)
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
    is_deepseek: bool,
) -> Result<(), ProxyError> {
    let mut pending_tool_calls = Vec::new();
    let mut pending_reasoning: Option<String> = None;

    match input {
        Value::String(text) => {
            // No tool_calls coming — attach reasoning to last assistant.
            attach_reasoning_to_last_assistant(messages, pending_reasoning.take(), is_deepseek);
            messages.push(json!({
                "role": "user",
                "content": text
            }));
        }
        Value::Array(items) => {
            for item in items {
                append_responses_item_as_chat_message(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_reasoning,
                    is_deepseek,
                )?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
                is_deepseek,
            )?;
        }
        _ => {}
    }

    // Flush reasoning together with tool_calls into one assistant message
    // (required by DeepSeek thinking mode). If only reasoning remains, attach
    // to last assistant.
    flush_reasoning_with_tool_calls(
        messages,
        &mut pending_tool_calls,
        &mut pending_reasoning,
        is_deepseek,
    );
    Ok(())
}

/// Attach reasoning to the last assistant message. If no assistant message
/// exists, create a standalone one. Only used when there are no tool_calls.
fn attach_reasoning_to_last_assistant(
    messages: &mut Vec<Value>,
    reasoning: Option<String>,
    is_deepseek: bool,
) {
    if !is_deepseek || reasoning.is_none() {
        return;
    }
    let reasoning = reasoning.unwrap();
    // Find the last assistant message and attach reasoning.
    for msg in messages.iter_mut().rev() {
        if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
            msg["reasoning_content"] = json!(reasoning);
            return;
        }
    }
    // No assistant message found — create a standalone one.
    messages.push(json!({
        "role": "assistant",
        "content": null,
        "reasoning_content": reasoning
    }));
}

/// Flush reasoning together with tool_calls into the SAME assistant message.
/// This is the critical DeepSeek thinking mode fix: splitting reasoning and
/// tool_calls causes "reasoning_content must be passed back" errors.
fn flush_reasoning_with_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    is_deepseek: bool,
) {
    let reasoning = pending_reasoning.take();
    if pending_tool_calls.is_empty() && reasoning.is_none() {
        return;
    }

    let has_reasoning = reasoning.is_some();
    let has_tool_calls = !pending_tool_calls.is_empty();

    if has_reasoning && has_tool_calls {
        // Try to merge into the previous assistant message if it has content
        // but no tool_calls — avoids creating a separate assistant message.
        if let Some(last) = messages.last_mut() {
            if last.get("role").and_then(|v| v.as_str()) == Some("assistant")
                && last.get("tool_calls").is_none()
                && !last["content"].is_null()
            {
                last["tool_calls"] = json!(std::mem::take(pending_tool_calls));
                last["reasoning_content"] = json!(reasoning.unwrap());
                return;
            }
        }
        // No mergeable assistant — create a new one with both fields.
        messages.push(json!({
            "role": "assistant",
            "content": null,
            "reasoning_content": reasoning.unwrap(),
            "tool_calls": std::mem::take(pending_tool_calls)
        }));
    } else if has_tool_calls {
        flush_pending_tool_calls_only(messages, pending_tool_calls);
    } else if has_reasoning {
        attach_reasoning_to_last_assistant(messages, reasoning, is_deepseek);
    }
}

/// Flush tool_calls only (no reasoning). Merge into previous assistant if possible.
fn flush_pending_tool_calls_only(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }
    // Merge into previous assistant if it has content but no tool_calls.
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("assistant")
            && last.get("tool_calls").is_none()
            && !last["content"].is_null()
        {
            last["tool_calls"] = json!(std::mem::take(pending_tool_calls));
            return;
        }
    }
    messages.push(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls)
    }));
}

fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    is_deepseek: bool,
) -> Result<(), ProxyError> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            // Do NOT flush pending_reasoning here — it will be merged into
            // the same assistant message as tool_calls when flushed later.
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(item));
        }
        Some("function_call_output") => {
            // Flush reasoning together with tool_calls so they end up in
            // the SAME assistant message (required by DeepSeek thinking mode).
            flush_reasoning_with_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                is_deepseek,
            );
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
            if is_deepseek {
                // Capture reasoning summary text for merging into the next assistant message.
                if let Some(summary_text) = item.pointer("/summary/0/text").and_then(|v| v.as_str())
                {
                    *pending_reasoning = Some(summary_text.to_string());
                }
            }
            // In standard mode, omit reasoning (previous behavior).
        }
        Some("message") | None => {
            // Flush tool_calls before processing the message — reasoning
            // stays pending so it can merge with tool_calls later.
            flush_pending_tool_calls_only(messages, pending_tool_calls);
            if item.get("role").is_some() || item.get("content").is_some() {
                let msg = responses_message_item_to_chat_message(item);
                if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                    let content = msg["content"].clone();
                    try_merge_content_into_last_tool_call_assistant(messages, content);
                } else {
                    messages.push(msg);
                }
            }
        }
        _ => {
            flush_pending_tool_calls_only(messages, pending_tool_calls);
            if item.get("role").is_some() || item.get("content").is_some() {
                let msg = responses_message_item_to_chat_message(item);
                if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                    let content = msg["content"].clone();
                    try_merge_content_into_last_tool_call_assistant(messages, content);
                } else {
                    messages.push(msg);
                }
            }
        }
    }

    Ok(())
}

/// If the last message is an assistant with tool_calls but no content,
/// merge the given content into it instead of creating a separate message.
fn try_merge_content_into_last_tool_call_assistant(messages: &mut Vec<Value>, content: Value) {
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("assistant")
            && last.get("tool_calls").is_some()
            && last["content"].is_null()
        {
            last["content"] = content;
            return;
        }
    }
    messages.push(json!({
        "role": "assistant",
        "content": content
    }));
}

fn responses_message_item_to_chat_message(item: &Value) -> Value {
    let mut role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    // Responses "developer" is not a valid Chat Completions role; normalize to "system"
    if role == "developer" {
        role = "system";
    }
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

fn responses_tool_choice_to_chat(tool_choice: &Value) -> Option<Value> {
    match tool_choice {
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("function") => {
            Some(json!({
                "type": "function",
                "function": {
                    "name": obj.get("name").and_then(|v| v.as_str()).unwrap_or("")
                }
            }))
        }
        // "none", "auto", "required" are valid in both APIs — pass through as-is
        Value::String(_) => Some(tool_choice.clone()),
        // Responses-only tool types (file_search, web_search_preview, etc.) have no Chat
        // Completions equivalent; omit entirely.
        _ => None,
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
        Some("length") | Some("content_filter") | Some("refusal") => "incomplete",
        _ => "completed",
    }
}

/// Normalize a Chat Completions error response to a Responses-style error shape,
/// so the proxy downstream error handler can render it uniformly.
pub fn chat_error_to_response_error(body: Option<&Value>) -> Value {
    let Some(value) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    };

    if let Some(text) = value.as_str() {
        return json!({
            "error": {
                "message": text,
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    }

    let source = value.get("error").unwrap_or(value);

    let message = source
        .get("message")
        .or_else(|| source.get("detail"))
        .or_else(|| source.get("status_msg"))
        .or_else(|| source.pointer("/base_resp/status_msg"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| source.as_str().map(ToString::to_string))
        .unwrap_or_else(|| {
            // 没法从字段提取出文本，就把整个 JSON 序列化回去，方便用户排查。
            serde_json::to_string(source).unwrap_or_else(|_| "Upstream error".to_string())
        });

    let error_type = source
        .get("type")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "upstream_error".to_string());

    let code = source
        .get("code")
        .cloned()
        .or_else(|| source.pointer("/base_resp/status_code").cloned())
        .unwrap_or(serde_json::Value::Null);

    let param = source
        .get("param")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
            "param": param,
        }
    })
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

        let result = responses_to_chat_completions(input, None).unwrap();

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

        let result = responses_to_chat_completions(input, None).unwrap();
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

    #[test]
    fn chat_response_content_filter_maps_to_incomplete() {
        let input = json!({
            "id": "chatcmpl_filter",
            "model": "gpt-5.4",
            "choices": [{
                "message": {"role": "assistant", "content": "filtered"},
                "finish_reason": "content_filter"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();
        assert_eq!(result["status"], "incomplete");
    }

    #[test]
    fn chat_response_refusal_maps_to_incomplete() {
        let input = json!({
            "id": "chatcmpl_refusal",
            "model": "gpt-5.4",
            "choices": [{
                "message": {"role": "assistant", "content": "I cannot help with that"},
                "finish_reason": "refusal"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();
        assert_eq!(result["status"], "incomplete");
    }

    #[test]
    fn chat_response_tool_calls_maps_to_completed() {
        let input = json!({
            "id": "chatcmpl_tools",
            "model": "gpt-5.4",
            "choices": [{
                "message": {"role": "assistant", "content": null},
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn responses_developer_role_is_normalized_to_system() {
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are a coding assistant.",
            "input": [
                {"role": "developer", "content": "Use Chinese for all responses."},
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there"},
                {"role": "developer", "content": "Always include code examples."}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // system from instructions
        assert_eq!(messages[0]["role"], "system");
        // developer -> system
        assert_eq!(messages[1]["role"], "system");
        assert_eq!(messages[1]["content"], "Use Chinese for all responses.");
        // user stays user
        assert_eq!(messages[2]["role"], "user");
        // assistant stays assistant
        assert_eq!(messages[3]["role"], "assistant");
        // second developer -> system
        assert_eq!(messages[4]["role"], "system");
        assert_eq!(messages[4]["content"], "Always include code examples.");

        // No developer role in output
        for msg in messages {
            assert_ne!(msg["role"], "developer");
        }
    }

    #[test]
    fn responses_roles_not_affected_except_developer() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Test"},
                {"role": "assistant", "content": "OK"},
                {"type": "function_call", "call_id": "c1", "name": "fn", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "result"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        // content="OK" and tool_calls are merged into one assistant message
        assert_eq!(messages[2]["content"], "OK");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[3]["role"], "tool");

        // No developer anywhere
        for msg in messages {
            assert_ne!(msg["role"], "developer");
        }
    }

    #[test]
    fn non_function_tools_are_filtered_out_from_chat_request() {
        // Responses-only tool types (web_search_preview, computer_use_preview, file_search,
        // code_interpreter, shell) must NOT be forwarded to a Chat Completions upstream.
        let input = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "Search the web"}],
            "tools": [
                {"type": "web_search_preview"},
                {"type": "computer_use_preview"},
                {"type": "file_search", "max_num_results": 10},
                {"type": "function", "name": "get_weather", "description": "Get weather", "parameters": {"type": "object"}},
                {"type": "function", "function": {"name": "read_file", "parameters": {"type": "object"}}}
            ],
            "tool_choice": "auto"
        });

        let result = responses_to_chat_completions(input, None).unwrap();

        // Only the 2 function tools should be present
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(tools[1]["type"], "function");
        assert_eq!(tools[1]["function"]["name"], "read_file");

        // tool_choice "auto" (string) should pass through
        assert_eq!(result["tool_choice"], "auto");
    }

    #[test]
    fn responses_only_tool_choice_is_omitted() {
        // file_search tool_choice is Responses-only; Chat Completions does not support it.
        let input = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "name": "search", "parameters": {}}],
            "tool_choice": {"type": "file_search"}
        });

        let result = responses_to_chat_completions(input, None).unwrap();

        // tool_choice should not be present in the result
        assert!(result.get("tool_choice").is_none());
    }

    #[test]
    fn stream_true_injects_stream_options_include_usage() {
        // Without stream: true, no stream_options should be added
        let input_no_stream = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}]
        });
        let result_no_stream = responses_to_chat_completions(input_no_stream, None).unwrap();
        assert!(result_no_stream.get("stream_options").is_none());

        // With stream: true, stream_options.include_usage should be injected
        let input_stream = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}],
            "stream": true
        });
        let result_stream = responses_to_chat_completions(input_stream, None).unwrap();
        assert_eq!(
            result_stream["stream_options"],
            json!({"include_usage": true})
        );
        assert_eq!(result_stream["stream"], true);

        // With stream: false, no stream_options should be added
        let input_stream_false = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}],
            "stream": false
        });
        let result_stream_false = responses_to_chat_completions(input_stream_false, None).unwrap();
        assert!(result_stream_false.get("stream_options").is_none());
    }

    #[test]
    fn instructions_append_agent_loop_hint_to_first_system_message() {
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are a coding assistant.",
            "input": [{"role": "user", "content": "test"}]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let content = result["messages"][0]["content"].as_str().unwrap();

        assert!(content.starts_with("You are a coding assistant."));
        assert!(content.contains(AGENT_LOOP_HINT));
    }

    #[test]
    fn no_instructions_does_not_append_agent_loop_hint() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        // No messages since no instructions and input is just a user message
        // (user messages from input go into messages array)
        let messages = result["messages"].as_array().unwrap();
        // The user message should be present
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        // No system message at all since no instructions
        for msg in messages {
            assert_ne!(msg["role"], "system");
        }
    }

    #[test]
    fn agent_loop_hint_not_duplicated_on_repeated_conversion() {
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are concise.",
            "input": [{"role": "user", "content": "test"}]
        });

        let result1 = responses_to_chat_completions(input.clone(), None).unwrap();
        let content1 = result1["messages"][0]["content"].as_str().unwrap();
        let hint_count1 = content1.matches(AGENT_LOOP_HINT).count();
        assert_eq!(hint_count1, 1, "hint should appear exactly once");

        // Convert the same input again (simulating repeated calls)
        let result2 = responses_to_chat_completions(input.clone(), None).unwrap();
        let content2 = result2["messages"][0]["content"].as_str().unwrap();
        let hint_count2 = content2.matches(AGENT_LOOP_HINT).count();
        assert_eq!(hint_count2, 1, "hint should still appear exactly once on repeated conversion");
    }

    #[test]
    fn responses_message_plus_function_call_merged_into_single_assistant_message() {
        // Responses output: message (text) + function_call + function_call_output
        // Should become a single assistant message with both content and tool_calls,
        // followed by a tool message.
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are helpful.",
            "input": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": "Let me check the weather."},
                {"type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "Sunny"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Should be: system, user, assistant{content+tool_calls}, tool
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        // Single merged assistant message with both content and tool_calls
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "Let me check the weather.");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "get_weather");
        // Tool result
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_1");
    }

    #[test]
    fn multi_round_merge_keeps_tool_calls_with_preceding_content() {
        // Multi-round: each round has message + function_call + function_call_output
        // All rounds should merge content into the tool_calls assistant message.
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are a coding agent.",
            "input": [
                {"role": "user", "content": "Run steps 1 and 2."},
                // Round 1
                {"role": "assistant", "content": "Step 1: list files."},
                {"type": "function_call", "call_id": "c1", "name": "ls", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "main.rs"},
                // Round 2
                {"role": "assistant", "content": "Step 2: read file."},
                {"type": "function_call", "call_id": "c2", "name": "read", "arguments": "{\"file\":\"main.rs\"}"},
                {"type": "function_call_output", "call_id": "c2", "output": "fn main() {}"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Should be: system, user, assistant1{content+tool_calls}, tool1,
        //            assistant2{content+tool_calls}, tool2
        assert_eq!(messages.len(), 6);

        // Round 1: merged
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "Step 1: list files.");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "ls");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "c1");

        // Round 2: merged
        assert_eq!(messages[4]["role"], "assistant");
        assert_eq!(messages[4]["content"], "Step 2: read file.");
        assert_eq!(messages[4]["tool_calls"][0]["id"], "c2");
        assert_eq!(messages[4]["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(messages[5]["role"], "tool");
        assert_eq!(messages[5]["tool_call_id"], "c2");
    }

    // ==================== DeepSeek Thinking Mode Tests ====================

    #[test]
    fn standard_mode_omits_reasoning() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {"role": "user", "content": "test"},
                {"role": "assistant", "content": "thinking..."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I should think about this."}]},
                {"type": "function_call", "call_id": "c1", "name": "fn", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "result"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // No reasoning_content in any message in standard mode
        for msg in messages {
            assert!(
                msg.get("reasoning_content").is_none(),
                "reasoning_content should be omitted in standard mode"
            );
        }
    }

    #[test]
    fn deepseek_thinking_preserves_reasoning_content() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "test"},
                {"role": "assistant", "content": "Here is the answer."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I should think about this carefully."}]}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Should have: user, assistant{content + reasoning_content}
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Here is the answer.");
        assert_eq!(
            messages[1]["reasoning_content"],
            "I should think about this carefully."
        );
    }

    #[test]
    fn deepseek_thinking_merges_reasoning_content_with_tool_calls() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": "Let me check."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Need to call weather API."}]},
                {"type": "function_call", "call_id": "c1", "name": "get_weather", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "Sunny"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Should be: user, assistant{content + reasoning_content + tool_calls}, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Let me check.");
        assert_eq!(
            messages[1]["reasoning_content"],
            "Need to call weather API."
        );
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(messages[2]["role"], "tool");
    }

    #[test]
    fn deepseek_thinking_maps_reasoning_effort() {
        let base = json!({
            "model": "deepseek-chat",
            "input": [{"role": "user", "content": "test"}],
            "reasoning": {"effort": "PLACEHOLDER"}
        });

        for (input_effort, expected) in [
            ("low", "high"),
            ("medium", "high"),
            ("high", "high"),
            ("xhigh", "max"),
            ("max", "max"),
        ] {
            let mut input = base.clone();
            input["reasoning"]["effort"] = json!(input_effort);
            let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
            assert_eq!(
                result["reasoning_effort"], json!(expected),
                "effort '{}' should map to '{}'",
                input_effort, expected
            );
        }
    }

    #[test]
    fn deepseek_thinking_strips_sampling_params() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [{"role": "user", "content": "test"}],
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": true
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();

        assert!(
            result.get("temperature").is_none(),
            "temperature should be absent in deepseek_thinking mode"
        );
        assert!(
            result.get("top_p").is_none(),
            "top_p should be absent in deepseek_thinking mode"
        );
        // stream should still be present
        assert_eq!(result["stream"], true);
        // thinking should be injected
        assert_eq!(result["thinking"]["type"], "enabled");
    }

    #[test]
    fn standard_mode_does_not_strip_sampling_params() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": "test"}],
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": true
        });

        let result = responses_to_chat_completions(input, None).unwrap();

        assert_eq!(result["temperature"], 0.7);
        assert_eq!(result["top_p"], 0.9);
        assert_eq!(result["stream"], true);
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn deepseek_thinking_multi_round_reasoning_preserved() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "Run steps."},
                // Round 1
                {"role": "assistant", "content": "Step 1."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Reasoning 1."}]},
                {"type": "function_call", "call_id": "c1", "name": "fn1", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "r1"},
                // Round 2
                {"role": "assistant", "content": "Step 2."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Reasoning 2."}]},
                {"type": "function_call", "call_id": "c2", "name": "fn2", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c2", "output": "r2"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Round 1 assistant has reasoning 1
        assert_eq!(messages[1]["reasoning_content"], "Reasoning 1.");
        assert_eq!(messages[1]["content"], "Step 1.");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");

        // Round 2 assistant has reasoning 2
        assert_eq!(messages[3]["reasoning_content"], "Reasoning 2.");
        assert_eq!(messages[3]["content"], "Step 2.");
        assert_eq!(messages[3]["tool_calls"][0]["id"], "c2");
    }

    #[test]
    fn deepseek_thinking_injects_thinking_enabled() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [{"role": "user", "content": "test"}]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
    }

    #[test]
    fn deepseek_thinking_filters_penalty_from_passthrough() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [{"role": "user", "content": "test"}],
            "frequency_penalty": 0.5,
            "presence_penalty": 0.3,
            "metadata": {"key": "value"}
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();

        assert!(result.get("frequency_penalty").is_none());
        assert!(result.get("presence_penalty").is_none());
        // Other passthrough fields should still work
        assert_eq!(result["metadata"]["key"], "value");
    }

    #[test]
    fn deepseek_thinking_reasoning_before_assistant_and_function_call() {
        // Actual Responses API output order:
        //   reasoning -> assistant message -> function_call -> function_call_output
        // Tests that reasoning before an assistant message still merges into it.
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "What is 2+2?"},
                {"role": "assistant", "content": "The answer is 4."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Simple arithmetic."}]},
                {"type": "function_call", "call_id": "c1", "name": "calculator", "arguments": "{\"expr\":\"2+2\"}"},
                {"type": "function_call_output", "call_id": "c1", "output": "4"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // user, assistant{content + reasoning_content + tool_calls}, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "The answer is 4.");
        assert_eq!(messages[1]["reasoning_content"], "Simple arithmetic.");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[2]["role"], "tool");
    }

    #[test]
    fn standard_mode_reasoning_before_assistant_still_omitted() {
        // Same order as above, but in standard mode reasoning must be omitted.
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {"role": "user", "content": "What is 2+2?"},
                {"role": "assistant", "content": "The answer is 4."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Simple arithmetic."}]},
                {"type": "function_call", "call_id": "c1", "name": "calculator", "arguments": "{\"expr\":\"2+2\"}"},
                {"type": "function_call_output", "call_id": "c1", "output": "4"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "The answer is 4.");
        assert!(messages[1].get("reasoning_content").is_none());
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
    }

    // ==================== New DeepSeek Reasoning + Tool Calls Tests ====================

    /// DeepSeek error case: reasoning -> function_call -> function_call_output
    /// (no assistant message in between). Reasoning and tool_calls must land
    /// in the SAME assistant message.
    #[test]
    fn deepseek_thinking_reasoning_before_function_call_without_message_merges_into_tool_call_assistant()
     {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "What's the weather?"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Need to call weather API."}]},
                {"type": "function_call", "call_id": "c1", "name": "get_weather", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "Sunny"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // Must be: user, assistant{reasoning_content + tool_calls}, tool
        // NOT: user, assistant{reasoning only}, assistant{tool_calls}, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], serde_json::Value::Null);
        assert_eq!(
            messages[1]["reasoning_content"],
            "Need to call weather API."
        );
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(messages[2]["role"], "tool");

        // Verify no separate reasoning-only assistant message exists
        let assistant_count = messages
            .iter()
            .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .count();
        assert_eq!(assistant_count, 1, "should have exactly one assistant message");
    }

    /// Multiple function_calls with reasoning: all tool_calls merge into one
    /// assistant message with reasoning_content, not separate messages.
    #[test]
    fn deepseek_thinking_reasoning_with_multiple_function_calls_merges_once() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "Search and read file."},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Need to search first."}]},
                {"type": "function_call", "call_id": "c1", "name": "search", "arguments": "{\"q\":\"test\"}"},
                {"type": "function_call_output", "call_id": "c1", "output": "found: main.rs"},
                {"type": "function_call", "call_id": "c2", "name": "read", "arguments": "{\"file\":\"main.rs\"}"},
                {"type": "function_call_output", "call_id": "c2", "output": "fn main() {}"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // First round: assistant{reasoning + tool_calls(c1)}, tool(c1)
        // Second round: assistant{tool_calls(c2)}, tool(c2)
        // Reasoning only merges into the FIRST round (where it was captured).
        assert_eq!(messages.len(), 5);

        // Round 1: assistant with reasoning_content and tool_calls
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["reasoning_content"], "Need to search first.");
        assert_eq!(messages[1]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "c1");

        // Round 2: assistant with tool_calls only (reasoning was consumed by round 1)
        assert_eq!(messages[3]["role"], "assistant");
        assert!(messages[3].get("reasoning_content").is_none());
        assert_eq!(messages[3]["tool_calls"][0]["id"], "c2");
        assert_eq!(messages[4]["role"], "tool");
        assert_eq!(messages[4]["tool_call_id"], "c2");
    }

    /// Standard mode: reasoning before function_call without message must still
    /// be omitted — no reasoning_content on assistant message.
    #[test]
    fn standard_mode_reasoning_before_function_call_without_message_still_omitted() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {"role": "user", "content": "test"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I should think."}]},
                {"type": "function_call", "call_id": "c1", "name": "fn", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "r1"}
            ]
        });

        let result = responses_to_chat_completions(input, None).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // user, assistant{tool_calls}, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert!(
            messages[1].get("reasoning_content").is_none(),
            "reasoning_content must be absent in standard mode"
        );
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
    }

    /// DeepSeek: reasoning -> assistant message -> function_call -> function_call_output
    /// Reasoning must merge into the assistant message that has tool_calls.
    #[test]
    fn deepseek_thinking_reasoning_before_message_and_function_call_still_merges() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "What is 2+2?"},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I should check my work."}]},
                {"role": "assistant", "content": "Let me verify."},
                {"type": "function_call", "call_id": "c1", "name": "calculator", "arguments": "{\"expr\":\"2+2\"}"},
                {"type": "function_call_output", "call_id": "c1", "output": "4"}
            ]
        });

        let result = responses_to_chat_completions(input, Some("deepseek_thinking")).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // user, assistant{content + reasoning_content + tool_calls}, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Let me verify.");
        assert_eq!(
            messages[1]["reasoning_content"],
            "I should check my work."
        );
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "calculator");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "c1");

        // No separate reasoning-only assistant message
        let assistant_count = messages
            .iter()
            .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .count();
        assert_eq!(assistant_count, 1);
    }
}
