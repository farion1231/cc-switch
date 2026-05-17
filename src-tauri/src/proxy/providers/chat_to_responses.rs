//! Chat Completions → OpenAI Responses API 响应翻译
//!
//! DeepSeek 等上游返回 Chat Completions 格式，Codex CLI 期望 Responses API 格式。
//! 本模块实现反向转换，包含非流式和流式（SSE 事件映射）两条路径。
//!
//! 同时处理 DeepSeek reasoning_content → Responses API reasoning output item + reasoning_text 转换。

use crate::proxy::error::ProxyError;
use bytes::Bytes;
use futures::stream::{self, Stream, StreamExt};
use serde_json::{json, Value};
use std::pin::Pin;

/// Chat Completions 非流式响应 → Responses API 响应
///
/// `request_body` 用于推理原请求中的 reasoning 参数，以决定是否输出 reasoning item。
/// 检测上游 Chat Completions 响应是否为错误
fn is_upstream_error(body: &Value) -> Option<&str> {
    body.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
}

pub fn chat_to_responses(
    body: &Value,
    request_body: Option<&Value>,
) -> Result<Value, ProxyError> {
    // Debug: log upstream chat response
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let truncated = if body_str.len() > 800 {
        // 安全截断（防止 UTF-8 边界 panic）：从 byte 800 回溯到最近的 char boundary
        let mut end = 800;
        while end > 0 && !body_str.is_char_boundary(end) {
            end -= 1;
        }
        &body_str[..end]
    } else {
        &body_str
    };
    log::info!("[Codex] <<< Upstream Chat response (truncated): {}", truncated);

    // 检测上游错误（如 DeepSeek 返回 400 但 forwarder 未拦截）
    if let Some(err_msg) = is_upstream_error(body) {
        // 返回 Responses API 格式的错误，让 Codex CLI 正确识别
        log::warn!("[Codex] Upstream error detected: {}", err_msg);
        return Ok(json!({
            "error": {
                "message": err_msg,
                "type": "upstream_error",
            }
        }));
    }

    let root = body;
    let model = root.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");
    let id = root
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("chatcmpl-unknown");

    let mut response = json!({
        "id": format!("resp_{}", id.trim_start_matches("chatcmpl-")),
        "object": "response",
        "created": root.get("created"),
        "model": model,
        "status": "completed",
        "output": [],
        "usage": {},
    });

    // usage
    if let Some(usage) = root.get("usage") {
        response["usage"] = json!({
            "input_tokens": usage.get("prompt_tokens").or_else(|| usage.get("input_tokens")),
            "output_tokens": usage.get("completion_tokens").or_else(|| usage.get("output_tokens")),
            "total_tokens": usage.get("total_tokens"),
        });
    }

    // choices → output items
    if let Some(choices) = root.get("choices").and_then(|v| v.as_array()) {
        let mut output_items: Vec<Value> = Vec::new();

        for (ci, choice) in choices.iter().enumerate() {
            if let Some(msg) = choice.get("message") {
                // 0. Check for reasoning_content
                let rc_text = msg
                    .get("reasoning_content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // 1. Reasoning item (if reasoning_content present)
                if !rc_text.is_empty() {
                    let rid = format!("rs_{}", id.trim_start_matches("chatcmpl-"));
                    let reasoning_item = json!({
                        "id": rid,
                        "type": "reasoning",
                        "status": "completed",
                        "encrypted_content": "",
                        "summary": [{"type": "summary_text", "text": rc_text}],
                    });
                    output_items.push(reasoning_item);
                }

                // 2. Message item
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let mut content_parts: Vec<Value> = Vec::new();

                // reasoning_text content part (for round-trip echo-back)
                if !rc_text.is_empty() {
                    content_parts.push(json!({
                        "type": "reasoning_text",
                        "text": rc_text,
                    }));
                }

                // output_text content part
                content_parts.push(json!({
                    "type": "output_text",
                    "text": content,
                    "annotations": [],
                }));

                let msg_item = json!({
                    "id": format!("msg_{}_{}", id.trim_start_matches("chatcmpl-"), ci),
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": content_parts,
                });
                output_items.push(msg_item);

                // 3. Tool call items
                if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        let fc_item = json!({
                            "id": tc.get("id"),
                            "type": "function_call",
                            "status": "completed",
                            "name": tc.pointer("/function/name"),
                            "call_id": tc.get("id"),
                            "arguments": tc.pointer("/function/arguments"),
                        });
                        output_items.push(fc_item);
                    }
                }
            }
        }

        response["output"] = json!(output_items);

        // finish_reason
        if let Some(choice) = choices.first() {
            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                response["status"] = json!("completed");
            }
        }
    }

    Ok(response)
}

