//! Codex 格式转换: OpenAI Responses API ↔ OpenAI Chat Completions
//!
//! When a Codex provider has api_format = "openai_chat", convert:
//!  Request:  Responses API → Chat Completions
//!  Response: Chat Completions → Responses API

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};
use uuid::Uuid;

/// Convert Responses API request → Chat Completions request
pub fn responses_to_chat_completions(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // model passthrough
    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    // Build messages array
    let mut messages: Vec<Value> = Vec::new();

    // instructions → system message
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.is_empty() {
            messages.push(json!({"role": "system", "content": instructions}));
        }
    }

    // input → user/assistant/tool messages
    if let Some(input) = body.get("input") {
        if let Some(text) = input.as_str() {
            // Simple string input
            messages.push(json!({"role": "user", "content": text}));
        } else if let Some(items) = input.as_array() {
            // Array of input items
            convert_input_items_to_messages(items, &mut messages);
        }
    }

    result["messages"] = json!(messages);

    // tools conversion: Responses format → Chat Completions format
    // Responses: {type: "function", name: "foo", description: "...", parameters: {...}}
    // Chat:      {type: "function", function: {name: "foo", description: "...", parameters: {...}}}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let chat_tools: Vec<Value> = tools
            .iter()
            .filter(|t| {
                // Skip tools with empty names and non-function types
                let is_function =
                    t.get("type").and_then(|v| v.as_str()) == Some("function");
                let has_name = t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .is_some_and(|n| !n.is_empty());
                is_function && has_name
            })
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "parameters": t.get("parameters").cloned().unwrap_or(json!({"type": "object"})),
                    }
                })
            })
            .collect();
        if !chat_tools.is_empty() {
            result["tools"] = json!(chat_tools);
        }
    }

    // tool_choice: map object format for Chat Completions
    if let Some(tc) = body.get("tool_choice") {
        result["tool_choice"] = match tc {
            // Responses API: {"type": "function", "name": "foo"}
            // Chat Completions: {"type": "function", "function": {"name": "foo"}}
            Value::Object(obj) if obj.get("type").and_then(|t| t.as_str()) == Some("function") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({"type": "function", "function": {"name": name}})
            }
            // String values ("auto", "required", "none") pass through unchanged
            _ => tc.clone(),
        };
    }

    // max_output_tokens → max_tokens
    if let Some(v) = body.get("max_output_tokens") {
        result["max_tokens"] = v.clone();
    }

    // Direct passthrough params
    for key in &["temperature", "top_p", "stream"] {
        if let Some(v) = body.get(*key) {
            result[*key] = v.clone();
        }
    }

    Ok(result)
}

/// Convert input items array to Chat Completions messages
fn convert_input_items_to_messages(items: &[Value], messages: &mut Vec<Value>) {
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match item_type {
            "message" => {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                if let Some(content) = item.get("content") {
                    if let Some(text) = content.as_str() {
                        messages.push(json!({"role": role, "content": text}));
                    } else if let Some(parts) = content.as_array() {
                        let text_parts: Vec<String> = parts
                            .iter()
                            .filter_map(|p| {
                                let ptype = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                match ptype {
                                    "input_text" | "output_text" | "text" => p
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    _ => None,
                                }
                            })
                            .collect();
                        if !text_parts.is_empty() {
                            messages.push(json!({"role": role, "content": text_parts.join("")}));
                        }
                    }
                }
            }
            "function_call" => {
                // Responses API function_call → Chat Completions assistant message with tool_calls
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                messages.push(json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments
                        }
                    }]
                }));
            }
            "function_call_output" => {
                // Responses API function_call_output → Chat Completions tool message
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output
                }));
            }
            _ => {
                // Skip unknown types (web_search, etc.)
            }
        }
    }
}

