//! Conservative repair for structurally complete Anthropic tool streams.
//!
//! Some compatible gateways close with a clean EOF after a complete
//! `input_json_delta`, but before the terminal Anthropic lifecycle events. This
//! adapter appends the missing tail only when the open tool input is provably
//! complete. Malformed, oversized, errored, and transport-failed streams remain
//! unchanged.

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

const MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;
const MAX_SSE_DELIMITER_BYTES: usize = 4;
const MAX_TRACKED_TOOL_INPUT_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn repair_anthropic_sse_tail<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut tracker = TailTracker::default();
        let mut stream_failed = false;
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    tracker.push(&bytes);
                    yield Ok(bytes);
                }
                Err(error) => {
                    stream_failed = true;
                    yield Err(std::io::Error::other(error.to_string()));
                    break;
                }
            }
        }

        if !stream_failed {
            if let Some(events) = tracker.finish() {
                log::warn!(
                    "[Claude] Repaired missing terminal events in a structurally complete Anthropic SSE stream"
                );
                for event in events {
                    yield Ok(event);
                }
            }
        }
    }
}

#[derive(Debug)]
enum OpenBlock {
    ToolUse { index: u64, partial_json: String },
    Other { index: u64 },
}

impl OpenBlock {
    fn index(&self) -> u64 {
        match self {
            Self::ToolUse { index, .. } | Self::Other { index } => *index,
        }
    }
}

#[derive(Default)]
struct TailTracker {
    buffer: String,
    utf8_remainder: Vec<u8>,
    saw_message_start: bool,
    saw_message_stop: bool,
    saw_error: bool,
    saw_message_delta: bool,
    // Anthropic content block lifecycles are serial: start, deltas, stop.
    open_block: Option<OpenBlock>,
    tracked_tool_input_bytes: usize,
    invalid: bool,
}

