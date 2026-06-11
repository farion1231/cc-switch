//! Kiro Streaming Response Adapter Module
//!
//! Converts Kiro event stream into Anthropic Messages SSE stream.

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

const EVENT_PATTERNS: &[&str] = &[
    "{\"content\":",
    "{\"name\":",
    "{\"input\":",
    "{\"stop\":",
    "{\"contextUsagePercentage\":",
    "{\"followupPrompt\":",
    "{\"usage\":",
    "{\"toolUseId\":",
    "{\"unit\":",
    "{\"error\":",
    "{\"Error\":",
    "{\"message\":",
];

pub enum KiroStreamEvent {
    Content(String),
    ToolUse {
        name: String,
        tool_use_id: String,
        input: String,
        stop: bool,
    },
    ToolUseInput(String),
    ToolUseStop(bool),
    ContextUsage(f64),
    Usage {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
    Error {
        error: String,
        message: Option<String>,
    },
}

fn find_json_end_bytes(text: &str, start_byte: usize) -> Option<usize> {
    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let bytes = text.as_bytes();
    for i in start_byte..bytes.len() {
        let b = bytes[i];
        if escape_next {
            escape_next = false;
            continue;
        }
        if b == b'\\' {
            escape_next = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if !in_string {
            if b == b'{' {
                brace_count += 1;
            } else if b == b'}' {
                brace_count -= 1;
                if brace_count == 0 {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn find_next_event_start(buffer: &str, from: usize) -> Option<usize> {
    let mut earliest = None;
    for pattern in EVENT_PATTERNS {
        if let Some(idx) = buffer[from..].find(pattern) {
            let abs_idx = from + idx;
            if earliest.is_none() || abs_idx < earliest.unwrap() {
                earliest = Some(abs_idx);
            }
        }
    }
    earliest
}

fn parse_kiro_event(parsed: &Value) -> Option<KiroStreamEvent> {
    if let Some(content) = parsed.get("content").and_then(|v| v.as_str()) {
        return Some(KiroStreamEvent::Content(content.to_string()));
    }
    if let (Some(name), Some(tool_use_id)) = (
        parsed.get("name").and_then(|v| v.as_str()),
        parsed.get("toolUseId").and_then(|v| v.as_str()),
    ) {
        let input = parsed.get("input").map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        }).unwrap_or_default();
        let stop = parsed.get("stop").and_then(|v| v.as_bool()).unwrap_or(false);
        return Some(KiroStreamEvent::ToolUse {
            name: name.to_string(),
            tool_use_id: tool_use_id.to_string(),
            input,
            stop,
        });
    }
    if let Some(input) = parsed.get("input") {
        if parsed.get("name").is_none() {
            let input_str = if let Some(s) = input.as_str() {
                s.to_string()
            } else {
                input.to_string()
            };
            return Some(KiroStreamEvent::ToolUseInput(input_str));
        }
    }
    if let Some(stop) = parsed.get("stop").and_then(|v| v.as_bool()) {
        if parsed.get("contextUsagePercentage").is_none() {
            return Some(KiroStreamEvent::ToolUseStop(stop));
        }
    }
    if let Some(pct) = parsed.get("contextUsagePercentage").and_then(|v| v.as_f64()) {
        return Some(KiroStreamEvent::ContextUsage(pct));
    }
    if let Some(error_val) = parsed.get("error").or_else(|| parsed.get("Error")) {
        let error_str = if let Some(s) = error_val.as_str() {
            s.to_string()
        } else {
            error_val.to_string()
        };
        let message = parsed.get("message")
            .or_else(|| parsed.get("Message"))
            .or_else(|| parsed.get("reason"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        return Some(KiroStreamEvent::Error { error: error_str, message });
    }
    if let Some(usage) = parsed.get("usage") {
        if parsed.get("unit").is_none() {
            let input_tokens = usage.get("inputTokens").and_then(|v| v.as_u64()).map(|v| v as u32);
            let output_tokens = usage.get("outputTokens").and_then(|v| v.as_u64()).map(|v| v as u32);
            return Some(KiroStreamEvent::Usage { input_tokens, output_tokens });
        }
    }
    None
}

fn parse_kiro_events(buffer: &str) -> (Vec<KiroStreamEvent>, String) {
    let mut events = Vec::new();
    let mut pos = 0;

    while pos < buffer.len() {
        let json_start = match find_next_event_start(buffer, pos) {
            Some(idx) => idx,
            None => break,
        };

        let json_end = match find_json_end_bytes(buffer, json_start) {
            Some(idx) => idx,
            None => {
                // Incomplete JSON at end of buffer
                return (events, buffer[json_start..].to_string());
            }
        };

        if let Ok(parsed) = serde_json::from_str::<Value>(&buffer[json_start..=json_end]) {
            if let Some(event) = parse_kiro_event(&parsed) {
                events.push(event);
            }
        }
        pos = json_end + 1;
    }

    (events, String::new())
}

/// Create Anthropic SSE Stream from Kiro Response Stream
pub fn create_anthropic_sse_stream_from_kiro<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();

        let mut has_sent_message_start = false;
        let mut current_block_index: Option<u32> = None;
        let mut current_block_type: Option<&'static str> = None; // "text" or "tool_use"
        let mut next_content_index: u32 = 0;

        let mut current_tool_id: Option<String> = None;
        let mut latest_usage: Option<Value> = None;
        let mut has_tool_calls = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    let (events, remaining) = parse_kiro_events(&buffer);
                    buffer = remaining;

                    for event in events {
                        match event {
                            KiroStreamEvent::Content(text) => {
                                if !has_sent_message_start {
                                    let msg_start = json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": format!("msg_kiro{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
                                            "type": "message",
                                            "role": "assistant",
                                            "content": [],
                                            "model": "claude-sonnet",
                                            "stop_reason": null,
                                            "stop_sequence": null,
                                            "usage": {
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            }
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&msg_start).unwrap())));
                                    has_sent_message_start = true;
                                }

                                if current_block_type != Some("text") {
                                    if current_block_index.is_some() {
                                        let block_stop = json!({
                                            "type": "content_block_stop",
                                            "index": current_block_index.unwrap()
                                        });
                                        yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap())));
                                    }

                                    let block_start = json!({
                                        "type": "content_block_start",
                                        "index": next_content_index,
                                        "content_block": {
                                            "type": "text",
                                            "text": ""
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&block_start).unwrap())));

                                    current_block_index = Some(next_content_index);
                                    current_block_type = Some("text");
                                    next_content_index += 1;
                                }

                                let block_delta = json!({
                                    "type": "content_block_delta",
                                    "index": current_block_index.unwrap(),
                                    "delta": {
                                        "type": "text_delta",
                                        "text": text
                                    }
                                });
                                yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&block_delta).unwrap())));
                            }
                            KiroStreamEvent::ToolUse { name, tool_use_id, input, stop } => {
                                has_tool_calls = true;
                                if !has_sent_message_start {
                                    let msg_start = json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": format!("msg_kiro{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
                                            "type": "message",
                                            "role": "assistant",
                                            "content": [],
                                            "model": "claude-sonnet",
                                            "stop_reason": null,
                                            "stop_sequence": null,
                                            "usage": {
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            }
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&msg_start).unwrap())));
                                    has_sent_message_start = true;
                                }

                                if current_block_type != Some("tool_use") || current_tool_id.as_deref() != Some(&tool_use_id) {
                                    if current_block_index.is_some() {
                                        let block_stop = json!({
                                            "type": "content_block_stop",
                                            "index": current_block_index.unwrap()
                                        });
                                        yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap())));
                                    }

                                    let block_start = json!({
                                        "type": "content_block_start",
                                        "index": next_content_index,
                                        "content_block": {
                                            "type": "tool_use",
                                            "id": tool_use_id,
                                            "name": name,
                                            "input": {}
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&block_start).unwrap())));

                                    current_block_index = Some(next_content_index);
                                    current_block_type = Some("tool_use");
                                    current_tool_id = Some(tool_use_id.clone());
                                    next_content_index += 1;
                                }

                                if !input.is_empty() {
                                    let block_delta = json!({
                                        "type": "content_block_delta",
                                        "index": current_block_index.unwrap(),
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": input
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&block_delta).unwrap())));
                                }

                                if stop {
                                    let block_stop = json!({
                                        "type": "content_block_stop",
                                        "index": current_block_index.unwrap()
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap())));
                                    current_block_index = None;
                                    current_block_type = None;
                                    current_tool_id = None;
                                }
                            }
                            KiroStreamEvent::ToolUseInput(input) => {
                                if current_block_type == Some("tool_use") && current_block_index.is_some() {
                                    let block_delta = json!({
                                        "type": "content_block_delta",
                                        "index": current_block_index.unwrap(),
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": input
                                        }
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&block_delta).unwrap())));
                                }
                            }
                            KiroStreamEvent::ToolUseStop(stop) => {
                                if stop && current_block_type == Some("tool_use") && current_block_index.is_some() {
                                    let block_stop = json!({
                                        "type": "content_block_stop",
                                        "index": current_block_index.unwrap()
                                    });
                                    yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap())));
                                    current_block_index = None;
                                    current_block_type = None;
                                    current_tool_id = None;
                                }
                            }
                            KiroStreamEvent::Usage { input_tokens, output_tokens } => {
                                latest_usage = Some(json!({
                                    "input_tokens": input_tokens.unwrap_or(0),
                                    "output_tokens": output_tokens.unwrap_or(0)
                                }));
                            }
                            KiroStreamEvent::Error { error, message } => {
                                let err_json = json!({
                                    "type": "error",
                                    "error": {
                                        "type": "api_error",
                                        "message": message.unwrap_or(error)
                                    }
                                });
                                yield Ok(Bytes::from(format!("event: error\ndata: {}\n\n", serde_json::to_string(&err_json).unwrap())));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                }
            }
        }

        // Close any remaining open blocks
        if current_block_index.is_some() {
            let block_stop = json!({
                "type": "content_block_stop",
                "index": current_block_index.unwrap()
            });
            yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&block_stop).unwrap())));
        }

        // Send message delta with final usage
        let usage = latest_usage.unwrap_or_else(|| json!({"input_tokens": 0, "output_tokens": 0}));
        let stop_reason = if has_tool_calls { "tool_use" } else { "end_turn" };
        let msg_delta = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": usage
        });
        yield Ok(Bytes::from(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&msg_delta).unwrap())));

        // Send message stop
        let msg_stop = json!({
            "type": "message_stop"
        });
        yield Ok(Bytes::from(format!("event: message_stop\ndata: {}\n\n", serde_json::to_string(&msg_stop).unwrap())));
    }
}
