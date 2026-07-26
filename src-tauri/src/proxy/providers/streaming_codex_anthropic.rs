//! Anthropic Messages SSE → OpenAI Responses SSE conversion.
//!
//! The opposite direction of `streaming_responses.rs` (Responses SSE → Anthropic SSE):
//! here the Codex client speaks Responses, while the upstream gateway speaks the native
//! Anthropic Messages protocol. The Responses events emitted here have the same shape as
//! those in `streaming_codex_chat.rs` (Chat → Responses); the Codex client only recognizes
//! this set of events.

use super::codex_responses_sse as sse;
use super::transform_codex_anthropic::{
    build_responses_usage_from_anthropic, map_anthropic_stop_reason_to_status,
    responses_reasoning_item_from_anthropic_block,
};
#[cfg(test)]
use super::transform_codex_anthropic::{
    decode_anthropic_thinking_block, ANTHROPIC_THINKING_ENCRYPTED_PREFIX,
};
use super::transform_codex_chat::{
    response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
    CodexToolContext,
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
    /// For `tool_use`: the `input` carried on `content_block_start` (compact JSON),
    /// used as a fallback when the gateway sends the full input in the start event and
    /// emits no `input_json_delta`. Empty otherwise.
    start_input: String,
    source_block: Value,
    has_visible_summary: bool,
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
    stream_truncated: bool,
    tool_context: CodexToolContext,
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
            stream_truncated: false,
            tool_context: CodexToolContext::default(),
        }
    }
}