/// Convert Chat Completions response → Responses API response
pub fn chat_completions_to_responses(body: Value) -> Result<Value, ProxyError> {
    let id = format!("resp_{}", Uuid::new_v4().to_string().replace('-', ""));
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut output: Vec<Value> = Vec::new();

    let empty_obj = json!({});
    if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            let message = choice.get("message").unwrap_or(&empty_obj);
            let role = message
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("assistant");

            // Handle tool_calls
            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tool_calls {
                    let func = tc.get("function").unwrap_or(&empty_obj);
                    output.push(json!({
                        "type": "function_call",
                        "id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "call_id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": func.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "arguments": func.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}"),
                        "status": "completed"
                    }));
                }
            }

            // Handle text content
            if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    let msg_id = format!("msg_{}", Uuid::new_v4().to_string().replace('-', ""));
                    output.push(json!({
                        "type": "message",
                        "id": msg_id,
                        "role": role,
                        "status": "completed",
                        "content": [{
                            "type": "output_text",
                            "text": content,
                            "annotations": []
                        }]
                    }));
                }
            }
        }
    }

    // Build usage
    let usage = if let Some(u) = body.get("usage") {
        json!({
            "input_tokens": u.get("prompt_tokens").cloned().unwrap_or(json!(0)),
            "output_tokens": u.get("completion_tokens").cloned().unwrap_or(json!(0)),
            "total_tokens": u.get("total_tokens").cloned().unwrap_or(json!(0))
        })
    } else {
        json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0})
    };

    let result = json!({
        "id": id,
        "object": "response",
        "model": model,
        "status": "completed",
        "output": output,
        "usage": usage,
        "metadata": {},
        "temperature": 1.0,
        "top_p": null,
        "max_output_tokens": null,
        "previous_response_id": null,
        "reasoning": {},
        "text": {},
        "truncation": null,
        "incomplete_details": null,
        "instructions": null,
        "tool_choice": "auto",
        "tools": [],
        "parallel_tool_calls": false
    });

    Ok(result)
}

/// Convert Responses API request → Anthropic Messages API request
pub fn responses_to_anthropic_messages(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // model passthrough
    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    // instructions → system
    let mut system_parts: Vec<String> = Vec::new();
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.is_empty() {
            system_parts.push(instructions.to_string());
        }
    }

    // Build messages array from input
    let mut messages: Vec<Value> = Vec::new();
    if let Some(input) = body.get("input") {
        if let Some(text) = input.as_str() {
            messages.push(json!({"role": "user", "content": [{"type": "text", "text": text}]}));
        } else if let Some(items) = input.as_array() {
            convert_input_items_to_anthropic_messages(items, &mut messages, &mut system_parts);
        }
    }

    // Set system prompt (from instructions + developer role messages)
    if !system_parts.is_empty() {
        result["system"] = json!(system_parts.join("\n\n"));
    }

    // Merge consecutive same-role messages (Anthropic requires alternating roles)
    let merged_messages = merge_consecutive_role_messages(messages);
    result["messages"] = json!(merged_messages);

    // tools conversion: Responses flat format → Anthropic format
    // Responses: {type: "function", name, description, parameters}
    // Anthropic: {name, description, input_schema: parameters}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let anthropic_tools: Vec<Value> = tools
            .iter()
            .filter(|t| {
                t.get("type").and_then(|v| v.as_str()) == Some("function")
                    && t.get("name")
                        .and_then(|v| v.as_str())
                        .is_some_and(|n| !n.is_empty())
            })
            .map(|t| {
                json!({
                    "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "input_schema": t.get("parameters").cloned().unwrap_or(json!({"type": "object"}))
                })
            })
            .collect();
        if !anthropic_tools.is_empty() {
            result["tools"] = json!(anthropic_tools);
        }
    }

    // tool_choice mapping
    if let Some(tc) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_responses_to_anthropic(tc);
    }

    // max_output_tokens → max_tokens (required by Anthropic API)
    if let Some(v) = body.get("max_output_tokens") {
        result["max_tokens"] = v.clone();
    } else {
        // Default max_tokens — Anthropic API requires this field
        result["max_tokens"] = json!(16384);
    }

    // Direct passthrough
    for key in &["temperature", "top_p", "stream"] {
        if let Some(v) = body.get(*key) {
            result[*key] = v.clone();
        }
    }

    // reasoning → thinking
    if let Some(reasoning) = body.get("reasoning") {
        if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
            let budget = match effort {
                "low" => 4096,
                "medium" => 10000,
                "high" | "xhigh" => 32000,
                _ => 10000,
            };
            result["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
            // Ensure max_tokens > budget_tokens
            if let Some(max) = result.get("max_tokens").and_then(|v| v.as_u64()) {
                if max <= budget {
                    result["max_tokens"] = json!(budget + max);
                }
            }
        }
    }

    Ok(result)
}

