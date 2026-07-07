//! Anthropic Messages SSE → OpenAI Responses SSE conversion.
//!
//! The opposite direction of `streaming_responses.rs` (Responses SSE → Anthropic SSE):
//! here the Codex client speaks Responses, while the upstream gateway speaks the native
//! Anthropic Messages protocol. The Responses events emitted here have the same shape as
//! those in `streaming_codex_chat.rs` (Chat → Responses); the Codex client only recognizes
//! this set of events.

use super::transform_codex_anthropic::{
    build_responses_usage_from_anthropic, map_anthropic_stop_reason_to_status,
};
use super::transform_responses::sanitize_anthropic_tool_use_input_json;
use crate::proxy::json_canonical::canonicalize_tool_arguments_str;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Tool,
    Thinking,
}

#[derive(Debug)]
struct BlockState {
    kind: BlockKind,
    output_index: u32,
    item_id: String,
    call_id: String,
    name: String,
    accum: String,
    done: bool,
}

struct AnthropicToResponsesState {
    response_started: bool,
    completed: bool,
    response_id: String,
    model: String,
    next_output_index: u32,
    blocks: BTreeMap<u64, BlockState>,
    output_items: Vec<(u32, Value)>,
    anthropic_usage: Map<String, Value>,
    stop_reason: Option<String>,
}

impl Default for AnthropicToResponsesState {
    fn default() -> Self {
        Self {
            response_started: false,
            completed: false,
            response_id: "resp_ccswitch".to_string(),
            model: String::new(),
            next_output_index: 0,
            blocks: BTreeMap::new(),
            output_items: Vec::new(),
            anthropic_usage: Map::new(),
            stop_reason: None,
        }
    }
}