impl AnthropicToResponsesState {
    fn with_tool_context(tool_context: CodexToolContext) -> Self {
        Self {
            tool_context,
            ..Self::default()
        }
    }

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
        let response = self.base_response("in_progress", Vec::new());
        vec![
            sse::response_created(&response),
            sse::response_in_progress(&response),
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
                events.push(sse::message_item_added(output_index, &item_id));
                events.push(sse::message_content_part_added(output_index, &item_id));
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Text,
                        output_index,
                        item_id,
                        call_id: String::new(),
                        name: String::new(),
                        accum: block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        start_input: String::new(),
                        source_block: block.clone(),
                        has_visible_summary: false,
                        done: false,
                    },
                );
            }
            "tool_use" => {
                let output_index = self.next_output_index();
                let call_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                // Some gateways put the full tool input on content_block_start and emit
                // no input_json_delta; capture it as a fallback (see close_block).
                let start_input = block
                    .get("input")
                    .filter(|v| v.as_object().map(|o| !o.is_empty()).unwrap_or(false))
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let item_id =
                    response_tool_call_item_id_from_chat_name(call_id, name, &self.tool_context);
                let item = response_tool_call_item_from_chat_name(
                    &item_id,
                    "in_progress",
                    call_id,
                    name,
                    "",
                    None,
                    &self.tool_context,
                );
                events.push(sse::output_item_added(output_index, &item));
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Tool,
                        output_index,
                        item_id,
                        call_id: call_id.to_string(),
                        name: name.to_string(),
                        accum: String::new(),
                        start_input,
                        source_block: block.clone(),
                        has_visible_summary: false,
                        done: false,
                    },
                );
            }
            "thinking" | "redacted_thinking" => {
                let output_index = self.next_output_index();
                let item_id = format!("rs_{}_{output_index}", self.response_id);
                events.push(sse::reasoning_item_added(output_index, &item_id));
                let has_visible_summary = block_type == "thinking";
                if has_visible_summary {
                    events.push(sse::reasoning_summary_part_added(output_index, &item_id));
                }
                self.blocks.insert(
                    index,
                    BlockState {
                        kind: BlockKind::Thinking,
                        output_index,
                        item_id,
                        call_id: String::new(),
                        name: String::new(),
                        accum: block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        start_input: String::new(),
                        source_block: block.clone(),
                        has_visible_summary,
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
                vec![sse::output_text_delta(output_index, &item_id, text)]
            }
            "input_json_delta" => {
                let partial = delta
                    .get("partial_json")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                block.accum.push_str(partial);
                // The Read tool needs to be sanitized at close time, to avoid emitting pages:"" deltas mid-stream
                if block.name == "Read" || self.tool_context.is_custom_tool_chat_name(&block.name) {
                    return Vec::new();
                }
                vec![sse::function_call_arguments_delta(
                    output_index,
                    &item_id,
                    partial,
                )]
            }
            "thinking_delta" => {
                let text = delta.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                block.accum.push_str(text);
                block.source_block["thinking"] = json!(block.accum);
                vec![sse::reasoning_summary_text_delta(
                    output_index,
                    &item_id,
                    text,
                )]
            }
            "signature_delta" => {
                if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                    block.source_block["signature"] = json!(signature);
                }
                Vec::new()
            }
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
        let source_block = block.source_block.clone();
        let has_visible_summary = block.has_visible_summary;

        match kind {
            BlockKind::Text => {
                let (events, item) = sse::message_close(output_index, &item_id, &text);
                self.output_items.push((output_index, item));
                events
            }
            BlockKind::Tool => {
                // Prefer streamed input_json_delta; fall back to the input carried on
                // content_block_start when the gateway emitted no deltas (mirrors the
                // non-streaming aggregator's precedence in transform_codex_anthropic).
                let raw_input = if text.trim().is_empty() {
                    block.start_input.clone()
                } else {
                    text
                };
                let arguments = if raw_input.trim().is_empty() {
                    "{}".to_string()
                } else if name == "Read" {
                    sanitize_anthropic_tool_use_input_json("Read", &raw_input)
                } else {
                    canonicalize_tool_arguments_str(&raw_input)
                };
                let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&name);
                let item = response_tool_call_item_from_chat_name(
                    &item_id,
                    if self.stream_truncated {
                        "incomplete"
                    } else {
                        "completed"
                    },
                    &call_id,
                    &name,
                    &arguments,
                    None,
                    &self.tool_context,
                );
                let mut events = Vec::new();
                if !self.stream_truncated {
                    if is_custom_tool {
                        let input = item.get("input").and_then(Value::as_str).unwrap_or("");
                        events.push(sse::custom_tool_call_input_done(
                            output_index,
                            &item_id,
                            input,
                        ));
                    } else {
                        events.push(sse::function_call_arguments_done(
                            output_index,
                            &item_id,
                            &arguments,
                        ));
                    }
                }
                events.push(sse::output_item_done(output_index, &item));
                self.output_items.push((output_index, item));
                events
            }
            BlockKind::Thinking => {
                let mut source_block = source_block;
                if source_block.get("type").and_then(Value::as_str) == Some("thinking") {
                    source_block["thinking"] = json!(text);
                }
                let Some(item) =
                    responses_reasoning_item_from_anthropic_block(&item_id, &source_block)
                else {
                    return Vec::new();
                };
                let events = sse::reasoning_close_with_item(
                    output_index,
                    &item_id,
                    &text,
                    &item,
                    has_visible_summary,
                );
                self.output_items.push((output_index, item));
                events
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

        events.push(sse::response_completed(&response));
        self.completed = true;
        events
    }

    fn failed_event(&mut self, message: String, error_type: Option<String>) -> Option<Bytes> {
        if self.completed {
            return None;
        }
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
        Some(sse::response_failed(&response))
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

fn process_anthropic_sse_block(
    state: &mut AnthropicToResponsesState,
    block: &str,
) -> (Vec<Bytes>, bool) {
    if block.trim().is_empty() {
        return (Vec::new(), false);
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
        return (Vec::new(), false);
    }
    let Ok(data) = serde_json::from_str::<Value>(&data_parts.join("\n")) else {
        return (Vec::new(), false);
    };
    let msg_type = data
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(event_name)
        .unwrap_or_default();

    let events = match msg_type.as_str() {
        "message_start" => state.handle_message_start(&data),
        "content_block_start" => state.handle_content_block_start(&data),
        "content_block_delta" => state.handle_content_block_delta(&data),
        "content_block_stop" => state.handle_content_block_stop(&data),
        "message_delta" => state.handle_message_delta(&data),
        "message_stop" => state.finalize(),
        "error" => {
            let (message, error_type) = extract_anthropic_sse_error(&data);
            return (
                state
                    .failed_event(message, error_type)
                    .into_iter()
                    .collect(),
                true,
            );
        }
        _ => Vec::new(),
    };
    (events, false)
}

fn json_document_candidate(input: &str) -> Option<&str> {
    let trimmed = input.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
    matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')).then_some(trimmed)
}

/// Convert a complete non-streaming Anthropic message (or error envelope) into
/// the same Responses SSE lifecycle emitted by the live stream converter. Some
/// compatible gateways ignore `stream:true` and return JSON with HTTP 200.
pub(crate) fn responses_sse_events_from_anthropic_message(
    body: &Value,
    tool_context: CodexToolContext,
) -> Vec<Bytes> {
    let mut state = AnthropicToResponsesState::with_tool_context(tool_context);
    if body.get("type").and_then(Value::as_str) == Some("error") || body.get("error").is_some() {
        let (message, error_type) = extract_anthropic_sse_error(body);
        return state
            .failed_event(message, error_type)
            .into_iter()
            .collect();
    }

    let mut message_start = body.clone();
    message_start["content"] = json!([]);
    let mut events = state.handle_message_start(&json!({
        "type": "message_start",
        "message": message_start
    }));

    if let Some(content) = body.get("content").and_then(Value::as_array) {
        for (index, block) in content.iter().enumerate() {
            let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
            let mut start_block = block.clone();
            match block_type {
                "text" => start_block["text"] = json!(""),
                "thinking" => start_block["thinking"] = json!(""),
                _ => {}
            }
            events.extend(state.handle_content_block_start(&json!({
                "type": "content_block_start",
                "index": index,
                "content_block": start_block
            })));

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        events.extend(state.handle_content_block_delta(&json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": { "type": "text_delta", "text": text }
                        })));
                    }
                }
                "thinking" => {
                    if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                        events.extend(state.handle_content_block_delta(&json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": { "type": "thinking_delta", "thinking": thinking }
                        })));
                    }
                }
                _ => {}
            }

            events.extend(state.handle_content_block_stop(&json!({
                "type": "content_block_stop",
                "index": index
            })));
        }
    }

    events.extend(state.handle_message_delta(&json!({
        "type": "message_delta",
        "delta": { "stop_reason": body.get("stop_reason").cloned().unwrap_or(Value::Null) }
    })));
    events.extend(state.finalize());
    events
}