impl TailTracker {
    fn push(&mut self, mut bytes: &[u8]) {
        if self.invalid {
            return;
        }

        let max_buffer_bytes = MAX_SSE_EVENT_BYTES + MAX_SSE_DELIMITER_BYTES;
        while !bytes.is_empty() {
            let buffered_bytes = self.buffer.len().saturating_add(self.utf8_remainder.len());
            let Some(available_bytes) = max_buffer_bytes.checked_sub(buffered_bytes) else {
                self.invalidate_and_clear_buffers();
                return;
            };
            if available_bytes == 0 {
                self.invalidate_and_clear_buffers();
                return;
            }

            let consumed_bytes = bytes.len().min(available_bytes);
            let (next, remaining) = bytes.split_at(consumed_bytes);
            append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, next);
            bytes = remaining;

            while let Some(block) = take_sse_block(&mut self.buffer) {
                self.observe_sse_block(&block);
                if self.invalid {
                    self.invalidate_and_clear_buffers();
                    return;
                }
            }

            if self.buffer.len().saturating_add(self.utf8_remainder.len()) > max_buffer_bytes {
                self.invalidate_and_clear_buffers();
                return;
            }
        }
    }

    fn finish(mut self) -> Option<Vec<Bytes>> {
        if self.invalid || !self.utf8_remainder.is_empty() {
            return None;
        }
        let mut separator = None;
        if !self.buffer.trim().is_empty() {
            if self.buffer.len() > MAX_SSE_EVENT_BYTES {
                return None;
            }
            separator = Some(if self.buffer.ends_with("\r\n") {
                Bytes::from_static(b"\r\n")
            } else if self.buffer.ends_with('\n') {
                Bytes::from_static(b"\n")
            } else {
                Bytes::from_static(b"\n\n")
            });
            let trailing = std::mem::take(&mut self.buffer);
            self.observe_sse_block(&trailing);
        }
        let mut events = self.missing_tail_events()?;
        if let Some(separator) = separator {
            events.insert(0, separator);
        }
        Some(events)
    }

    fn observe_sse_block(&mut self, block: &str) {
        if block.len() > MAX_SSE_EVENT_BYTES {
            self.invalidate();
            return;
        }

        let mut event_name = None;
        let mut data_lines = Vec::new();
        for line in block.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                event_name = Some(event.trim());
            } else if let Some(data) = strip_sse_field(line, "data") {
                data_lines.push(data);
            }
        }
        if data_lines.is_empty() {
            return;
        }

        let Ok(payload) = serde_json::from_str::<Value>(&data_lines.join("\n")) else {
            self.invalidate();
            return;
        };
        if !payload.is_object() {
            self.invalidate();
            return;
        }
        let payload_type = payload.get("type").and_then(Value::as_str);
        if event_name
            .zip(payload_type)
            .is_some_and(|(event_name, payload_type)| event_name != payload_type)
        {
            self.invalidate();
            return;
        }
        let Some(event_type) = payload_type.or(event_name) else {
            self.invalidate();
            return;
        };
        self.observe_event(event_type, &payload);
    }

    fn observe_event(&mut self, event_type: &str, payload: &Value) {
        match event_type {
            "message_start"
                if !self.saw_message_start
                    && payload.get("message").is_some_and(Value::is_object) =>
            {
                self.saw_message_start = true
            }
            "message_start" => self.invalidate(),
            "content_block_start" => self.start_block(payload),
            "content_block_delta" => self.update_block(payload),
            "content_block_stop" => self.stop_block(payload),
            "message_delta" if self.open_block.is_none() => self.saw_message_delta = true,
            "message_delta" => self.invalidate(),
            "message_stop" => self.saw_message_stop = true,
            "error" => self.saw_error = true,
            _ => {}
        }
    }

    fn start_block(&mut self, payload: &Value) {
        let Some(index) = payload.get("index").and_then(Value::as_u64) else {
            self.invalidate();
            return;
        };
        let Some(content_block) = payload.get("content_block").and_then(Value::as_object) else {
            self.invalidate();
            return;
        };
        let Some(block_type) = content_block.get("type").and_then(Value::as_str) else {
            self.invalidate();
            return;
        };
        if !self.saw_message_start
            || self.saw_message_stop
            || self.saw_message_delta
            || self.open_block.is_some()
        {
            self.invalidate();
            return;
        }

        self.open_block = Some(if block_type == "tool_use" {
            let valid_tool_start = content_block
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
                && content_block
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty())
                && content_block.get("input").is_some_and(Value::is_object);
            if !valid_tool_start {
                self.invalidate();
                return;
            }
            OpenBlock::ToolUse {
                index,
                partial_json: String::new(),
            }
        } else {
            OpenBlock::Other { index }
        });
    }

    fn update_block(&mut self, payload: &Value) {
        let Some(index) = payload.get("index").and_then(Value::as_u64) else {
            self.invalidate();
            return;
        };
        let Some(delta) = payload.get("delta").and_then(Value::as_object) else {
            self.invalidate();
            return;
        };
        let Some(delta_type) = delta.get("type").and_then(Value::as_str) else {
            self.invalidate();
            return;
        };

        match self.open_block.as_mut() {
            Some(OpenBlock::ToolUse {
                index: block_index,
                partial_json,
            }) if *block_index == index && delta_type == "input_json_delta" => {
                let Some(fragment) = delta.get("partial_json").and_then(Value::as_str) else {
                    self.invalidate();
                    return;
                };
                let Some(total) = self.tracked_tool_input_bytes.checked_add(fragment.len()) else {
                    self.invalidate();
                    return;
                };
                if total > MAX_TRACKED_TOOL_INPUT_BYTES {
                    self.invalidate();
                    return;
                }
                self.tracked_tool_input_bytes = total;
                partial_json.push_str(fragment);
            }
            Some(OpenBlock::Other { index: block_index })
                if *block_index == index && delta_type != "input_json_delta" => {}
            _ => self.invalidate(),
        }
    }

    fn stop_block(&mut self, payload: &Value) {
        let index = payload.get("index").and_then(Value::as_u64);
        if index.is_some() && self.open_block.as_ref().map(OpenBlock::index) == index {
            self.open_block = None;
        } else {
            self.invalidate();
        }
    }

    fn missing_tail_events(&self) -> Option<Vec<Bytes>> {
        if self.invalid
            || !self.saw_message_start
            || self.saw_message_stop
            || self.saw_message_delta
            || self.saw_error
        {
            return None;
        }

        let Some(OpenBlock::ToolUse {
            index,
            partial_json,
        }) = &self.open_block
        else {
            return None;
        };
        if partial_json.is_empty()
            || !serde_json::from_str::<Value>(partial_json).is_ok_and(|value| value.is_object())
        {
            return None;
        }

        encode_events([
            (
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            ),
            (
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use", "stop_sequence": null},
                    // Terminal usage never arrived, so use the same zero fallback
                    // as CC Switch's existing Anthropic stream converters.
                    "usage": {"output_tokens": 0}
                }),
            ),
            ("message_stop", json!({"type": "message_stop"})),
        ])
    }

    fn invalidate(&mut self) {
        self.invalid = true;
    }

    fn invalidate_and_clear_buffers(&mut self) {
        self.invalidate();
        self.buffer.clear();
        self.utf8_remainder.clear();
    }
}