impl AnthropicToResponsesState {
    fn next_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    fn responses_usage(&self) -> Value {
        if self.anthropic_usage.is_empty() {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0,
                "total_tokens": 0,
                "output_tokens_details": { "reasoning_tokens": 0 }
            });
        }
        build_responses_usage_from_anthropic(Some(&Value::Object(self.anthropic_usage.clone())))
    }

    fn base_response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": 0,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": self.responses_usage()
        })
    }

    fn merge_usage(&mut self, usage: &Value) {
        if let Some(obj) = usage.as_object() {
            for (key, value) in obj {
                if value.is_null() {
                    continue;
                }
                self.anthropic_usage.insert(key.clone(), value.clone());
            }
        }
    }

    fn ensure_response_started(&mut self) -> Vec<Bytes> {
        if self.response_started {
            return Vec::new();
        }
        self.response_started = true;
        vec![
            sse_event(
                "response.created",
                json!({
                    "type": "response.created",
                    "response": self.base_response("in_progress", Vec::new())
                }),
            ),
            sse_event(
                "response.in_progress",
                json!({
                    "type": "response.in_progress",
                    "response": self.base_response("in_progress", Vec::new())
                }),
            ),
        ]
    }

    fn handle_message_start(&mut self, data: &Value) -> Vec<Bytes> {
        if let Some(message) = data.get("message") {
            if let Some(id) = message.get("id").and_then(|v| v.as_str()) {
                self.response_id = if id.starts_with("resp_") {
                    id.to_string()
                } else {
                    format!("resp_{id}")
                };
            }
            if let Some(model) = message.get("model").and_then(|v| v.as_str()) {
                if !model.is_empty() {
                    self.model = model.to_string();
                }
            }
            if let Some(usage) = message.get("usage") {
                self.merge_usage(usage);
            }
        }
        self.ensure_response_started()
    }

    fn handle_content_block_start(&mut self, data: &Value) -> Vec<Bytes> {
        let mut events = self.ensure_response_started();
        let Some(index) = data.get("index").and_then(|v| v.as_u64()) else {
            return events;
        };
        let block = data.get("content_block").unwrap_or(&Value::Null);
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match block_type {
            "text" => {
                let output_index = self.next_output_index();
                let item_id = format!("{}_msg_{output_index}", self.response_id);
                events.push(sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": output_index,
                        "item": {
                            "id": item_id,
                            "type": "message",
                            "status": "in_progress",
                            "role": "assistant",
                            "content": []
                        }
                    }),
                ));
                events.push(sse_event(
                    "response.content_part.added",
                    json!({
                        "type": "response.content_part.added",
                        "item_id": item_id,
                        "output_index": output_index,
                        "content_index": 0,
                        "part": { "type": "output_text", "text": "", "annotations": [] }
                    }),
                ));
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Text,
                        output_index,
                        item_id,
                        call_id: String::new(),
                        name: String::new(),
                        accum: String::new(),
                        done: false,
                    },
                );
            }
            "tool_use" => {
                let output_index = self.next_output_index();
                let call_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let item_id = format!("fc_{call_id}");
                events.push(sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": output_index,
                        "item": {
                            "id": item_id,
                            "type": "function_call",
                            "status": "in_progress",
                            "call_id": call_id,
                            "name": name,
                            "arguments": ""
                        }
                    }),
                ));
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Tool,
                        output_index,
                        item_id,
                        call_id: call_id.to_string(),
                        name: name.to_string(),
                        accum: String::new(),
                        done: false,
                    },
                );
            }
            "thinking" | "redacted_thinking" => {
                let output_index = self.next_output_index();
                let item_id = format!("rs_{}_{output_index}", self.response_id);
                events.push(sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": output_index,
                        "item": {
                            "id": item_id,
                            "type": "reasoning",
                            "status": "in_progress",
                            "summary": []
                        }
                    }),
                ));
                events.push(sse_event(
                    "response.reasoning_summary_part.added",
                    json!({
                        "type": "response.reasoning_summary_part.added",
                        "item_id": item_id,
                        "output_index": output_index,
                        "summary_index": 0,
                        "part": { "type": "summary_text", "text": "" }
                    }),
                ));
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Thinking,
                        output_index,
                        item_id,
                        call_id: String::new(),
                        name: String::new(),
                        accum: String::new(),
                        done: false,
                    },
                );
            }
            _ => {}
        }

        events
    }

    fn handle_content_block_delta(&mut self, data: &Value) -> Vec<Bytes> {
        let Some(index) = data.get("index").and_then(|v| v.as_u64()) else {
            return Vec::new();
        };
        let delta = data.get("delta").unwrap_or(&Value::Null);
        let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");

        let Some(block) = self.blocks.get_mut(&index) else {
            return Vec::new();
        };
        let output_index = block.output_index;
        let item_id = block.item_id.clone();

        match delta_type {
            "text_delta" => {
                let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or("");
                block.accum.push_str(text);
                vec![sse_event(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "content_index": 0,
                        "delta": text
                    }),
                )]
            }
            "input_json_delta" => {
                let partial = delta
                    .get("partial_json")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                block.accum.push_str(partial);
                // The Read tool needs to be sanitized at close time, to avoid emitting pages:"" deltas mid-stream
                if block.name == "Read" {
                    return Vec::new();
                }
                vec![sse_event(
                    "response.function_call_arguments.delta",
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "delta": partial
                    }),
                )]
            }
            "thinking_delta" => {
                let text = delta.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                block.accum.push_str(text);
                vec![sse_event(
                    "response.reasoning_summary_text.delta",
                    json!({
                        "type": "response.reasoning_summary_text.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "summary_index": 0,
                        "delta": text
                    }),
                )]
            }
            // Ignore signature_delta and the like
            _ => Vec::new(),
        }
    }

    fn handle_content_block_stop(&mut self, data: &Value) -> Vec<Bytes> {
        let Some(index) = data.get("index").and_then(|v| v.as_u64()) else {
            return Vec::new();
        };
        self.close_block(index)
    }

    fn close_block(&mut self, index: u64) -> Vec<Bytes> {
        let Some(block) = self.blocks.get_mut(&index) else {
            return Vec::new();
        };
        if block.done {
            return Vec::new();
        }
        block.done = true;
        let output_index = block.output_index;
        let item_id = block.item_id.clone();
        let kind = block.kind;
        let text = block.accum.clone();
        let call_id = block.call_id.clone();
        let name = block.name.clone();

        match kind {
            BlockKind::Text => {
                let item = json!({
                    "id": item_id,
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text, "annotations": [] }]
                });
                self.output_items.push((output_index, item.clone()));
                vec![
                    sse_event(
                        "response.output_text.done",
                        json!({
                            "type": "response.output_text.done",
                            "item_id": item_id,
                            "output_index": output_index,
                            "content_index": 0,
                            "text": text
                        }),
                    ),
                    sse_event(
                        "response.content_part.done",
                        json!({
                            "type": "response.content_part.done",
                            "item_id": item_id,
                            "output_index": output_index,
                            "content_index": 0,
                            "part": { "type": "output_text", "text": text, "annotations": [] }
                        }),
                    ),
                    sse_event(
                        "response.output_item.done",
                        json!({
                            "type": "response.output_item.done",
                            "output_index": output_index,
                            "item": item
                        }),
                    ),
                ]
            }
            BlockKind::Tool => {
                let arguments = if name == "Read" {
                    sanitize_anthropic_tool_use_input_json("Read", &text)
                } else {
                    canonicalize_tool_arguments_str(&text)
                };
                let item = json!({
                    "id": item_id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments
                });
                self.output_items.push((output_index, item.clone()));
                vec![
                    sse_event(
                        "response.function_call_arguments.done",
                        json!({
                            "type": "response.function_call_arguments.done",
                            "item_id": item_id,
                            "output_index": output_index,
                            "arguments": arguments
                        }),
                    ),
                    sse_event(
                        "response.output_item.done",
                        json!({
                            "type": "response.output_item.done",
                            "output_index": output_index,
                            "item": item
                        }),
                    ),
                ]
            }
            BlockKind::Thinking => {
                let item = json!({
                    "id": item_id,
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": text }]
                });
                self.output_items.push((output_index, item.clone()));
                vec![
                    sse_event(
                        "response.reasoning_summary_text.done",
                        json!({
                            "type": "response.reasoning_summary_text.done",
                            "item_id": item_id,
                            "output_index": output_index,
                            "summary_index": 0,
                            "text": text
                        }),
                    ),
                    sse_event(
                        "response.reasoning_summary_part.done",
                        json!({
                            "type": "response.reasoning_summary_part.done",
                            "item_id": item_id,
                            "output_index": output_index,
                            "summary_index": 0,
                            "part": { "type": "summary_text", "text": text }
                        }),
                    ),
                    sse_event(
                        "response.output_item.done",
                        json!({
                            "type": "response.output_item.done",
                            "output_index": output_index,
                            "item": item
                        }),
                    ),
                ]
            }
        }
    }

    fn handle_message_delta(&mut self, data: &Value) -> Vec<Bytes> {
        if let Some(reason) = data.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
            self.stop_reason = Some(reason.to_string());
        }
        if let Some(usage) = data.get("usage") {
            self.merge_usage(usage);
        }
        Vec::new()
    }

    /// Whether any partial output was produced (completed items, buffered text, or a
    /// started tool call). Used to distinguish a truncated-with-output stream (report
    /// incomplete) from one that produced nothing (report failed).
    fn has_substantive_output(&self) -> bool {
        !self.output_items.is_empty()
            || self.blocks.values().any(|b| {
                !b.accum.trim().is_empty()
                    || !b.call_id.trim().is_empty()
                    || !b.name.trim().is_empty()
            })
    }

    fn finalize(&mut self) -> Vec<Bytes> {
        if self.completed {
            return Vec::new();
        }
        let mut events = self.ensure_response_started();

        // Close out any blocks that are still open
        let open: Vec<u64> = self
            .blocks
            .iter()
            .filter(|(_, b)| !b.done)
            .map(|(index, _)| *index)
            .collect();
        for index in open {
            events.extend(self.close_block(index));
        }

        let (status, incomplete_reason) =
            map_anthropic_stop_reason_to_status(self.stop_reason.as_deref());

        let mut output = self.output_items.clone();
        output.sort_by_key(|(output_index, _)| *output_index);
        let output: Vec<Value> = output.into_iter().map(|(_, item)| item).collect();

        let mut response = self.base_response(status, output);
        if let Some(reason) = incomplete_reason {
            response["incomplete_details"] = json!({ "reason": reason });
        }

        events.push(sse_event(
            "response.completed",
            json!({
                "type": "response.completed",
                "response": response
            }),
        ));
        self.completed = true;
        events
    }

    fn failed_event(&mut self, message: String, error_type: Option<String>) -> Bytes {
        self.completed = true;
        let mut error = json!({ "message": message });
        if let Some(error_type) = error_type.filter(|value| !value.is_empty()) {
            error["type"] = json!(error_type);
        }
        let mut output = self.output_items.clone();
        output.sort_by_key(|(output_index, _)| *output_index);
        let output: Vec<Value> = output.into_iter().map(|(_, item)| item).collect();
        let mut response = self.base_response("failed", output);
        response["error"] = error;
        sse_event(
            "response.failed",
            json!({
                "type": "response.failed",
                "response": response
            }),
        )
    }
}

