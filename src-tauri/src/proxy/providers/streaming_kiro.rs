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
    /// 上下文使用百分比（当前仅解析，暂未透传给下游）
    #[allow(dead_code)]
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
    for (i, &b) in bytes.iter().enumerate().skip(start_byte) {
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
        let input = parsed
            .get("input")
            .map(|v| {
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                }
            })
            .unwrap_or_default();
        let stop = parsed
            .get("stop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
    if let Some(pct) = parsed
        .get("contextUsagePercentage")
        .and_then(|v| v.as_f64())
    {
        return Some(KiroStreamEvent::ContextUsage(pct));
    }
    if let Some(error_val) = parsed.get("error").or_else(|| parsed.get("Error")) {
        let error_str = if let Some(s) = error_val.as_str() {
            s.to_string()
        } else {
            error_val.to_string()
        };
        let message = parsed
            .get("message")
            .or_else(|| parsed.get("Message"))
            .or_else(|| parsed.get("reason"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        return Some(KiroStreamEvent::Error {
            error: error_str,
            message,
        });
    }
    if let Some(usage) = parsed.get("usage") {
        if parsed.get("unit").is_none() {
            let input_tokens = usage
                .get("inputTokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let output_tokens = usage
                .get("outputTokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            return Some(KiroStreamEvent::Usage {
                input_tokens,
                output_tokens,
            });
        }
    }
    None
}

fn parse_kiro_events(buffer: &str) -> (Vec<KiroStreamEvent>, String) {
    let mut events = Vec::new();
    let mut pos = 0;
    // 最长的 event pattern 是 {"contextUsagePercentage": (25 bytes)
    // 保留尾部这么多字节,避免跨 chunk 的部分 pattern 丢失
    const MAX_PATTERN_LEN: usize = 25;

    while pos < buffer.len() {
        let json_start = match find_next_event_start(buffer, pos) {
            Some(idx) => idx,
            None => {
                // 从当前 pos 起找不到任何 pattern,可能是:
                // 1) buffer[pos..] 全是非 event 数据(空白/乱码) -> 安全丢弃
                // 2) buffer[pos..] 是部分 pattern(如 {"con) -> 必须保留尾部
                // 策略:保留最后 MAX_PATTERN_LEN-1 字节,足够容纳任何部分 pattern
                let keep_from = buffer.len().saturating_sub(MAX_PATTERN_LEN - 1);
                return (events, buffer[keep_from..].to_string());
            }
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
                                    let data = serde_json::to_string(&msg_start).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: message_start\ndata: {}\n\n", data)));
                                    has_sent_message_start = true;
                                }

                                if current_block_type != Some("text") {
                                    if let Some(idx) = current_block_index {
                                        let block_stop = json!({
                                            "type": "content_block_stop",
                                            "index": idx
                                        });
                                        let data = serde_json::to_string(&block_stop).map_err(std::io::Error::other)?;
                                        yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", data)));
                                    }

                                    let block_start = json!({
                                        "type": "content_block_start",
                                        "index": next_content_index,
                                        "content_block": {
                                            "type": "text",
                                            "text": ""
                                        }
                                    });
                                    let data = serde_json::to_string(&block_start).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: content_block_start\ndata: {}\n\n", data)));

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
                                let data = serde_json::to_string(&block_delta).map_err(std::io::Error::other)?;
                                yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", data)));
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
                                    let data = serde_json::to_string(&msg_start).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: message_start\ndata: {}\n\n", data)));
                                    has_sent_message_start = true;
                                }

                                if current_block_type != Some("tool_use") || current_tool_id.as_deref() != Some(&tool_use_id) {
                                    if let Some(idx) = current_block_index {
                                        let block_stop = json!({
                                            "type": "content_block_stop",
                                            "index": idx
                                        });
                                        let data = serde_json::to_string(&block_stop).map_err(std::io::Error::other)?;
                                        yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", data)));
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
                                    let data = serde_json::to_string(&block_start).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: content_block_start\ndata: {}\n\n", data)));

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
                                    let data = serde_json::to_string(&block_delta).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", data)));
                                }

                                if stop {
                                    if let Some(idx) = current_block_index {
                                        let block_stop = json!({
                                            "type": "content_block_stop",
                                            "index": idx
                                        });
                                        let data = serde_json::to_string(&block_stop).map_err(std::io::Error::other)?;
                                        yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", data)));
                                    }
                                    current_block_index = None;
                                    current_block_type = None;
                                    current_tool_id = None;
                                }
                            }
                            KiroStreamEvent::ToolUseInput(input) if current_block_type == Some("tool_use") => {
                                if let Some(idx) = current_block_index {
                                    let block_delta = json!({
                                        "type": "content_block_delta",
                                        "index": idx,
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": input
                                        }
                                    });
                                    let data = serde_json::to_string(&block_delta).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: content_block_delta\ndata: {}\n\n", data)));
                                }
                            }
                            KiroStreamEvent::ToolUseStop(stop) if stop && current_block_type == Some("tool_use") => {
                                if let Some(idx) = current_block_index {
                                    let block_stop = json!({
                                        "type": "content_block_stop",
                                        "index": idx
                                    });
                                    let data = serde_json::to_string(&block_stop).map_err(std::io::Error::other)?;
                                    yield Ok(Bytes::from(format!("event: content_block_stop\ndata: {}\n\n", data)));
                                }
                                current_block_index = None;
                                current_block_type = None;
                                current_tool_id = None;
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
                    yield Err(std::io::Error::other(e.to_string()));
                }
            }
        }

        // Close any remaining open blocks
        if let Some(idx) = current_block_index {
            let block_stop = json!({
                "type": "content_block_stop",
                "index": idx
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

/// 非流式聚合：把 Kiro 完整 eventstream 响应体解析并聚合成单个 Anthropic
/// Messages JSON（type:"message"）。
///
/// 用于客户端发起 stream:false 请求时（如 Claude Desktop 网关探活），Kiro 仍以
/// application/vnd.amazon.eventstream 返回多段事件，需在此聚合成一条完整消息。
/// 返回 `Err(message)` 表示 eventstream 中出现了 application 级错误事件
/// （KiroStreamEvent::Error，如配额/模型/鉴权错误）。此时不应伪造成功消息，
/// 调用方应据此向客户端返回错误响应。
pub fn kiro_eventstream_to_anthropic_response(body: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(body);
    let (events, _remaining) = parse_kiro_events(&text);

    // content blocks 按出现顺序聚合（保持文本/工具交错）
    let mut content_blocks: Vec<Value> = Vec::new();
    let mut current_text = String::new();
    let mut has_open_text = false;

    // 当前工具调用：(id, name, accumulated input json string)
    let mut current_tool: Option<(String, String, String)> = None;

    let mut has_tool_calls = false;
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;

    fn flush_text(
        current_text: &mut String,
        has_open_text: &mut bool,
        content_blocks: &mut Vec<Value>,
    ) {
        if *has_open_text {
            content_blocks.push(json!({
                "type": "text",
                "text": std::mem::take(current_text),
            }));
            *has_open_text = false;
        }
    }

    fn flush_tool(
        current_tool: &mut Option<(String, String, String)>,
        content_blocks: &mut Vec<Value>,
    ) {
        if let Some((id, name, input_json)) = current_tool.take() {
            let input_value: Value = if input_json.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&input_json).unwrap_or_else(|_| json!({}))
            };
            content_blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input_value,
            }));
        }
    }

    for event in events {
        match event {
            KiroStreamEvent::Content(t) => {
                // 文本出现表示上一个工具块（若有）已结束
                flush_tool(&mut current_tool, &mut content_blocks);
                current_text.push_str(&t);
                has_open_text = true;
            }
            KiroStreamEvent::ToolUse {
                name,
                tool_use_id,
                input,
                stop,
            } => {
                has_tool_calls = true;
                // 文本块在工具块前定格
                flush_text(&mut current_text, &mut has_open_text, &mut content_blocks);

                // 切换工具：先定格旧工具
                let switching = match &current_tool {
                    Some((id, _, _)) => id != &tool_use_id,
                    None => true,
                };
                if switching {
                    flush_tool(&mut current_tool, &mut content_blocks);
                    current_tool = Some((tool_use_id.clone(), name.clone(), String::new()));
                }
                if let Some((_, _, buf)) = current_tool.as_mut() {
                    if !input.is_empty() {
                        buf.push_str(&input);
                    }
                }
                if stop {
                    flush_tool(&mut current_tool, &mut content_blocks);
                }
            }
            KiroStreamEvent::ToolUseInput(input) => {
                if let Some((_, _, buf)) = current_tool.as_mut() {
                    buf.push_str(&input);
                }
            }
            KiroStreamEvent::ToolUseStop(stop) if stop => {
                flush_tool(&mut current_tool, &mut content_blocks);
            }
            KiroStreamEvent::Usage {
                input_tokens: it,
                output_tokens: ot,
            } => {
                if let Some(v) = it {
                    input_tokens = v;
                }
                if let Some(v) = ot {
                    output_tokens = v;
                }
            }
            KiroStreamEvent::Error { error, message } => {
                // 出现 application 级错误事件：放弃聚合，向上抛错，避免伪造成功消息。
                return Err(message.unwrap_or(error));
            }
            _ => {}
        }
    }

    // 收尾
    flush_text(&mut current_text, &mut has_open_text, &mut content_blocks);
    flush_tool(&mut current_tool, &mut content_blocks);

    let stop_reason = if has_tool_calls {
        "tool_use"
    } else {
        "end_turn"
    };

    Ok(json!({
        "id": format!("msg_kiro{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": "claude-sonnet",
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_text_only_response() {
        // Kiro eventstream: 多段 content 事件拼接（无 SSE 框架，裸 JSON 对象流）
        let body = br#"{"content":"Hello"}{"content":", world"}{"usage":{"inputTokens":12,"outputTokens":3}}"#;
        let resp = kiro_eventstream_to_anthropic_response(body).unwrap();
        assert_eq!(resp["type"], "message");
        assert_eq!(resp["role"], "assistant");
        assert_eq!(resp["stop_reason"], "end_turn");
        let content = resp["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Hello, world");
        assert_eq!(resp["usage"]["input_tokens"], 12);
        assert_eq!(resp["usage"]["output_tokens"], 3);
    }

    #[test]
    fn aggregate_tool_use_response() {
        let body = br#"{"content":"Let me check"}{"name":"get_weather","toolUseId":"tool_1","input":""}{"input":"{\"city\":"}{"input":"\"SF\"}"}{"stop":true}"#;
        let resp = kiro_eventstream_to_anthropic_response(body).unwrap();
        assert_eq!(resp["stop_reason"], "tool_use");
        let content = resp["content"].as_array().unwrap();
        // 文本块在前，工具块在后
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Let me check");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["id"], "tool_1");
        assert_eq!(content[1]["name"], "get_weather");
        assert_eq!(content[1]["input"]["city"], "SF");
    }

    #[test]
    fn aggregate_empty_response_is_valid_message() {
        let resp = kiro_eventstream_to_anthropic_response(b"").unwrap();
        assert_eq!(resp["type"], "message");
        assert_eq!(resp["content"].as_array().unwrap().len(), 0);
        assert_eq!(resp["stop_reason"], "end_turn");
    }

    #[test]
    fn aggregate_error_event_returns_err() {
        // eventstream 中出现 application 级错误事件：不得伪造成功消息
        let body =
            br#"{"content":"partial"}{"error":"ThrottlingException","message":"quota exceeded"}"#;
        let resp = kiro_eventstream_to_anthropic_response(body);
        assert!(resp.is_err());
        assert_eq!(resp.unwrap_err(), "quota exceeded");
    }

    #[test]
    fn aggregate_error_event_without_message_uses_error_field() {
        let body = br#"{"Error":"InternalServerException"}"#;
        let resp = kiro_eventstream_to_anthropic_response(body);
        assert!(resp.is_err());
        assert_eq!(resp.unwrap_err(), "InternalServerException");
    }

    #[test]
    fn parse_preserves_partial_pattern_across_chunks() {
        // 模拟跨 chunk 的部分 pattern: {"con 应该被保留到 remainder
        let chunk1 = r#"{"content":"hi"}{"con"#;
        let (events1, remainder1) = parse_kiro_events(chunk1);
        assert_eq!(events1.len(), 1);
        assert!(matches!(&events1[0], KiroStreamEvent::Content(t) if t == "hi"));
        // 关键:remainder 必须包含 {"con,不能是空字符串
        assert!(
            remainder1.contains("{\"con"),
            "remainder 应保留部分 pattern,实际: {remainder1:?}"
        );

        // 模拟第二个 chunk 到达后,拼接到 remainder 应该能解析出完整事件
        let chunk2 = r#"tent":"world"}{"usage":{"inputTokens":1,"outputTokens":2}}"#;
        let combined = format!("{}{}", remainder1, chunk2);
        let (events2, _remainder2) = parse_kiro_events(&combined);
        assert!(events2
            .iter()
            .any(|e| matches!(e, KiroStreamEvent::Content(t) if t == "world")));
    }
}