/// Convert the upstream Anthropic Messages SSE into the Responses SSE that Codex expects.
#[allow(dead_code)]
pub fn create_responses_sse_stream_from_anthropic<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_responses_sse_stream_from_anthropic_with_context(stream, CodexToolContext::default())
}

pub(crate) fn create_responses_sse_stream_from_anthropic_with_context<
    E: std::error::Error + Send + 'static,
>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: CodexToolContext,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = AnthropicToResponsesState::with_tool_context(tool_context);
        let mut stream_failed = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    // A few compatible gateways ignore stream:true and return one
                    // JSON document. Hold that body intact (including pretty-printed
                    // blank lines) until EOF instead of discarding it as SSE blocks.
                    if json_document_candidate(&buffer).is_none() {
                        while let Some(block) = take_sse_block(&mut buffer) {
                            let (events, failed) = process_anthropic_sse_block(&mut state, &block);
                            for event in events {
                                yield Ok(event);
                            }
                            if failed {
                                stream_failed = true;
                                break;
                            }
                        }
                    }

                    if stream_failed {
                        break;
                    }
                }
                Err(e) => {
                    if let Some(event) = state.failed_event(
                        format!("Stream error: {e}"),
                        Some("stream_error".to_string()),
                    ) {
                        yield Ok(event);
                    }
                    stream_failed = true;
                    break;
                }
            }
        }

        // Process a final event even when the upstream omitted the trailing blank
        // line. This is common with buffering reverse proxies and must not discard
        // the last delta or terminal message_stop.
        if !stream_failed && !buffer.trim().is_empty() {
            if !state.response_started {
                if let Some(candidate) = json_document_candidate(&buffer) {
                    if let Ok(body) = serde_json::from_str::<Value>(candidate) {
                        for event in responses_sse_events_from_anthropic_message(
                            &body,
                            state.tool_context.clone(),
                        ) {
                            yield Ok(event);
                        }
                        state.completed = true;
                    }
                }
            }
            if !state.completed {
                let (events, failed) = process_anthropic_sse_block(&mut state, &buffer);
                for event in events {
                    yield Ok(event);
                }
                stream_failed = failed;
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
                state.stream_truncated = true;
                for event in state.finalize() {
                    yield Ok(event);
                }
            } else {
                // Stream ended before any terminal signal or output: surface a failure.
                if let Some(event) = state.failed_event(
                    "Upstream Anthropic stream ended before message_stop".to_string(),
                    Some("stream_truncated".to_string()),
                ) {
                    yield Ok(event);
                }
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

    async fn run_with_context(input: &str, context: CodexToolContext) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        create_responses_sse_stream_from_anthropic_with_context(upstream, context)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect()
    }

    fn render_message_events(body: &Value) -> String {
        responses_sse_events_from_anthropic_message(body, CodexToolContext::default())
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(&chunk).to_string())
            .collect()
    }

    #[test]
    fn test_json_error_envelope_preserves_upstream_error() {
        let merged = render_message_events(&json!({
            "type": "error",
            "error": { "type": "authentication_error", "message": "bad key" }
        }));
        assert!(merged.contains("event: response.failed"));
        assert!(merged.contains("authentication_error"));
        assert!(merged.contains("bad key"));
        assert!(!merged.contains("stream_truncated"));
    }

    #[test]
    fn test_json_message_becomes_complete_responses_stream() {
        let merged = render_message_events(&json!({
            "id": "msg_json",
            "type": "message",
            "role": "assistant",
            "model": "claude",
            "content": [{ "type": "text", "text": "Hello" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 4, "output_tokens": 2 }
        }));
        assert!(merged.contains("event: response.created"));
        assert!(merged.contains("event: response.output_text.delta"));
        assert!(merged.contains("\"delta\":\"Hello\""));
        assert!(merged.contains("event: response.completed"));
        assert!(merged.contains("\"status\":\"completed\""));
        assert!(!merged.contains("event: response.failed"));
    }

    #[tokio::test]
    async fn test_raw_json_error_body_is_not_reported_as_truncated_sse() {
        let merged = run(concat!(
            "{\n",
            "  \"type\": \"error\",\n\n",
            "  \"error\": {\"type\":\"overloaded_error\",\"message\":\"busy\"}\n",
            "}"
        ))
        .await;
        assert!(merged.contains("event: response.failed"));
        assert!(merged.contains("overloaded_error"));
        assert!(merged.contains("busy"));
        assert!(!merged.contains("stream_truncated"));
    }

    #[tokio::test]
    async fn test_raw_json_message_body_becomes_responses_stream() {
        let merged = run(
            r#"{"id":"msg_json","type":"message","role":"assistant","model":"claude","content":[{"type":"text","text":"Hello"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":2}}"#,
        )
        .await;
        assert!(merged.contains("event: response.output_text.delta"));
        assert!(merged.contains("\"delta\":\"Hello\""));
        assert!(merged.contains("event: response.completed"));
        assert!(!merged.contains("stream_truncated"));
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
    async fn test_truncated_tool_call_is_not_marked_completed() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool_truncated\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"exec\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\"}}\n\n"
        );

        let merged = run(input).await;
        assert!(merged.contains("\"status\":\"incomplete\""));
        assert!(!merged.contains("event: response.function_call_arguments.done"));
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
    async fn test_tool_use_input_only_in_start_event() {
        // Some gateways carry the full tool input on content_block_start and emit no
        // input_json_delta. The arguments must still be populated (previously empty) —
        // matching the non-streaming aggregator's behavior.
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_si\",\"model\":\"claude\",\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_i\",\"name\":\"get_weather\",\"input\":{\"city\":\"Tokyo\"}}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"name\":\"get_weather\""));
        assert!(merged.contains("event: response.function_call_arguments.done"));
        // The tool arguments came from the start event, not from any delta.
        assert!(merged.contains("Tokyo"));
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
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
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
        assert!(merged.contains(ANTHROPIC_THINKING_ENCRYPTED_PREFIX));
    }

    #[tokio::test]
    async fn test_thinking_signature_is_preserved() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_sig\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_abc\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        let encoded_start = merged.find(ANTHROPIC_THINKING_ENCRYPTED_PREFIX).unwrap();
        let encoded = merged[encoded_start..].split('"').next().unwrap();
        let block = decode_anthropic_thinking_block(encoded).unwrap();
        assert_eq!(block["signature"], "sig_abc");
        assert_eq!(block["thinking"], "hmm");
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
    async fn test_empty_read_input_finishes_as_empty_object() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_read\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_r\",\"name\":\"Read\",\"input\":{}}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"arguments\":\"{}\""));
    }

    #[tokio::test]
    async fn test_namespace_tool_stream_restores_namespace() {
        let context = super::super::transform_codex_chat::build_codex_tool_context_from_request(
            &json!({
                "tools": [{
                    "type": "namespace",
                    "name": "mcp_files",
                    "tools": [{"type": "function", "name": "read", "parameters": {"type": "object"}}]
                }]
            }),
        );
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_ns\",\"model\":\"claude\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"mcp_files__read\",\"input\":{}}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let merged = run_with_context(input, context).await;
        assert!(merged.contains("\"namespace\":\"mcp_files\""));
        assert!(merged.contains("\"name\":\"read\""));
    }

    #[tokio::test]
    async fn test_final_event_without_blank_line_is_processed() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tail\"}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"tail\"}}"
        );
        let merged = run(input).await;
        assert!(merged.contains("\"delta\":\"tail\""));
        assert!(merged.contains("\"status\":\"incomplete\""));
    }

    #[tokio::test]
    async fn test_error_after_message_stop_does_not_emit_second_terminal_event() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_terminal\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"message\":\"late\"}}\n\n"
        );
        let merged = run(input).await;
        assert_eq!(merged.matches("event: response.completed").count(), 1);
        assert_eq!(merged.matches("event: response.failed").count(), 0);
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