fn extract_anthropic_sse_error(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    (message, error_type)
}

fn sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    ))
}

/// Convert the upstream Anthropic Messages SSE into the Responses SSE that Codex expects.
pub fn create_responses_sse_stream_from_anthropic<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = AnthropicToResponsesState::default();
        let mut stream_failed = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }

                        let mut event_name: Option<String> = None;
                        let mut data_parts: Vec<String> = Vec::new();
                        for line in block.lines() {
                            if let Some(event) = strip_sse_field(line, "event") {
                                event_name = Some(event.trim().to_string());
                            }
                            if let Some(data) = strip_sse_field(line, "data") {
                                data_parts.push(data.to_string());
                            }
                        }
                        if data_parts.is_empty() {
                            continue;
                        }

                        let data_str = data_parts.join("\n");
                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };

                        let msg_type = data
                            .get("type")
                            .and_then(|t| t.as_str())
                            .map(str::to_string)
                            .or_else(|| event_name.clone())
                            .unwrap_or_default();

                        match msg_type.as_str() {
                            "message_start" => {
                                for event in state.handle_message_start(&data) {
                                    yield Ok(event);
                                }
                            }
                            "content_block_start" => {
                                for event in state.handle_content_block_start(&data) {
                                    yield Ok(event);
                                }
                            }
                            "content_block_delta" => {
                                for event in state.handle_content_block_delta(&data) {
                                    yield Ok(event);
                                }
                            }
                            "content_block_stop" => {
                                for event in state.handle_content_block_stop(&data) {
                                    yield Ok(event);
                                }
                            }
                            "message_delta" => {
                                for event in state.handle_message_delta(&data) {
                                    yield Ok(event);
                                }
                            }
                            "message_stop" => {
                                for event in state.finalize() {
                                    yield Ok(event);
                                }
                            }
                            "error" => {
                                let (message, error_type) = extract_anthropic_sse_error(&data);
                                yield Ok(state.failed_event(message, error_type));
                                stream_failed = true;
                                break;
                            }
                            // Ignore ping and other unknown events
                            _ => {}
                        }
                    }

                    if stream_failed {
                        break;
                    }
                }
                Err(e) => {
                    yield Ok(state.failed_event(
                        format!("Stream error: {e}"),
                        Some("stream_error".to_string()),
                    ));
                    stream_failed = true;
                    break;
                }
            }
        }

        if !stream_failed && !state.completed {
            if state.stop_reason.is_some() {
                // message_delta (stop_reason + final usage) arrived but the stream ended
                // before message_stop; the turn is semantically complete, finalize normally.
                for event in state.finalize() {
                    yield Ok(event);
                }
            } else if state.has_substantive_output() {
                // Upstream truncated mid-stream (e.g. a proxy closed the connection without
                // an I/O error) after emitting partial output. Report it as incomplete so
                // Codex does not accept the truncated output as a normal completion.
                state.stop_reason = Some("max_tokens".to_string());
                for event in state.finalize() {
                    yield Ok(event);
                }
            } else {
                // Stream ended before any terminal signal or output: surface a failure.
                yield Ok(state.failed_event(
                    "Upstream Anthropic stream ended before message_stop".to_string(),
                    Some("stream_truncated".to_string()),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    async fn run(input: &str) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_responses_sse_stream_from_anthropic(upstream);
        let chunks: Vec<_> = converted.collect().await;
        chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>()
    }

    #[tokio::test]
    async fn test_text_stream() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("event: response.created"));
        assert!(merged.contains("\"id\":\"resp_msg_1\""));
        assert!(merged.contains("\"model\":\"claude\""));
        assert!(merged.contains("event: response.output_text.delta"));
        assert!(merged.contains("\"delta\":\"Hello\""));
        assert!(merged.contains("event: response.completed"));
        assert!(merged.contains("\"status\":\"completed\""));
        assert!(merged.contains("\"input_tokens\":12"));
        assert!(merged.contains("\"output_tokens\":3"));
    }

    #[tokio::test]
    async fn test_truncated_stream_with_output_reports_incomplete() {
        // Upstream closes after partial text but before message_delta/message_stop.
        // The partial output must be reported as incomplete, not a normal completion.
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_t1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":4}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"delta\":\"partial\""));
        assert!(merged.contains("event: response.completed"));
        // The top-level response is incomplete (message output items keep their own
        // "completed" status, but the response status must not be "completed").
        assert!(merged.contains("\"status\":\"incomplete\""));
        assert!(merged.contains("\"reason\":\"max_output_tokens\""));
    }

    #[tokio::test]
    async fn test_truncated_stream_without_output_reports_failed() {
        // Upstream closes before producing any output or terminal signal: report failed.
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_t2\",\"model\":\"claude\"}}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("event: response.failed"));
        assert!(merged.contains("stream_truncated"));
        assert!(!merged.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn test_stop_reason_without_message_stop_completes() {
        // message_delta carried the stop_reason and final usage, but the stream ended
        // before message_stop; the turn is complete and should finalize normally.
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_t3\",\"model\":\"claude\",\"usage\":{\"input_tokens\":4}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("event: response.completed"));
        assert!(merged.contains("\"status\":\"completed\""));
        assert!(!merged.contains("event: response.failed"));
    }

    #[tokio::test]
    async fn test_tool_use_stream() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"model\":\"claude\",\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"type\":\"function_call\""));
        assert!(merged.contains("\"call_id\":\"toolu_1\""));
        assert!(merged.contains("\"name\":\"get_weather\""));
        assert!(merged.contains("event: response.function_call_arguments.delta"));
        assert!(merged.contains("event: response.function_call_arguments.done"));
        assert!(merged.contains("\"status\":\"completed\""));
    }

    #[tokio::test]
    async fn test_thinking_stream() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_3\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"type\":\"reasoning\""));
        assert!(merged.contains("event: response.reasoning_summary_text.delta"));
        assert!(merged.contains("\"delta\":\"hmm\""));
    }

    #[tokio::test]
    async fn test_max_tokens_incomplete() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_4\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"status\":\"incomplete\""));
        assert!(merged.contains("\"reason\":\"max_output_tokens\""));
    }

    #[tokio::test]
    async fn test_read_tool_drops_empty_pages() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_5\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_r\",\"name\":\"Read\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/x\\\",\\\"pages\\\":\\\"\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("/tmp/x"));
        assert!(!merged.contains("pages"));
    }

    #[tokio::test]
    async fn test_error_event_becomes_failed() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_6\",\"model\":\"claude\"}}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"boom\"}}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("event: response.failed"));
        assert!(merged.contains("boom"));
    }
}
