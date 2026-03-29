//! Anthropic SSE → OpenAI Responses API SSE 流式转换模块
//!
//! Anthropic 生命周期:
//! message_start → content_block_start → content_block_delta →
//! content_block_stop → message_delta → message_stop
//!
//! Responses API 生命周期 (data includes "type" field, response.created/completed wrap in "response" key):
//! response.created → output_item.added → content_part.added →
//! output_text.delta → content_part.done → output_item.done → response.completed

use crate::proxy::sse::strip_sse_field;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

/// Emit an SSE event with the standard Responses API data format.
/// All events include a `type` field. `response.created` and `response.completed`
/// wrap the payload in a `response` key.
fn emit_sse(event_name: &str, mut data: Value) -> String {
    data["type"] = json!(event_name);
    format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    )
}

/// Emit an SSE event where the payload is wrapped under `"response"` key.
fn emit_sse_with_response(event_name: &str, response_data: Value) -> String {
    let data = json!({
        "type": event_name,
        "response": response_data
    });
    format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    )
}

/// Infer tool name by matching input keys against tool definitions.
/// Falls back to first tool name or "unknown".
fn infer_tool_name(input: &Value, tools: &[Value]) -> String {
    // If only one tool, use it
    if tools.len() == 1 {
        return tools[0]
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown")
            .to_string();
    }
    // Match input keys against tool input_schema.properties
    if let Some(input_obj) = input.as_object() {
        let input_keys: std::collections::HashSet<&str> =
            input_obj.keys().map(|k| k.as_str()).collect();
        for tool in tools {
            if let Some(props) = tool
                .pointer("/input_schema/properties")
                .and_then(|p| p.as_object())
            {
                let tool_keys: std::collections::HashSet<&str> =
                    props.keys().map(|k| k.as_str()).collect();
                if !input_keys.is_empty() && input_keys.is_subset(&tool_keys) {
                    return tool
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                }
            }
        }
    }
    // Fallback to first tool
    tools
        .first()
        .and_then(|t| t.get("name").and_then(|n| n.as_str()))
        .unwrap_or("unknown")
        .to_string()
}