/// Convert input items array to Anthropic Messages API messages.
///
/// Anthropic requires alternating user/assistant messages with specific rules:
/// - `developer` role messages are extracted to system_parts (Anthropic system prompt)
/// - All tool_use blocks from the same turn must be in ONE assistant message
/// - All tool_result blocks must be in ONE user message following the assistant
/// - function_call items after an assistant message are merged into that message
/// - Consecutive function_call_output items are merged into one user message
fn convert_input_items_to_anthropic_messages(
    items: &[Value],
    messages: &mut Vec<Value>,
    system_parts: &mut Vec<String>,
) {
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match item_type {
            "message" => {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");

                // developer/system role → extract to system_parts, not messages
                if role == "developer" || role == "system" {
                    if let Some(content) = item.get("content") {
                        if let Some(text) = content.as_str() {
                            system_parts.push(text.to_string());
                        } else if let Some(parts) = content.as_array() {
                            for p in parts {
                                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                                    system_parts.push(t.to_string());
                                }
                            }
                        }
                    }
                    continue;
                }

                if let Some(content) = item.get("content") {
                    if let Some(text) = content.as_str() {
                        messages.push(
                            json!({"role": role, "content": [{"type": "text", "text": text}]}),
                        );
                    } else if let Some(parts) = content.as_array() {
                        let blocks: Vec<Value> = parts
                            .iter()
                            .filter_map(|p| {
                                let ptype = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                match ptype {
                                    "input_text" | "output_text" | "text" => p
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .map(|t| json!({"type": "text", "text": t})),
                                    _ => None,
                                }
                            })
                            .collect();
                        if !blocks.is_empty() {
                            messages.push(json!({"role": role, "content": blocks}));
                        }
                    }
                }
            }
            "function_call" => {
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args_str = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                let tool_use_block = json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                });

                // Merge into last assistant message if possible
                let merged = if let Some(last) = messages.last_mut() {
                    if last.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                        if let Some(content) =
                            last.get_mut("content").and_then(|c| c.as_array_mut())
                        {
                            content.push(tool_use_block.clone());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !merged {
                    messages.push(json!({
                        "role": "assistant",
                        "content": [tool_use_block]
                    }));
                }
            }
            "function_call_output" => {
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
                let tool_result_block = json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": output
                });

                // Merge into last user message if it contains tool_results
                let merged = if let Some(last) = messages.last_mut() {
                    if last.get("role").and_then(|r| r.as_str()) == Some("user") {
                        if let Some(content) =
                            last.get_mut("content").and_then(|c| c.as_array_mut())
                        {
                            let has_tool_result = content.iter().any(|b| {
                                b.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                            });
                            if has_tool_result {
                                content.push(tool_result_block.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !merged {
                    messages.push(json!({
                        "role": "user",
                        "content": [tool_result_block]
                    }));
                }
            }
            _ => {}
        }
    }
}

/// Merge consecutive messages with the same role.
/// Anthropic requires strictly alternating user/assistant roles.
fn merge_consecutive_role_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let should_merge = if let Some(last) = merged.last() {
            last.get("role").and_then(|r| r.as_str()) == Some(role)
        } else {
            false
        };

        if should_merge {
            if let Some(last) = merged.last_mut() {
                // Normalize new content to array
                let new_blocks: Vec<Value> = match msg.get("content") {
                    Some(Value::Array(arr)) => arr.clone(),
                    Some(Value::String(s)) => vec![json!({"type": "text", "text": s})],
                    _ => vec![],
                };
                if new_blocks.is_empty() {
                    continue;
                }
                // Normalize existing content to array if it's a string
                if let Some(existing_str) = last
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
                {
                    last["content"] = json!([{"type": "text", "text": existing_str}]);
                }
                // Now merge into existing array
                if let Some(existing) = last.get_mut("content").and_then(|c| c.as_array_mut()) {
                    existing.extend(new_blocks);
                }
            }
        } else {
            merged.push(msg);
        }
    }
    merged
}

