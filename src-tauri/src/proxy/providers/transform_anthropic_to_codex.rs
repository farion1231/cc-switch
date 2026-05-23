//! Anthropic Messages API response → OpenAI format converters
//!
//! Converts Anthropic Messages API responses (both non-streaming JSON and
//! accumulated from streaming) into OpenAI Chat Completions or Responses API
//! format. Used when Codex CLI talks to an Anthropic upstream.

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// Convert an Anthropic Messages API response to OpenAI Chat Completions format.
///
/// Input: Anthropic `{id, type:"message", role:"assistant", content:[...], model, stop_reason, usage}`
/// Output: OpenAI Chat `{id, object:"chat.completion", model, choices:[...], usage}`
pub fn anthropic_to_chat_completion_response(body: Value) -> Result<Value, ProxyError> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("chatcmpl-unknown");
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let stop_reason = body.get("stop_reason").and_then(|v| v.as_str());

    let content_blocks = body
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    // Collect text content
    let mut text_parts: Vec<String> = Vec::new();
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut tool_call_index: usize = 0;

    for block in &content_blocks {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        text_parts.push(text.to_string());
                    }
                }
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                    if !thinking.is_empty() {
                        reasoning_parts.push(thinking.to_string());
                    }
                }
            }
            Some("tool_use") => {
                let tc_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let input = block.get("input").cloned().unwrap_or(json!({}));
                let args_str = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());

                tool_calls.push(json!({
                    "index": tool_call_index,
                    "id": tc_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": args_str
                    }
                }));
                tool_call_index += 1;
            }
            _ => {}
        }
    }

    // Build message
    let mut message = json!({
        "role": "assistant"
    });

    let text_content = text_parts.join("");
    if !text_content.is_empty() {
        message["content"] = json!(text_content);
    } else if tool_calls.is_empty() {
        message["content"] = Value::Null;
    }

    // reasoning_content (DeepSeek/MiMo style)
    let reasoning_content = reasoning_parts.join("");
    if !reasoning_content.is_empty() {
        message["reasoning_content"] = json!(reasoning_content);
    }

    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }

    // Map stop_reason
    let finish_reason = map_anthropic_stop_reason(stop_reason, !tool_calls.is_empty());

    let usage = body.get("usage").cloned().unwrap_or(json!({}));

    Ok(json!({
        "id": format!("chatcmpl-{}", id),
        "object": "chat.completion",
        "created": chrono_now(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": anthropic_usage_to_openai_chat(&usage)
    }))
}

/// Convert an Anthropic Messages API response to OpenAI Responses API format.
///
/// Input: Anthropic `{id, type:"message", role:"assistant", content:[...], model, stop_reason, usage}`
/// Output: Responses `{id, object:"response", status, model, output:[...], usage}`
pub fn anthropic_to_responses_response(body: Value) -> Result<Value, ProxyError> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp-unknown");
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let stop_reason = body.get("stop_reason").and_then(|v| v.as_str());

    let content_blocks = body
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let mut output: Vec<Value> = Vec::new();
    let mut has_tool_use = false;

    // Check for reasoning/thinking first
    let thinking_text = content_blocks
        .iter()
        .filter_map(|b| {
            if b.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                b.get("thinking").and_then(|t| t.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");

    if !thinking_text.is_empty() {
        output.push(json!({
            "id": format!("rs_{id}"),
            "type": "reasoning",
            "summary": [{
                "type": "summary_text",
                "text": thinking_text
            }]
        }));
    }

    // Collect text content
    let text_content = content_blocks
        .iter()
        .filter_map(|b| {
            if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                b.get("text").and_then(|t| t.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");

    if !text_content.is_empty() {
        output.push(json!({
            "id": format!("msg_{id}"),
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text_content,
                "annotations": []
            }]
        }));
    }

    // Tool calls
    for block in &content_blocks {
        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
            has_tool_use = true;
            let tc_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let input = block.get("input").cloned().unwrap_or(json!({}));
            let args_str = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());

            output.push(json!({
                "id": format!("fc_{tc_id}"),
                "type": "function_call",
                "call_id": tc_id,
                "name": name,
                "arguments": args_str
            }));
        }
    }

    let status = map_anthropic_status(stop_reason);
    let usage = body.get("usage").cloned().unwrap_or(json!({}));

    let mut response = json!({
        "id": id,
        "object": "response",
        "created_at": chrono_now(),
        "status": status,
        "model": model,
        "output": output,
        "usage": anthropic_usage_to_responses(&usage)
    });

    if stop_reason == Some("max_tokens") {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }

    Ok(response)
}

fn map_anthropic_stop_reason(stop_reason: Option<&str>, has_tool_use: bool) -> &'static str {
    match stop_reason {
        Some("end_turn") => "stop",
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        Some("stop_sequence") => "stop",
        _ => {
            if has_tool_use {
                "tool_calls"
            } else {
                "stop"
            }
        }
    }
}

fn map_anthropic_status(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("end_turn") | Some("stop_sequence") => "completed",
        Some("max_tokens") => "incomplete",
        Some("tool_use") => "completed",
        _ => "completed",
    }
}

pub(crate) fn anthropic_usage_to_openai_chat(usage: &Value) -> Value {
    let prompt_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mut result = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": prompt_tokens + completion_tokens
    });

    // Cache tokens
    if let Some(cached) = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
    {
        result["prompt_tokens_details"] = json!({ "cached_tokens": cached });
    }

    result
}

pub(crate) fn anthropic_usage_to_responses(usage: &Value) -> Value {
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens
    });

    if let Some(cached) = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
    {
        result["input_tokens_details"] = json!({ "cached_tokens": cached });
    }

    result
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_chat_completion_text_only() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = anthropic_to_chat_completion_response(anthropic_response).unwrap();
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["model"], "claude-sonnet-4-6");
        assert_eq!(result["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 5);
    }

    #[test]
    fn test_anthropic_to_chat_completion_tool_use() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me search."},
                {"type": "tool_use", "id": "toolu_123", "name": "search", "input": {"query": "test"}}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = anthropic_to_chat_completion_response(anthropic_response).unwrap();
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
        let tool_calls = result["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["function"]["name"], "search");
    }

    #[test]
    fn test_anthropic_to_chat_completion_thinking() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think..."},
                {"type": "text", "text": "The answer is 42."}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 30}
        });

        let result = anthropic_to_chat_completion_response(anthropic_response).unwrap();
        assert_eq!(
            result["choices"][0]["message"]["reasoning_content"],
            "Let me think..."
        );
        assert_eq!(
            result["choices"][0]["message"]["content"],
            "The answer is 42."
        );
    }

    #[test]
    fn test_anthropic_to_responses_text_only() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = anthropic_to_responses_response(anthropic_response).unwrap();
        assert_eq!(result["object"], "response");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["model"], "claude-sonnet-4-6");
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "Hello!");
    }

    #[test]
    fn test_anthropic_to_responses_with_tool_use() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "toolu_123", "name": "search", "input": {"q": "test"}}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = anthropic_to_responses_response(anthropic_response).unwrap();
        assert_eq!(result["status"], "completed");
        let output = result["output"].as_array().unwrap();
        assert_eq!(output[0]["type"], "function_call");
        assert_eq!(output[0]["name"], "search");
        assert_eq!(output[0]["call_id"], "toolu_123");
    }

    #[test]
    fn test_anthropic_to_responses_with_thinking() {
        let anthropic_response = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Reasoning..."},
                {"type": "text", "text": "Answer."}
            ],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 30}
        });

        let result = anthropic_to_responses_response(anthropic_response).unwrap();
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(output[1]["type"], "message");
    }
}