pub fn create_responses_sse_stream_from_anthropic(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    tools: Vec<Value>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut response_id = String::new();
        let mut current_model = String::new();
        let mut output_index: usize = 0;
        let mut current_block_type = String::new();
        let mut accumulated_tool_args = String::new();
        let mut final_usage: Value = json!(null);
        let mut final_stop_reason: Option<String> = None;
        let mut accumulated_output: Vec<Value> = Vec::new();
        let mut accumulated_text = String::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    while let Some(pos) = buffer.find("\n\n") {
                        let block = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if block.trim().is_empty() {
                            continue;
                        }

                        let mut event_type: Option<String> = None;
                        let mut data_str = String::new();

                        for line in block.lines() {
                            if let Some(evt) = strip_sse_field(line, "event") {
                                event_type = Some(evt.trim().to_string());
                            } else if let Some(d) = strip_sse_field(line, "data") {
                                if !data_str.is_empty() {
                                    data_str.push('\n');
                                }
                                data_str.push_str(d);
                            }
                        }

                        if data_str.is_empty() {
                            continue;
                        }

                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        let event_name = event_type
                            .as_deref()
                            .or_else(|| data.get("type").and_then(|t| t.as_str()))
                            .unwrap_or("");

                        match event_name {
                            "message_start" => {
                                if let Some(msg) = data.get("message") {
                                    response_id = msg.get("id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| format!("resp_{}", s.trim_start_matches("msg_")))
                                        .unwrap_or_default();
                                    current_model = msg.get("model")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                }
                                output_index = 0;

                                yield Ok(Bytes::from(emit_sse_with_response("response.created", json!({
                                    "id": &response_id,
                                    "object": "response",
                                    "status": "in_progress",
                                    "model": &current_model,
                                    "output": [],
                                    "usage": null
                                }))));
                            }

                            "content_block_start" => {
                                let empty = json!({});
                                let block = data.get("content_block").unwrap_or(&empty);
                                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                current_block_type = block_type.to_string();

                                match block_type {
                                    "text" => {
                                        accumulated_text.clear();
                                        let item_id = format!("msg_{:032x}", output_index);
                                        yield Ok(Bytes::from(emit_sse("response.output_item.added", json!({
                                            "output_index": output_index,
                                            "item": {
                                                "type": "message",
                                                "id": &item_id,
                                                "role": "assistant",
                                                "status": "in_progress",
                                                "content": []
                                            }
                                        }))));
                                        yield Ok(Bytes::from(emit_sse("response.content_part.added", json!({
                                            "output_index": output_index,
                                            "content_index": 0,
                                            "part": {"type": "output_text", "text": "", "annotations": []}
                                        }))));
                                    }
                                    "tool_use" => {
                                        accumulated_tool_args.clear();
                                        // Extract id or generate one (some gateways omit id)
                                        current_tool_id = block.get("id")
                                            .and_then(|v| v.as_str())
                                            .filter(|s| !s.is_empty())
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| format!("call_{:032x}", output_index as u128 + 1));
                                        // Extract name or infer from tools list (some gateways omit name)
                                        current_tool_name = block.get("name")
                                            .and_then(|v| v.as_str())
                                            .filter(|s| !s.is_empty())
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| {
                                                // Try to infer from input keys matching tool schemas
                                                let empty_input = json!({});
                                                let input = block.get("input").unwrap_or(&empty_input);
                                                infer_tool_name(input, &tools)
                                            });
                                        // Some gateways put complete input in content_block_start
                                        if let Some(input) = block.get("input") {
                                            if !input.is_null() && input != &json!({}) {
                                                accumulated_tool_args = serde_json::to_string(input)
                                                    .unwrap_or_else(|_| "{}".to_string());
                                            }
                                        }
                                        yield Ok(Bytes::from(emit_sse("response.output_item.added", json!({
                                            "output_index": output_index,
                                            "item": {
                                                "type": "function_call",
                                                "id": &current_tool_id,
                                                "call_id": &current_tool_id,
                                                "name": &current_tool_name,
                                                "arguments": "",
                                                "status": "in_progress"
                                            }
                                        }))));
                                    }
                                    "thinking" => {
                                        accumulated_text.clear();
                                    }
                                    _ => {}
                                }
                            }

                            "content_block_delta" => {
                                let empty_delta = json!({});
                                let delta = data.get("delta").unwrap_or(&empty_delta);
                                let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                match delta_type {
                                    "text_delta" => {
                                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                            accumulated_text.push_str(text);
                                            yield Ok(Bytes::from(emit_sse("response.output_text.delta", json!({
                                                "output_index": output_index,
                                                "content_index": 0,
                                                "delta": text
                                            }))));
                                        }
                                    }
                                    "input_json_delta" => {
                                        if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                            accumulated_tool_args.push_str(partial);
                                            yield Ok(Bytes::from(emit_sse("response.function_call_arguments.delta", json!({
                                                "output_index": output_index,
                                                "delta": partial
                                            }))));
                                        }
                                    }
                                    "thinking_delta" => {
                                        if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                            accumulated_text.push_str(thinking);
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            "content_block_stop" => {
                                match current_block_type.as_str() {
                                    "text" => {
                                        yield Ok(Bytes::from(emit_sse("response.content_part.done", json!({
                                            "output_index": output_index,
                                            "content_index": 0,
                                            "part": {"type": "output_text", "text": &accumulated_text, "annotations": []}
                                        }))));

                                        let item_id = format!("msg_{:032x}", output_index);
                                        yield Ok(Bytes::from(emit_sse("response.output_item.done", json!({
                                            "output_index": output_index,
                                            "item": {
                                                "type": "message",
                                                "id": &item_id,
                                                "role": "assistant",
                                                "status": "completed",
                                                "content": [{"type": "output_text", "text": &accumulated_text, "annotations": []}]
                                            }
                                        }))));

                                        accumulated_output.push(json!({
                                            "type": "message", "id": &item_id, "role": "assistant", "status": "completed",
                                            "content": [{"type": "output_text", "text": &accumulated_text, "annotations": []}]
                                        }));
                                    }
                                    "tool_use" => {
                                        yield Ok(Bytes::from(emit_sse("response.function_call_arguments.done", json!({
                                            "output_index": output_index,
                                            "arguments": &accumulated_tool_args
                                        }))));

                                        yield Ok(Bytes::from(emit_sse("response.output_item.done", json!({
                                            "output_index": output_index,
                                            "item": {
                                                "type": "function_call",
                                                "id": &current_tool_id,
                                                "call_id": &current_tool_id,
                                                "name": &current_tool_name,
                                                "arguments": &accumulated_tool_args,
                                                "status": "completed"
                                            }
                                        }))));

                                        accumulated_output.push(json!({
                                            "type": "function_call", "id": &current_tool_id, "call_id": &current_tool_id,
                                            "name": &current_tool_name, "arguments": &accumulated_tool_args, "status": "completed"
                                        }));
                                    }
                                    "thinking" => {
                                        // Skip thinking blocks in SSE output — Codex CLI
                                        // doesn't handle reasoning items and misinterprets
                                        // them as empty function_calls in follow-up requests
                                    }
                                    _ => {}
                                }
                                // Only increment output_index for non-thinking blocks
                                if current_block_type != "thinking" {
                                    output_index += 1;
                                }
                                current_block_type.clear();
                            }

                            "message_delta" => {
                                if let Some(delta) = data.get("delta") {
                                    if let Some(sr) = delta.get("stop_reason").and_then(|s| s.as_str()) {
                                        final_stop_reason = Some(sr.to_string());
                                    }
                                }
                                if let Some(usage) = data.get("usage") {
                                    final_usage = usage.clone();
                                }
                            }

                            "message_stop" => {
                                let status = match final_stop_reason.as_deref() {
                                    Some("max_tokens") => "incomplete",
                                    _ => "completed",
                                };
                                let usage = if final_usage.is_null() {
                                    json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0})
                                } else {
                                    let input = final_usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                    let output = final_usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                    json!({"input_tokens": input, "output_tokens": output, "total_tokens": input + output})
                                };
                                let mut response_obj = json!({
                                    "id": &response_id, "object": "response", "model": &current_model,
                                    "status": status, "output": &accumulated_output, "usage": usage,
                                    "metadata": {}, "temperature": 1.0, "top_p": null,
                                    "max_output_tokens": null, "previous_response_id": null,
                                    "reasoning": {}, "text": {}, "truncation": null,
                                    "instructions": null, "tool_choice": "auto", "tools": [],
                                    "parallel_tool_calls": false
                                });
                                if status == "incomplete" {
                                    response_obj["incomplete_details"] = json!({"reason": "max_output_tokens"});
                                }
                                yield Ok(Bytes::from(emit_sse_with_response("response.completed", response_obj)));
                            }

                            "error" => {
                                let sse = format!(
                                    "event: error\ndata: {}\n\n",
                                    serde_json::to_string(&json!({"type": "error", "error": data})).unwrap_or_default()
                                );
                                log::error!("[Codex/Anthropic] Upstream SSE error: {data}");
                                yield Ok(Bytes::from(sse));
                            }

                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    log::error!("[Codex/Anthropic] Stream error: {e}");
                    yield Err(std::io::Error::other(e.to_string()));
                }
            }
        }
    }
}
