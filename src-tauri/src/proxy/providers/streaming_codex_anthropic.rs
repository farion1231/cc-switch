//! Anthropic Messages SSE → OpenAI format SSE converters
//!
//! Two converters:
//! - `create_openai_chat_sse_stream_from_anthropic`: Anthropic SSE → OpenAI Chat Completions SSE
//! - `create_responses_sse_stream_from_anthropic`: Anthropic SSE → OpenAI Responses API SSE
//!
//! Used when Codex CLI (which expects OpenAI format) talks to an Anthropic upstream.

use super::transform_anthropic_to_codex::{
    anthropic_usage_to_openai_chat, anthropic_usage_to_responses,
};
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

// ============================================================================
// Anthropic SSE → OpenAI Chat Completions SSE
// ============================================================================

/// Convert an Anthropic Messages SSE stream into an OpenAI Chat Completions
/// SSE stream. This is the reverse of `create_anthropic_sse_stream()` in
/// `streaming.rs`.
pub fn create_openai_chat_sse_stream_from_anthropic<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut remainder: Vec<u8> = Vec::new();

        // State
        let mut message_id = String::new();
        let mut model = String::new();
        let mut started = false;
        let mut _current_block_type: Option<String> = None; // "text" | "thinking" | "tool_use"
        let mut _current_tool_index: usize = 0;
        let mut tool_started: Vec<bool> = Vec::new();
        let mut finish_reason: Option<String> = None;
        let mut final_usage: Option<Value> = None;

        tokio::pin!(stream);

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        let (event_type, data_str) = parse_anthropic_sse_block(&block);
                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        match event_type.as_deref() {
                            Some("message_start") => {
                                if let Some(msg) = data.get("message") {
                                    message_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("chatcmpl-unknown").to_string();
                                    model = msg.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    if let Some(usage) = msg.get("usage") {
                                        final_usage = Some(usage.clone());
                                    }
                                }
                            }
                            Some("content_block_start") => {
                                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                if let Some(cb) = data.get("content_block") {
                                    let cb_type = cb.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    _current_block_type = Some(cb_type.to_string());

                                    match cb_type {
                                        "text" => {
                                            // Will emit content deltas
                                        }
                                        "thinking" => {
                                            // Will emit reasoning_content deltas
                                        }
                                        "tool_use" => {
                                            let id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                            let name = cb.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                            while tool_started.len() <= index {
                                                tool_started.push(false);
                                            }
                                            _current_tool_index = index;

                                            // Emit tool_call start chunk
                                            let chunk = json!({
                                                "id": format!("chatcmpl-{message_id}"),
                                                "object": "chat.completion.chunk",
                                                "created": chrono_now(),
                                                "model": model,
                                                "choices": [{
                                                    "index": 0,
                                                    "delta": {
                                                        "tool_calls": [{
                                                            "index": index,
                                                            "id": id,
                                                            "type": "function",
                                                            "function": {"name": name, "arguments": ""}
                                                        }]
                                                    },
                                                    "finish_reason": null
                                                }]
                                            });
                                            if !started {
                                                started = true;
                                            }
                                            tool_started[index] = true;
                                            yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("content_block_delta") => {
                                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                if let Some(delta) = data.get("delta") {
                                    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");

                                    match delta_type {
                                        "text_delta" => {
                                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                                if !text.is_empty() {
                                                    if !started {
                                                        started = true;
                                                    }
                                                    let chunk = json!({
                                                        "id": format!("chatcmpl-{message_id}"),
                                                        "object": "chat.completion.chunk",
                                                        "created": chrono_now(),
                                                        "model": model,
                                                        "choices": [{
                                                            "index": 0,
                                                            "delta": {"content": text},
                                                            "finish_reason": null
                                                        }]
                                                    });
                                                    yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                                                }
                                            }
                                        }
                                        "thinking_delta" => {
                                            if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                                if !thinking.is_empty() {
                                                    if !started {
                                                        started = true;
                                                    }
                                                    let chunk = json!({
                                                        "id": format!("chatcmpl-{message_id}"),
                                                        "object": "chat.completion.chunk",
                                                        "created": chrono_now(),
                                                        "model": model,
                                                        "choices": [{
                                                            "index": 0,
                                                            "delta": {"reasoning_content": thinking},
                                                            "finish_reason": null
                                                        }]
                                                    });
                                                    yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                                                }
                                            }
                                        }
                                        "input_json_delta" => {
                                            if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                                if !partial.is_empty() {
                                                    let chunk = json!({
                                                        "id": format!("chatcmpl-{message_id}"),
                                                        "object": "chat.completion.chunk",
                                                        "created": chrono_now(),
                                                        "model": model,
                                                        "choices": [{
                                                            "index": 0,
                                                            "delta": {
                                                                "tool_calls": [{
                                                                    "index": index,
                                                                    "function": {"arguments": partial}
                                                                }]
                                                            },
                                                            "finish_reason": null
                                                        }]
                                                    });
                                                    yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("message_delta") => {
                                if let Some(delta) = data.get("delta") {
                                    if let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                        finish_reason = Some(map_anthropic_stop_reason(sr).to_string());
                                    }
                                }
                                if let Some(usage) = data.get("usage") {
                                    final_usage = Some(usage.clone());
                                }
                            }
                            Some("message_stop") => {
                                // Emit final chunk with finish_reason
                                let fr = finish_reason.as_deref().unwrap_or("stop");
                                let mut chunk = json!({
                                    "id": format!("chatcmpl-{message_id}"),
                                    "object": "chat.completion.chunk",
                                    "created": chrono_now(),
                                    "model": model,
                                    "choices": [{
                                        "index": 0,
                                        "delta": {},
                                        "finish_reason": fr
                                    }]
                                });
                                if let Some(ref usage) = final_usage {
                                    chunk["usage"] = anthropic_usage_to_openai_chat(usage);
                                }
                                yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                                yield Ok(Bytes::from("data: [DONE]\n\n"));
                            }
                            Some("error") => {
                                let error_msg = data.get("error")
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("upstream error");
                                let chunk = json!({
                                    "id": format!("chatcmpl-{message_id}"),
                                    "object": "chat.completion.chunk",
                                    "created": chrono_now(),
                                    "model": model,
                                    "choices": [{
                                        "index": 0,
                                        "delta": {},
                                        "finish_reason": null
                                    }],
                                    "error": {"message": error_msg, "type": "upstream_error"}
                                });
                                yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    let chunk = json!({
                        "id": format!("chatcmpl-{message_id}"),
                        "object": "chat.completion.chunk",
                        "created": chrono_now(),
                        "model": model,
                        "choices": [{"index": 0, "delta": {}, "finish_reason": null}],
                        "error": {"message": format!("Stream error: {e}"), "type": "stream_error"}
                    });
                    yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
                    break;
                }
            }
        }

        // Safety: if stream ended without message_stop
        if finish_reason.is_some() {
            // Already emitted [DONE]
        } else {
            let chunk = json!({
                "id": format!("chatcmpl-{message_id}"),
                "object": "chat.completion.chunk",
                "created": chrono_now(),
                "model": model,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            });
            yield Ok(Bytes::from(format!("data: {chunk}\n\n")));
            yield Ok(Bytes::from("data: [DONE]\n\n"));
        }
    }
}

// ============================================================================
// Anthropic SSE → OpenAI Responses API SSE
// ============================================================================

/// Convert an Anthropic Messages SSE stream into an OpenAI Responses API SSE
/// stream. This is the reverse direction of `create_anthropic_sse_stream_from_responses()`.
pub fn create_responses_sse_stream_from_anthropic<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut remainder: Vec<u8> = Vec::new();

        // State
        let mut response_id = String::new();
        let mut model = String::new();
        let mut _response_created = false;
        let mut next_output_index: u32 = 0;
        let mut text_item_id = String::new();
        let mut text_item_added = false;
        let mut text_output_index: u32 = 0;
        let mut _current_tool_index: usize = 0;
        let mut tool_items: Vec<(String, String, u32, String)> = Vec::new(); // (item_id, name, output_index, call_id)
        let mut thinking_item_added = false;
        let mut text_cb_index: usize = 0; // Anthropic content block index for text
        let mut thinking_cb_index: usize = 0; // Anthropic content block index for thinking
        let mut thinking_item_id = String::new();
        let mut thinking_output_index: u32 = 0;
        let mut finish_reason: Option<String> = None;
        let mut final_usage: Option<Value> = None;

        tokio::pin!(stream);

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        let (event_type, data_str) = parse_anthropic_sse_block(&block);
                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        match event_type.as_deref() {
                            Some("message_start") => {
                                if let Some(msg) = data.get("message") {
                                    response_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("resp-unknown").to_string();
                                    model = msg.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    if let Some(usage) = msg.get("usage") {
                                        final_usage = Some(usage.clone());
                                    }

                                    // Emit response.created
                                    let created = json!({
                                        "type": "response.created",
                                        "response": {
                                            "id": response_id,
                                            "object": "response",
                                            "status": "in_progress",
                                            "model": model,
                                            "output": [],
                                            "usage": null
                                        }
                                    });
                                    yield Ok(format_sse_event("response.created", &created));
                                    _response_created = true;

                                    // Emit response.in_progress
                                    let in_progress = json!({
                                        "type": "response.in_progress",
                                        "response": {
                                            "id": response_id,
                                            "object": "response",
                                            "status": "in_progress",
                                            "model": model,
                                            "output": [],
                                            "usage": null
                                        }
                                    });
                                    yield Ok(format_sse_event("response.in_progress", &in_progress));
                                }
                            }
                            Some("content_block_start") => {
                                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                if let Some(cb) = data.get("content_block") {
                                    let cb_type = cb.get("type").and_then(|v| v.as_str()).unwrap_or("");

                                    match cb_type {
                                        "text" => {
                                            let oi = next_output_index;
                                            next_output_index += 1;
                                            text_item_id = format!("msg_{response_id}_{oi}");
                                            text_item_added = true;
                                            text_output_index = oi;
                                            text_cb_index = index;

                                            // Emit output_item.added
                                            let item_added = json!({
                                                "type": "response.output_item.added",
                                                "output_index": oi,
                                                "item": {
                                                    "id": text_item_id,
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "content": []
                                                }
                                            });
                                            yield Ok(format_sse_event("response.output_item.added", &item_added));

                                            // Emit content_part.added
                                            let part_added = json!({
                                                "type": "response.content_part.added",
                                                "output_index": oi,
                                                "content_index": 0,
                                                "part": {"type": "output_text", "text": "", "annotations": []}
                                            });
                                            yield Ok(format_sse_event("response.content_part.added", &part_added));
                                        }
                                        "thinking" => {
                                            let oi = next_output_index;
                                            next_output_index += 1;
                                            thinking_item_id = format!("rs_{response_id}_{oi}");
                                            thinking_item_added = true;
                                            thinking_output_index = oi;
                                            thinking_cb_index = index;

                                            // Emit output_item.added for reasoning
                                            let item_added = json!({
                                                "type": "response.output_item.added",
                                                "output_index": oi,
                                                "item": {
                                                    "id": thinking_item_id,
                                                    "type": "reasoning",
                                                    "summary": []
                                                }
                                            });
                                            yield Ok(format_sse_event("response.output_item.added", &item_added));
                                        }
                                        "tool_use" => {
                                            let id = cb.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                            let name = cb.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                            let oi = next_output_index;
                                            next_output_index += 1;
                                            let item_id = format!("fc_{response_id}_{oi}");

                                            while tool_items.len() <= index {
                                                tool_items.push((String::new(), String::new(), 0, String::new()));
                                            }
                                            tool_items[index] = (item_id.clone(), name.to_string(), oi, id.to_string());

                                            // Emit output_item.added for function_call
                                            let item_added = json!({
                                                "type": "response.output_item.added",
                                                "output_index": oi,
                                                "item": {
                                                    "id": item_id,
                                                    "type": "function_call",
                                                    "call_id": id,
                                                    "name": name,
                                                    "arguments": "",
                                                    "status": "in_progress"
                                                }
                                            });
                                            yield Ok(format_sse_event("response.output_item.added", &item_added));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("content_block_delta") => {
                                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                                if let Some(delta) = data.get("delta") {
                                    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");

                                    match delta_type {
                                        "text_delta" => {
                                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                                if !text.is_empty() && text_item_added {
                                                    let event = json!({
                                                        "type": "response.output_text.delta",
                                                        "output_index": text_output_index,
                                                        "content_index": 0,
                                                        "delta": text
                                                    });
                                                    yield Ok(format_sse_event("response.output_text.delta", &event));
                                                }
                                            }
                                        }
                                        "thinking_delta" => {
                                            if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                                if !thinking.is_empty() && thinking_item_added {
                                                    let event = json!({
                                                        "type": "response.reasoning.delta",
                                                        "output_index": thinking_output_index,
                                                        "delta": {"type": "summary_text", "text": thinking}
                                                    });
                                                    yield Ok(format_sse_event("response.reasoning.delta", &event));
                                                }
                                            }
                                        }
                                        "input_json_delta" => {
                                            if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                                if !partial.is_empty() {
                                                    let oi = if index < tool_items.len() { tool_items[index].2 } else { index as u32 };
                                                    let event = json!({
                                                        "type": "response.function_call_arguments.delta",
                                                        "output_index": oi,
                                                        "delta": partial
                                                    });
                                                    yield Ok(format_sse_event("response.function_call_arguments.delta", &event));
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("content_block_stop") => {
                                let index = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                                // Determine what type of block ended using stored content block indices
                                if text_item_added && index == text_cb_index {
                                    // Text block: emit content_part.done + output_item.done
                                    let part_done = json!({
                                        "type": "response.content_part.done",
                                        "output_index": text_output_index,
                                        "content_index": 0,
                                        "part": {"type": "output_text", "text": "", "annotations": []}
                                    });
                                    yield Ok(format_sse_event("response.content_part.done", &part_done));

                                    let item_done = json!({
                                        "type": "response.output_item.done",
                                        "output_index": text_output_index,
                                        "item": {
                                            "id": text_item_id,
                                            "type": "message",
                                            "role": "assistant",
                                            "content": [{"type": "output_text", "text": "", "annotations": []}]
                                        }
                                    });
                                    yield Ok(format_sse_event("response.output_item.done", &item_done));
                                    text_item_added = false;
                                } else if thinking_item_added && index == thinking_cb_index {
                                    let item_done = json!({
                                        "type": "response.output_item.done",
                                        "output_index": thinking_output_index,
                                        "item": {
                                            "id": thinking_item_id,
                                            "type": "reasoning",
                                            "summary": []
                                        }
                                    });
                                    yield Ok(format_sse_event("response.output_item.done", &item_done));
                                    thinking_item_added = false;
                                } else if index < tool_items.len() {
                                    // Tool block done
                                    let (ref item_id, ref _name, oi, ref call_id) = tool_items[index];
                                    let args_done = json!({
                                        "type": "response.function_call_arguments.done",
                                        "output_index": oi,
                                        "arguments": ""
                                    });
                                    yield Ok(format_sse_event("response.function_call_arguments.done", &args_done));

                                    let item_done = json!({
                                        "type": "response.output_item.done",
                                        "output_index": oi,
                                        "item": {
                                            "id": item_id,
                                            "type": "function_call",
                                            "call_id": call_id,
                                            "name": _name,
                                            "arguments": "",
                                            "status": "completed"
                                        }
                                    });
                                    yield Ok(format_sse_event("response.output_item.done", &item_done));
                                }
                            }
                            Some("message_delta") => {
                                if let Some(delta) = data.get("delta") {
                                    if let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                        finish_reason = Some(sr.to_string());
                                    }
                                }
                                if let Some(usage) = data.get("usage") {
                                    final_usage = Some(usage.clone());
                                }
                            }
                            Some("message_stop") => {
                                let status = match finish_reason.as_deref() {
                                    Some("max_tokens") => "incomplete",
                                    _ => "completed",
                                };
                                let mut response_obj = json!({
                                    "id": response_id,
                                    "object": "response",
                                    "status": status,
                                    "model": model,
                                    "output": [],
                                    "usage": final_usage.as_ref().map(anthropic_usage_to_responses).unwrap_or(json!(null))
                                });
                                if finish_reason.as_deref() == Some("max_tokens") {
                                    response_obj["incomplete_details"] = json!({"reason": "max_output_tokens"});
                                }

                                let completed = json!({
                                    "type": "response.completed",
                                    "response": response_obj
                                });
                                yield Ok(format_sse_event("response.completed", &completed));
                            }
                            Some("error") => {
                                let error_msg = data.get("error")
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("upstream error");
                                let error_event = json!({
                                    "type": "error",
                                    "code": "upstream_error",
                                    "message": error_msg
                                });
                                yield Ok(format_sse_event("error", &error_event));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    let error_event = json!({
                        "type": "error",
                        "code": "stream_error",
                        "message": format!("Stream error: {e}")
                    });
                    yield Ok(format_sse_event("error", &error_event));
                    break;
                }
            }
        }

        // Safety: if stream ended without message_stop, emit response.completed
        if finish_reason.is_none() {
            let status = "completed";
            let mut response_obj = json!({
                "id": response_id,
                "object": "response",
                "status": status,
                "model": model,
                "output": [],
                "usage": final_usage.as_ref().map(anthropic_usage_to_responses).unwrap_or(json!(null))
            });
            let completed = json!({
                "type": "response.completed",
                "response": response_obj
            });
            yield Ok(format_sse_event("response.completed", &completed));
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_anthropic_sse_block(block: &str) -> (Option<String>, String) {
    let mut event_type: Option<String> = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in block.lines() {
        if let Some(et) = strip_sse_field(line, "event") {
            event_type = Some(et.to_string());
        } else if let Some(d) = strip_sse_field(line, "data") {
            data_lines.push(d);
        }
    }

    let data_str = data_lines.join("\n");
    (event_type, data_str)
}

fn format_sse_event(event_name: &str, data: &Value) -> Bytes {
    let data_str = serde_json::to_string(data).unwrap_or_default();
    Bytes::from(format!("event: {event_name}\ndata: {data_str}\n\n"))
}

fn map_anthropic_stop_reason(reason: &str) -> &str {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "stop_sequence" => "stop",
        _ => "stop",
    }
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