/// Chat Completions 流式 SSE → Responses API SSE 流
///
/// 接收 Chat Completions 的 SSE chunk 流 (delta-based)，
/// 输出 Responses API 的命名事件 (named event) 流。
pub fn create_chat_compat_sse_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let cached_model: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let resp_id: String = format!("resp_{}", uuid::Uuid::new_v4());

    // State machine state
    enum State {
        Idle,
        Reasoning {
            item_id: String,
            output_index: u64,
        },
        Message {
            idx: usize,
            output_index: u64,
            item_id: String,
        },
    }

    let state = std::cell::RefCell::new(State::Idle);

    let emitted_response_created = std::sync::atomic::AtomicBool::new(false);

    let mapped = stream
        .flat_map(move |chunk_result| {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => return stream::iter(vec![Err(e)]).left_stream(),
            };

            let chunk_str = String::from_utf8_lossy(&chunk);
            let mut events: Vec<Result<Bytes, std::io::Error>> = Vec::new();

            log::info!("[Codex SSE] Received chunk: {} bytes", chunk.len());

            // Emit response.created and response.in_progress on the first chunk
            if !emitted_response_created.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let created_event = format!(
                    "event: response.created\ndata: {{\"type\":\"response.created\",\"response\":{{\"id\":\"{}\",\"object\":\"response\",\"model\":\"\",\"status\":\"in_progress\",\"output\":[]}}}}\n\n",
                    resp_id
                );
                events.push(Ok(Bytes::from(created_event)));
                let in_progress_event = format!(
                    "event: response.in_progress\ndata: {{\"type\":\"response.in_progress\",\"response\":{{\"id\":\"{}\",\"object\":\"response\",\"model\":\"\",\"status\":\"in_progress\",\"output\":[]}}}}\n\n",
                    resp_id
                );
                events.push(Ok(Bytes::from(in_progress_event)));
            }

            // Parse SSE lines
            for line in chunk_str.lines() {
                let trimmed = line.trim();

                // Skip comments, empty lines, and the "data:" prefix lines we handle below
                if trimmed.is_empty() || trimmed.starts_with(':') {
                    continue;
                }

                // Parse "data: ..." or "event: ..." lines
                let parsed = if let Some(data) = trimmed.strip_prefix("data: ") {
                    Some(("data", data))
                } else if let Some(data) = trimmed.strip_prefix("data:") {
                    Some(("data", data))
                } else if let Some(event) = trimmed.strip_prefix("event: ") {
                    Some(("event", event))
                } else if let Some(event) = trimmed.strip_prefix("event:") {
                    Some(("event", event))
                } else {
                    None
                };

                match parsed {
                    Some(("data", data_str)) => {
                        if data_str.trim() == "[DONE]" {
                            log::info!("[Codex SSE] Got [DONE], closing state and emitting response.completed");
                            // Close any open state and emit response.completed
                            let mut current_state = state.borrow_mut();
                            match &*current_state {
                                State::Message { output_index, item_id, .. } => {
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.content_part.done\ndata: {{\"type\":\"response.content_part.done\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0}}\n\n",
                                        0, item_id, output_index
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}]}}}}\n\n",
                                        0, output_index, item_id
                                    ))));
                                }
                                State::Reasoning { output_index, item_id, .. } => {
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"reasoning\",\"status\":\"completed\",\"summary\":[]}}}}\n\n",
                                        0, output_index, item_id
                                    ))));
                                }
                                State::Idle => {}
                            }
                            *current_state = State::Idle;
                            // Emit response.completed with model, id, and status.
                            let model = cached_model.lock().unwrap().clone().unwrap_or_default();
                            let completed = format!(
                                "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"{}\",\"object\":\"response\",\"model\":\"{}\",\"status\":\"completed\",\"output\":[]}}}}\n\n",
                                resp_id, model
                            );
                            events.push(Ok(Bytes::from(completed)));
                            continue;
                        }

                        let data: Value = match serde_json::from_str(data_str) {
                            Ok(v) => v,
                            Err(_) => {
                                log::info!("[Codex SSE] Failed to parse data: {}", data_str);
                                continue;
                            }
                        };

                        // Cache model name for response.completed
                        if let Some(m) = data.get("model").and_then(|v| v.as_str()) {
                            if let Ok(mut mc) = cached_model.lock() {
                                if mc.is_none() {
                                    *mc = Some(m.to_string());
                                }
                            }
                        }

                        // Extract delta from choices[0]
                        let delta = data.pointer("/choices/0/delta");
                        let finish_reason = data
                            .pointer("/choices/0/finish_reason")
                            .and_then(|v| v.as_str());

                        let rc = delta
                            .and_then(|d| d.get("reasoning_content"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let content = delta
                            .and_then(|d| d.get("content"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let tool_calls = delta.and_then(|d| d.get("tool_calls"));

                        let mut current_state = state.borrow_mut();

                        // --- Handle finishing state ---
                        if finish_reason.is_some() {
                            // Close open message state
                            match &*current_state {
                                State::Message {
                                    idx,
                                    output_index,
                                    item_id,
                                } => {
                                    // emit content_part.done + output_item.done
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.content_part.done\ndata: {{\"type\":\"response.content_part.done\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0}}\n\n",
                                        0,
                                        item_id,
                                        output_index
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}]}}}}\n\n",
                                        0,
                                        output_index,
                                        item_id
                                    ))));
                                }
                                State::Reasoning { .. } => {
                                    // Just close
                                }
                                State::Idle => {}
                            }
                            *current_state = State::Idle;
                            continue;
                        }

                        // --- Handle reasoning_content ---
                        if !rc.is_empty() {
                            match &*current_state {
                                State::Reasoning { .. } => {
                                    // Continuing reasoning: emit delta
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.reasoning_summary_text.delta\ndata: {{\"type\":\"response.reasoning_summary_text.delta\",\"sequence_number\":{},\"delta\":\"{}\"}}\n\n",
                                        0,
                                        rc.escape_default()
                                    ))));
                                }
                                _ => {
                                    // If message was open, close it first
                                    if let State::Message { .. } = &*current_state {
                                        // TODO: close open message before starting reasoning
                                    }
                                    // Start new reasoning item
                                    let item_id = format!("rs_{}", 0);
                                    let output_index = 0;
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"reasoning\",\"status\":\"in_progress\",\"summary\":[]}}}}\n\n",
                                        0,
                                        output_index,
                                        item_id
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.reasoning_summary_part.added\ndata: {{\"type\":\"response.reasoning_summary_part.added\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"summary_index\":0,\"part\":{{\"type\":\"summary_text\",\"text\":\"\"}}}}\n\n",
                                        0,
                                        item_id,
                                        output_index
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.reasoning_summary_text.delta\ndata: {{\"type\":\"response.reasoning_summary_text.delta\",\"sequence_number\":{},\"delta\":\"{}\"}}\n\n",
                                        0,
                                        rc.escape_default()
                                    ))));
                                    *current_state = State::Reasoning {
                                        item_id,
                                        output_index,
                                    };
                                }
                            }
                            continue;
                        }

                        // --- Handle content (text) ---
                        if !content.is_empty() {
                            match &*current_state {
                                State::Message {
                                    idx: _,
                                    output_index,
                                    item_id,
                                } => {
                                    // Continuing content: emit text delta
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0,\"delta\":\"{}\"}}\n\n",
                                        0,
                                        item_id,
                                        output_index,
                                        content.escape_default()
                                    ))));
                                }
                                State::Reasoning {
                                    item_id,
                                    output_index,
                                } => {
                                    // Close reasoning first, then start message
                                    // Close: reasoning output_item.done
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.done\ndata: {{\"type\":\"response.output_item.done\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"reasoning\",\"status\":\"completed\",\"summary\":[]}}}}\n\n",
                                        0,
                                        output_index,
                                        item_id
                                    ))));
                                    // Start message
                                    let msg_id = format!("msg_{}", 0);
                                    let msg_idx = 0;
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}}}\n\n",
                                        0,
                                        msg_idx,
                                        msg_id
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.content_part.added\ndata: {{\"type\":\"response.content_part.added\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0,\"part\":{{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}}}\n\n",
                                        0,
                                        msg_id,
                                        msg_idx
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0,\"delta\":\"{}\"}}\n\n",
                                        0,
                                        msg_id,
                                        msg_idx,
                                        content.escape_default()
                                    ))));
                                    *current_state = State::Message {
                                        idx: msg_idx as usize,
                                        output_index: msg_idx,
                                        item_id: msg_id.clone(),
                                    };
                                }
                                State::Idle => {
                                    // Start message
                                    let msg_id = format!("msg_{}", 0);
                                    let msg_idx = 0;
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"{}\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}}}\n\n",
                                        0,
                                        msg_idx,
                                        msg_id
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.content_part.added\ndata: {{\"type\":\"response.content_part.added\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0,\"part\":{{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}}}\n\n",
                                        0,
                                        msg_id,
                                        msg_idx
                                    ))));
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0,\"delta\":\"{}\"}}\n\n",
                                        0,
                                        msg_id,
                                        msg_idx,
                                        content.escape_default()
                                    ))));
                                    *current_state = State::Message {
                                        idx: msg_idx as usize,
                                        output_index: msg_idx,
                                        item_id: msg_id.clone(),
                                    };
                                }
                            }
                            continue;
                        }

                        // --- Handle tool_calls ---
                        if let Some(tcs) = tool_calls {
                            if let Some(tc_array) = tcs.as_array() {
                                // Close any open message first
                                if let State::Message {
                                    idx: _,
                                    output_index,
                                    item_id,
                                } = &*current_state
                                {
                                    // Already in message state, tool calls come as content deltas
                                    // in the same message. Close the content part and emit function_calls.
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.content_part.done\ndata: {{\"type\":\"response.content_part.done\",\"sequence_number\":{},\"item_id\":\"{}\",\"output_index\":{},\"content_index\":0}}\n\n",
                                        0,
                                        item_id,
                                        output_index
                                    ))));
                                }

                                for tc in tc_array {
                                    let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                    let fn_name = tc
                                        .pointer("/function/name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let fn_args = tc
                                        .pointer("/function/arguments")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    let fc_output_index = 0;
                                    events.push(Ok(Bytes::from(format!(
                                        "event: response.output_item.added\ndata: {{\"type\":\"response.output_item.added\",\"sequence_number\":{},\"output_index\":{},\"item\":{{\"id\":\"fc_{}\",\"type\":\"function_call\",\"status\":\"in_progress\",\"name\":\"{}\",\"call_id\":\"{}\",\"arguments\":\"\"}}}}\n\n",
                                        0,
                                        fc_output_index,
                                        tc_id,
                                        fn_name,
                                        tc_id
                                    ))));
                                    if !fn_args.is_empty() {
                                        events.push(Ok(Bytes::from(format!(
                                            "event: response.function_call_arguments.delta\ndata: {{\"type\":\"response.function_call_arguments.delta\",\"sequence_number\":{},\"item_id\":\"fc_{}\",\"output_index\":{},\"delta\":\"{}\"}}\n\n",
                                            0,
                                            tc_id,
                                            fc_output_index,
                                            fn_args.escape_default()
                                        ))));
                                        events.push(Ok(Bytes::from(format!(
                                            "event: response.function_call_arguments.done\ndata: {{\"type\":\"response.function_call_arguments.done\",\"sequence_number\":{},\"item_id\":\"fc_{}\",\"output_index\":{}}}]\n\n",
                                            0,
                                            tc_id,
                                            fc_output_index
                                        ))));
                                    }
                                }
                                *current_state = State::Idle;
                            }
                        }
                    }
                    _ => {}
                }
            }

            let event_count = events.len();
            if event_count > 0 {
                log::info!("[Codex SSE] Emitting {} events", event_count);
            }
            stream::iter(events).right_stream()
        });

    Box::pin(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_non_stream_response() {
        let input = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "created": 1712345678,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let result = chat_to_responses(&input, None).unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["type"], "message");
        assert_eq!(result["output"][0]["content"][0]["text"], "Hello!");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_non_stream_with_reasoning_content() {
        let input = json!({
            "id": "chatcmpl-def456",
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Final answer",
                    "reasoning_content": "Let me think..."
                },
                "finish_reason": "stop"
            }]
        });

        let result = chat_to_responses(&input, None).unwrap();
        let output = result["output"].as_array().unwrap();

        // First item: reasoning
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(output[0]["summary"][0]["text"], "Let me think...");

        // Second item: message with reasoning_text + output_text
        assert_eq!(output[1]["type"], "message");
        assert_eq!(output[1]["content"][0]["type"], "reasoning_text");
        assert_eq!(output[1]["content"][0]["text"], "Let me think...");
        assert_eq!(output[1]["content"][1]["type"], "output_text");
        assert_eq!(output[1]["content"][1]["text"], "Final answer");
    }

    #[test]
    fn test_non_stream_with_tool_calls() {
        let input = json!({
            "id": "chatcmpl-ghi789",
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"NYC\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_to_responses(&input, None).unwrap();
        let output = result["output"].as_array().unwrap();

        // First: message item (empty content)
        assert_eq!(output[0]["type"], "message");

        // Second: function_call item
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["name"], "get_weather");
        assert_eq!(output[1]["call_id"], "call_1");
    }
}