fn encode_events(events: impl IntoIterator<Item = (&'static str, Value)>) -> Option<Vec<Bytes>> {
    events
        .into_iter()
        .map(|(event_name, payload)| {
            serde_json::to_string(&payload)
                .ok()
                .map(|data| Bytes::from(format!("event: {event_name}\ndata: {data}\n\n")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn event(name: &'static str, payload: Value) -> String {
        format!(
            "event: {name}\ndata: {}\n\n",
            serde_json::to_string(&payload).expect("test payload should serialize")
        )
    }

    fn message_start() -> String {
        event(
            "message_start",
            json!({"type": "message_start", "message": {"content": [], "stop_reason": null}}),
        )
    }

    fn tool_start() -> String {
        event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_test", "name": "ping", "input": {}}
            }),
        )
    }

    fn tool_delta(partial_json: &str) -> String {
        event(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": partial_json}
            }),
        )
    }

    fn tool_stream(partial_json: &str) -> String {
        format!(
            "{}{}{}",
            message_start(),
            tool_start(),
            tool_delta(partial_json)
        )
    }

    async fn run(chunks: Vec<Result<Bytes, std::io::Error>>) -> Vec<Result<Bytes, std::io::Error>> {
        repair_anthropic_sse_tail(stream::iter(chunks))
            .collect()
            .await
    }

    async fn output(input: &str) -> String {
        let mut output = Vec::new();
        for chunk in run(vec![Ok(Bytes::copy_from_slice(input.as_bytes()))]).await {
            output.extend_from_slice(&chunk.expect("test stream should succeed"));
        }
        String::from_utf8(output).expect("test stream should contain valid UTF-8")
    }

    #[tokio::test]
    async fn repairs_clean_eof_after_complete_tool_input() {
        let input = tool_stream("{\"value\":\"ok\"}");
        let repaired = output(&input).await;

        assert!(repaired.starts_with(&input));
        assert!(repaired.contains("event: content_block_stop\n"));
        assert!(repaired.contains("\"stop_reason\":\"tool_use\""));
        assert!(repaired.ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"));
    }

    #[tokio::test]
    async fn repairs_split_utf8_and_unterminated_final_event() {
        let input = tool_stream("{\"city\":\"杭州\"}").trim_end().to_string();
        let split = input
            .as_bytes()
            .windows("杭".len())
            .position(|window| window == "杭".as_bytes())
            .expect("test input contains the multibyte character")
            + 1;
        let chunks = vec![
            Ok(Bytes::copy_from_slice(&input.as_bytes()[..split])),
            Ok(Bytes::copy_from_slice(&input.as_bytes()[split..])),
        ];
        let mut repaired = Vec::new();
        for chunk in run(chunks).await {
            repaired.extend_from_slice(&chunk.expect("test stream should succeed"));
        }

        let repaired = String::from_utf8(repaired).expect("output should remain valid UTF-8");
        assert!(repaired.starts_with(&format!("{input}\n\nevent: content_block_stop\n")));
        assert!(repaired.contains("event: message_stop\n"));
    }

    #[tokio::test]
    async fn repairs_large_transport_chunk_when_each_sse_event_is_bounded() {
        let ping = event(
            "ping",
            json!({"type": "ping", "padding": "x".repeat(MAX_SSE_EVENT_BYTES / 2)}),
        );
        let input = format!(
            "{}{}{}{}{}",
            message_start(),
            ping,
            ping,
            tool_start(),
            tool_delta("{}")
        );
        assert!(ping.len() < MAX_SSE_EVENT_BYTES);
        assert!(input.len() > MAX_SSE_EVENT_BYTES);

        let repaired = output(&input).await;

        assert!(repaired.starts_with(&input));
        assert!(repaired.ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"));
    }

    #[tokio::test]
    async fn mismatched_event_and_payload_types_remain_unchanged() {
        let input = format!(
            "{}event: error\ndata: {{\"type\":\"ping\"}}\n\n",
            tool_stream("{}")
        );

        assert_eq!(output(&input).await, input);
    }

    #[tokio::test]
    async fn leaves_complete_stream_byte_for_byte_unchanged() {
        let input = format!(
            "{}{}{}{}",
            tool_stream("{}"),
            event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": 0})
            ),
            event(
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use", "stop_sequence": null},
                    "usage": {"output_tokens": 7}
                })
            ),
            event("message_stop", json!({"type": "message_stop"}))
        );

        assert_eq!(output(&input).await, input);
    }

    #[tokio::test]
    async fn unsafe_streams_remain_unchanged() {
        let cases = [
            tool_stream("{\"value\":"),
            format!("{}{}", message_start(), tool_start()),
            format!(
                "{}{}",
                message_start(),
                event(
                    "message_delta",
                    json!({
                        "type": "message_delta",
                        "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                        "usage": {"output_tokens": 3}
                    })
                )
            ),
            format!(
                "{}{}{}",
                message_start(),
                event(
                    "content_block_start",
                    json!({
                        "type": "content_block_start", "index": 0,
                        "content_block": {"type": "tool_use", "name": "ping", "input": {}}
                    })
                ),
                tool_delta("{}")
            ),
            format!(
                "{}{}{}",
                message_start(),
                event(
                    "content_block_start",
                    json!({
                        "type": "content_block_start", "index": 0,
                        "content_block": {"type": "text", "text": ""}
                    })
                ),
                event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta", "index": 0,
                        "delta": {"type": "text_delta", "text": "partial"}
                    })
                )
            ),
            format!(
                "{}{}",
                tool_stream("{}"),
                event(
                    "error",
                    json!({"type": "error", "error": {"type": "api_error"}})
                )
            ),
            format!(
                "{}event: ping\ndata: not-json\n\n{}{}",
                message_start(),
                tool_start(),
                tool_delta("{}")
            ),
        ];

        for input in cases {
            assert_eq!(output(&input).await, input);
        }
    }

    #[tokio::test]
    async fn oversized_event_remains_unchanged() {
        let input = format!(
            "{}event: ping\ndata: {{\"type\":\"ping\",\"padding\":\"{}\"}}\n\n{}{}",
            message_start(),
            "x".repeat(MAX_SSE_EVENT_BYTES + 1),
            tool_start(),
            tool_delta("{}")
        );

        assert_eq!(output(&input).await, input);
    }

    #[tokio::test]
    async fn transport_error_never_gets_a_synthetic_tail() {
        let prefix = tool_stream("{}");
        let output = run(vec![
            Ok(Bytes::from(prefix.clone())),
            Err(std::io::Error::other("connection reset")),
        ])
        .await;

        assert_eq!(output.len(), 2);
        assert_eq!(
            output[0].as_ref().expect("first chunk should pass"),
            &prefix
        );
        assert!(output[1].is_err());
    }
}