fn map_tool_choice_responses_to_anthropic(tc: &Value) -> Value {
    match tc {
        Value::String(s) => match s.as_str() {
            "required" => json!({"type": "any"}),
            "auto" => json!({"type": "auto"}),
            "none" => json!({"type": "auto"}), // Anthropic has no "none"; fallback to auto
            _ => tc.clone(),
        },
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("function") {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({"type": "tool", "name": name})
            } else {
                tc.clone()
            }
        }
        _ => tc.clone(),
    }
}

/// Convert Anthropic Messages API response → Responses API response
pub fn anthropic_messages_to_responses(body: Value) -> Result<Value, ProxyError> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| format!("resp_{}", s.trim_start_matches("msg_")))
        .unwrap_or_else(|| format!("resp_{}", Uuid::new_v4().to_string().replace('-', "")));

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let stop_reason = body.get("stop_reason").and_then(|v| v.as_str());

    let mut output: Vec<Value> = Vec::new();

    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            let msg_id =
                                format!("msg_{}", Uuid::new_v4().to_string().replace('-', ""));
                            output.push(json!({
                                "type": "message",
                                "id": msg_id,
                                "role": "assistant",
                                "status": "completed",
                                "content": [{"type": "output_text", "text": text, "annotations": []}]
                            }));
                        }
                    }
                }
                "tool_use" => {
                    let tool_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                    output.push(json!({
                        "type": "function_call",
                        "id": tool_id,
                        "call_id": tool_id,
                        "name": name,
                        "arguments": arguments,
                        "status": "completed"
                    }));
                }
                "thinking" => {
                    if let Some(thinking_text) = block.get("thinking").and_then(|t| t.as_str()) {
                        if !thinking_text.is_empty() {
                            output.push(json!({
                                "type": "reasoning",
                                "id": format!("rs_{}", Uuid::new_v4().to_string().replace('-', "")),
                                "summary": [{"type": "summary_text", "text": thinking_text}]
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Map stop_reason → status
    let (status, incomplete_details) = match stop_reason {
        Some("end_turn") | Some("tool_use") => ("completed", None),
        Some("max_tokens") => ("incomplete", Some(json!({"reason": "max_output_tokens"}))),
        _ => ("completed", None),
    };

    // Map usage
    let usage = if let Some(u) = body.get("usage") {
        let input_tokens = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output_tokens = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let mut usage_json = json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        });
        if let Some(v) = u.get("cache_read_input_tokens") {
            usage_json["cache_read_input_tokens"] = v.clone();
        }
        if let Some(v) = u.get("cache_creation_input_tokens") {
            usage_json["cache_creation_input_tokens"] = v.clone();
        }
        usage_json
    } else {
        json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0})
    };

    let mut result = json!({
        "id": id,
        "object": "response",
        "model": model,
        "status": status,
        "output": output,
        "usage": usage,
        "metadata": {},
        "temperature": 1.0,
        "top_p": null,
        "max_output_tokens": null,
        "previous_response_id": null,
        "reasoning": {},
        "text": {},
        "truncation": null,
        "instructions": null,
        "tool_choice": "auto",
        "tools": [],
        "parallel_tool_calls": false
    });

    if let Some(details) = incomplete_details {
        result["incomplete_details"] = details;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_responses_to_chat_completions_string_input() {
        let body = json!({
            "model": "gpt-4o",
            "instructions": "You are a helpful assistant.",
            "input": "Hello, how are you?"
        });

        let result = responses_to_chat_completions(body).unwrap();

        assert_eq!(result["model"], "gpt-4o");
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello, how are you?");
    }

    #[test]
    fn test_responses_to_chat_completions_array_input() {
        let body = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": "Hello!"
                }
            ]
        });

        let result = responses_to_chat_completions(body).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello!");
    }

    #[test]
    fn test_responses_to_chat_completions_filters_empty_tools() {
        let body = json!({
            "model": "gpt-4o",
            "input": "test",
            "tools": [
                {
                    "type": "function",
                    "name": "",
                    "description": "empty name tool"
                },
                {
                    "type": "function",
                    "name": "my_tool",
                    "description": "a valid tool",
                    "parameters": {"type": "object"}
                }
            ]
        });

        let result = responses_to_chat_completions(body).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "my_tool");
    }

    #[test]
    fn test_responses_to_chat_completions_function_call_items() {
        let body = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "get_weather",
                    "arguments": "{\"city\":\"Paris\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_abc",
                    "output": "Sunny, 22°C"
                }
            ]
        });

        let result = responses_to_chat_completions(body).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);

        // function_call → assistant with tool_calls
        assert_eq!(messages[0]["role"], "assistant");
        let tool_calls = messages[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls[0]["id"], "call_abc");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");

        // function_call_output → tool message
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_abc");
        assert_eq!(messages[1]["content"], "Sunny, 22°C");
    }

    #[test]
    fn test_responses_to_chat_completions_max_output_tokens() {
        let body = json!({
            "model": "gpt-4o",
            "input": "test",
            "max_output_tokens": 1024
        });

        let result = responses_to_chat_completions(body).unwrap();
        assert_eq!(result["max_tokens"], 1024);
    }

    #[test]
    fn test_chat_completions_to_responses_basic() {
        let body = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 8,
                "total_tokens": 18
            }
        });

        let result = chat_completions_to_responses(body).unwrap();

        assert_eq!(result["object"], "response");
        assert_eq!(result["model"], "gpt-4o");
        assert_eq!(result["status"], "completed");

        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["role"], "assistant");
        let content = output[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "output_text");
        assert_eq!(content[0]["text"], "Hello! How can I help you?");

        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 8);
    }

    #[test]
    fn test_chat_completions_to_responses_tool_calls() {
        let body = json!({
            "model": "gpt-4o",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                }
            }]
        });

        let result = chat_completions_to_responses(body).unwrap();
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "function_call");
        assert_eq!(output[0]["call_id"], "call_xyz");
        assert_eq!(output[0]["name"], "get_weather");
        assert_eq!(output[0]["arguments"], "{\"city\":\"Paris\"}");
        assert_eq!(output[0]["status"], "completed");
    }

    // ============= Anthropic Transform Tests =============

    #[test]
    fn test_responses_to_anthropic_string_input() {
        let body = json!({
            "model": "o3-mini",
            "instructions": "You are helpful.",
            "input": "Hello!"
        });
        let result = responses_to_anthropic_messages(body).unwrap();
        assert_eq!(result["model"], "o3-mini");
        assert_eq!(result["system"], "You are helpful.");
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"][0]["type"], "text");
        assert_eq!(msgs[0]["content"][0]["text"], "Hello!");
    }

    #[test]
    fn test_responses_to_anthropic_array_input() {
        let body = json!({
            "model": "o3-mini",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hello!"}]}
            ]
        });
        let result = responses_to_anthropic_messages(body).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"][0]["type"], "text");
        assert_eq!(msgs[1]["role"], "assistant");
    }

    #[test]
    fn test_responses_to_anthropic_with_tools() {
        let body = json!({
            "model": "o3-mini",
            "input": "test",
            "tools": [
                {"type": "function", "name": "get_weather", "description": "Get weather", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}
            ]
        });
        let result = responses_to_anthropic_messages(body).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert!(tools[0].get("input_schema").is_some());
        assert!(tools[0].get("parameters").is_none());
    }

    #[test]
    fn test_responses_to_anthropic_function_call_items() {
        let body = json!({
            "model": "o3-mini",
            "input": [
                {"type": "function_call", "call_id": "call_abc", "name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"},
                {"type": "function_call_output", "call_id": "call_abc", "output": "Sunny, 25°C"}
            ]
        });
        let result = responses_to_anthropic_messages(body).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        // function_call → assistant with tool_use
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["content"][0]["type"], "tool_use");
        assert_eq!(msgs[0]["content"][0]["id"], "call_abc");
        assert_eq!(msgs[0]["content"][0]["name"], "get_weather");
        assert_eq!(msgs[0]["content"][0]["input"]["city"], "Tokyo");
        // function_call_output → user with tool_result
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[1]["content"][0]["tool_use_id"], "call_abc");
    }

    #[test]
    fn test_responses_to_anthropic_max_output_tokens() {
        let body = json!({"model": "o3-mini", "input": "test", "max_output_tokens": 4096});
        let result = responses_to_anthropic_messages(body).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn test_responses_to_anthropic_reasoning_effort() {
        let body = json!({"model": "o3-mini", "input": "test", "max_output_tokens": 4096, "reasoning": {"effort": "high"}});
        let result = responses_to_anthropic_messages(body).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 32000);
        // max_tokens must be > budget_tokens
        assert!(result["max_tokens"].as_u64().unwrap() > 32000);
    }

    #[test]
    fn test_anthropic_to_responses_basic() {
        let body = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-opus-4-6-v1",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let result = anthropic_messages_to_responses(body).unwrap();
        assert_eq!(result["object"], "response");
        assert_eq!(result["model"], "claude-opus-4-6-v1");
        assert_eq!(result["status"], "completed");
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["type"], "output_text");
        assert_eq!(output[0]["content"][0]["text"], "Hello!");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
        assert_eq!(result["usage"]["total_tokens"], 15);
    }

    #[test]
    fn test_anthropic_to_responses_tool_use() {
        let body = json!({
            "id": "msg_abc",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"city": "Tokyo"}}],
            "model": "claude-opus-4-6-v1",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 15}
        });
        let result = anthropic_messages_to_responses(body).unwrap();
        assert_eq!(result["status"], "completed");
        let output = result["output"].as_array().unwrap();
        assert_eq!(output[0]["type"], "function_call");
        assert_eq!(output[0]["call_id"], "call_123");
        assert_eq!(output[0]["name"], "get_weather");
        assert_eq!(output[0]["arguments"], "{\"city\":\"Tokyo\"}");
    }

    #[test]
    fn test_anthropic_to_responses_thinking() {
        let body = json!({
            "id": "msg_abc",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think about this..."},
                {"type": "text", "text": "The answer is 42"}
            ],
            "model": "claude-opus-4-6-v1",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 30, "output_tokens": 25}
        });
        let result = anthropic_messages_to_responses(body).unwrap();
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(
            output[0]["summary"][0]["text"],
            "Let me think about this..."
        );
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["text"], "The answer is 42");
    }

    #[test]
    fn test_anthropic_to_responses_max_tokens_stop() {
        let body = json!({
            "id": "msg_abc",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Partial..."}],
            "model": "claude-opus-4-6-v1",
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 10, "output_tokens": 4096}
        });
        let result = anthropic_messages_to_responses(body).unwrap();
        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
    }
}
