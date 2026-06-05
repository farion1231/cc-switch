//! OpenAI Chat Completions SSE → OpenAI Responses SSE conversion.

use super::{
    codex_chat_common::{
        extract_reasoning_field_text, split_leading_think_block, strip_leading_think_open_tag,
    },
    transform_codex_chat::{
        chat_usage_to_responses_usage, custom_tool_input_from_chat_arguments,
        response_id_from_chat_id, response_status_from_finish_reason,
        response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
        CodexToolContext,
    },
};
use crate::proxy::json_canonical::canonicalize_tool_arguments_str;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct TextItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    added: bool,
    done: bool,
}

#[derive(Debug, Default)]
struct ReasoningItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    added: bool,
    done: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum InlineThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

#[derive(Debug, Default)]
struct InlineThinkState {
    mode: InlineThinkMode,
    buffer: String,
}

#[derive(Debug, Default)]
struct ToolCallState {
    output_index: Option<u32>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    reasoning_content: String,
    added: bool,
    done: bool,
}

#[derive(Debug)]
struct ChatToResponsesState {
    response_started: bool,
    completed: bool,
    response_id: String,
    model: String,
    created_at: u64,
    next_output_index: u32,
    text: TextItemState,
    reasoning: ReasoningItemState,
    inline_think: InlineThinkState,
    tools: BTreeMap<usize, ToolCallState>,
    output_items: Vec<(u32, Value)>,
    latest_usage: Option<Value>,
    finish_reason: Option<String>,
    tool_context: CodexToolContext,
}

impl Default for ChatToResponsesState {
    fn default() -> Self {
        Self {
            response_started: false,
            completed: false,
            response_id: "resp_ccswitch".to_string(),
            model: String::new(),
            created_at: 0,
            next_output_index: 0,
            text: TextItemState::default(),
            reasoning: ReasoningItemState::default(),
            inline_think: InlineThinkState::default(),
            tools: BTreeMap::new(),
            output_items: Vec::new(),
            latest_usage: None,
            finish_reason: None,
            tool_context: CodexToolContext::default(),
        }
    }
}

impl ChatToResponsesState {
    fn with_tool_context(tool_context: CodexToolContext) -> Self {
        Self {
            tool_context,
            ..Self::default()
        }
    }

    fn handle_chat_chunk(&mut self, chunk: &Value) -> Vec<Bytes> {
        let mut events = Vec::new();

        if let Some(id) = chunk.get("id").and_then(|v| v.as_str()) {
            self.response_id = response_id_from_chat_id(Some(id));
        }
        if let Some(model) = chunk.get("model").and_then(|v| v.as_str()) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(created) = chunk.get("created").and_then(|v| v.as_u64()) {
            self.created_at = created;
        }

        events.extend(self.ensure_response_started());

        if let Some(usage) = chunk.get("usage").filter(|v| !v.is_null()) {
            self.latest_usage = Some(chat_usage_to_responses_usage(Some(usage)));
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|choices| choices.first())
        else {
            return events;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(reasoning) = chat_delta_reasoning_text(delta) {
                events.extend(self.push_reasoning_delta(&reasoning));
            }

            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    events.extend(self.push_content_delta(content));
                }
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                events.extend(self.flush_inline_think_at_boundary());
                let reasoning_for_tool_call = self.current_reasoning_text();
                events.extend(self.finalize_reasoning());
                for tool_call in tool_calls {
                    events.extend(
                        self.push_tool_call_delta(tool_call, reasoning_for_tool_call.as_deref()),
                    );
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = Some(finish_reason.to_string());
        }

        events
    }

    fn push_content_delta(&mut self, delta: &str) -> Vec<Bytes> {
        match self.inline_think.mode {
            InlineThinkMode::Text => {
                let mut events = self.finalize_reasoning();
                events.extend(self.push_text_delta(delta));
                events
            }
            InlineThinkMode::Detecting => {
                self.inline_think.buffer.push_str(delta);
                match leading_think_prefix_decision(&self.inline_think.buffer) {
                    ThinkPrefixDecision::NeedMore => Vec::new(),
                    ThinkPrefixDecision::Reasoning => {
                        self.inline_think.mode = InlineThinkMode::Reasoning;
                        self.drain_complete_inline_think()
                    }
                    ThinkPrefixDecision::Text => {
                        self.inline_think.mode = InlineThinkMode::Text;
                        let text = std::mem::take(&mut self.inline_think.buffer);
                        let mut events = self.finalize_reasoning();
                        events.extend(self.push_text_delta(&text));
                        events
                    }
                }
            }
            InlineThinkMode::Reasoning => {
                self.inline_think.buffer.push_str(delta);
                self.drain_complete_inline_think()
            }
        }
    }

    fn drain_complete_inline_think(&mut self) -> Vec<Bytes> {
        let Some((reasoning, answer)) = split_leading_think_block(&self.inline_think.buffer) else {
            return Vec::new();
        };

        self.inline_think.mode = InlineThinkMode::Text;
        self.inline_think.buffer.clear();

        let mut events = Vec::new();
        if !reasoning.is_empty() {
            events.extend(self.push_reasoning_delta(&reasoning));
            events.extend(self.finalize_reasoning());
        }
        if !answer.is_empty() {
            events.extend(self.push_text_delta(&answer));
        }

        events
    }

    fn flush_inline_think_at_boundary(&mut self) -> Vec<Bytes> {
        match self.inline_think.mode {
            InlineThinkMode::Text => Vec::new(),
            InlineThinkMode::Detecting => {
                self.inline_think.mode = InlineThinkMode::Text;
                let text = std::mem::take(&mut self.inline_think.buffer);
                if text.is_empty() {
                    Vec::new()
                } else {
                    let mut events = self.finalize_reasoning();
                    events.extend(self.push_text_delta(&text));
                    events
                }
            }
            InlineThinkMode::Reasoning => {
                let buffered = std::mem::take(&mut self.inline_think.buffer);
                self.inline_think.mode = InlineThinkMode::Text;
                if let Some((reasoning, answer)) = split_leading_think_block(&buffered) {
                    let mut events = Vec::new();
                    if !reasoning.is_empty() {
                        events.extend(self.push_reasoning_delta(&reasoning));
                        events.extend(self.finalize_reasoning());
                    }
                    if !answer.is_empty() {
                        events.extend(self.push_text_delta(&answer));
                    }
                    return events;
                }

                let reasoning = strip_leading_think_open_tag(&buffered).unwrap_or(buffered);
                if reasoning.is_empty() {
                    Vec::new()
                } else {
                    let mut events = self.push_reasoning_delta(&reasoning);
                    events.extend(self.finalize_reasoning());
                    events
                }
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
            sse_event(
                "response.created",
                json!({
                    "type": "response.created",
                    "response": response
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

    fn push_reasoning_delta(&mut self, delta: &str) -> Vec<Bytes> {
        let mut events = Vec::new();

        if !self.reasoning.added {
            let output_index = self.next_output_index();
            let item_id = format!("rs_{}", self.response_id);
            self.reasoning.output_index = Some(output_index);
            self.reasoning.item_id = item_id.clone();
            self.reasoning.added = true;

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
                    "item_id": self.reasoning.item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": {
                        "type": "summary_text",
                        "text": ""
                    }
                }),
            ));
        }

        self.reasoning.text.push_str(delta);
        let output_index = self.reasoning.output_index.unwrap_or(0);
        events.push(sse_event(
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": self.reasoning.item_id,
                "output_index": output_index,
                "summary_index": 0,
                "delta": delta
            }),
        ));

        events
    }

    fn push_text_delta(&mut self, delta: &str) -> Vec<Bytes> {
        let mut events = Vec::new();

        if !self.text.added {
            let output_index = self.next_output_index();
            let item_id = format!("{}_msg", self.response_id);
            self.text.output_index = Some(output_index);
            self.text.item_id = item_id.clone();
            self.text.added = true;

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
                    "item_id": self.text.item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": {
                        "type": "output_text",
                        "text": "",
                        "annotations": []
                    }
                }),
            ));
        }

        self.text.text.push_str(delta);
        let output_index = self.text.output_index.unwrap_or(0);
        events.push(sse_event(
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "item_id": self.text.item_id,
                "output_index": output_index,
                "content_index": 0,
                "delta": delta
            }),
        ));

        events
    }

    fn current_reasoning_text(&self) -> Option<String> {
        (!self.reasoning.text.trim().is_empty()).then(|| self.reasoning.text.trim().to_string())
    }

    fn push_tool_call_delta(&mut self, tool_call: &Value, reasoning: Option<&str>) -> Vec<Bytes> {
        let chat_index = tool_call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let id_delta = tool_call
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let function = tool_call.get("function").unwrap_or(&Value::Null);
        let name_delta = function
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let args_delta = function
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut should_add = false;
        let mut output_index = None;
        let mut item_id = String::new();
        let mut pending_arguments = String::new();
        let current_name: String;

        {
            let state = self.tools.entry(chat_index).or_default();
            if let Some(id) = id_delta {
                state.call_id = id;
            }
            if let Some(name) = name_delta {
                state.name = name;
            }
            if !args_delta.is_empty() {
                state.arguments.push_str(&args_delta);
            }
            if state.reasoning_content.is_empty() {
                if let Some(reasoning) = reasoning.map(str::trim).filter(|value| !value.is_empty())
                {
                    state.reasoning_content = reasoning.to_string();
                }
            }

            if !state.added && (!state.call_id.is_empty() || !state.name.is_empty()) {
                should_add = true;
                pending_arguments = state.arguments.clone();
            } else if state.added {
                output_index = state.output_index;
                item_id = state.item_id.clone();
            }
            current_name = state.name.clone();
        }

        let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&current_name);
        let mut events = Vec::new();

        if should_add {
            let assigned = self.next_output_index();
            let Some(state) = self.tools.get_mut(&chat_index) else {
                return events;
            };
            state.added = true;
            if state.call_id.is_empty() {
                state.call_id = format!("call_{chat_index}");
            }
            if state.name.is_empty() {
                state.name = "unknown_tool".to_string();
            }
            state.output_index = Some(assigned);
            let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&state.name);
            state.item_id = response_tool_call_item_id_from_chat_name(
                &state.call_id,
                &state.name,
                &self.tool_context,
            );
            item_id = state.item_id.clone();

            let item = response_tool_call_item_from_chat_name(
                &item_id,
                "in_progress",
                &state.call_id,
                &state.name,
                "",
                Some(&state.reasoning_content),
                &self.tool_context,
            );

            events.push(sse_event(
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "output_index": assigned,
                    "item": item
                }),
            ));

            if !pending_arguments.is_empty() && !is_custom_tool {
                events.push(sse_event(
                    "response.function_call_arguments.delta",
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": state.item_id,
                        "output_index": assigned,
                        "delta": pending_arguments
                    }),
                ));
            }
        } else if !args_delta.is_empty() && !is_custom_tool {
            if let Some(output_index) = output_index {
                events.push(sse_event(
                    "response.function_call_arguments.delta",
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "delta": args_delta
                    }),
                ));
            }
        }

        events
    }

    fn finalize(&mut self) -> Vec<Bytes> {
        if self.completed {
            return Vec::new();
        }

        let mut events = self.ensure_response_started();
        events.extend(self.flush_inline_think_at_boundary());
        events.extend(self.finalize_reasoning());
        events.extend(self.finalize_text());
        events.extend(self.finalize_tools());

        let status = response_status_from_finish_reason(self.finish_reason.as_deref());
        let mut response = self.base_response(status, self.completed_output_items());
        if status == "incomplete" {
            response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
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

    fn finalize_reasoning(&mut self) -> Vec<Bytes> {
        if !self.reasoning.added || self.reasoning.done {
            return Vec::new();
        }

        let output_index = self.reasoning.output_index.unwrap_or(0);
        let item_id = self.reasoning.item_id.clone();
        let text = self.reasoning.text.clone();
        let item = json!({
            "id": item_id,
            "type": "reasoning",
            "summary": [{
                "type": "summary_text",
                "text": text
            }]
        });
        self.output_items.push((output_index, item.clone()));
        self.reasoning.done = true;

        vec![
            sse_event(
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": self.reasoning.item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "text": self.reasoning.text
                }),
            ),
            sse_event(
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": self.reasoning.item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": {
                        "type": "summary_text",
                        "text": self.reasoning.text
                    }
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

    fn finalize_text(&mut self) -> Vec<Bytes> {
        if !self.text.added || self.text.done {
            return Vec::new();
        }

        let output_index = self.text.output_index.unwrap_or(0);
        let item = json!({
            "id": self.text.item_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": self.text.text,
                "annotations": []
            }]
        });
        self.output_items.push((output_index, item.clone()));
        self.text.done = true;

        vec![
            sse_event(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": self.text.item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "text": self.text.text
                }),
            ),
            sse_event(
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "item_id": self.text.item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": {
                        "type": "output_text",
                        "text": self.text.text,
                        "annotations": []
                    }
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

    fn finalize_tools(&mut self) -> Vec<Bytes> {
        let mut events = Vec::new();
        let keys: Vec<usize> = self.tools.keys().copied().collect();

        for key in keys {
            let mut add_event: Option<Bytes> = None;
            if self.tools.get(&key).map(|state| state.done).unwrap_or(true) {
                continue;
            }

            if self
                .tools
                .get(&key)
                .map(|state| !state.added && !state.done)
                .unwrap_or(false)
            {
                let assigned = self.next_output_index();
                let Some(state) = self.tools.get_mut(&key) else {
                    continue;
                };
                state.added = true;
                if state.call_id.is_empty() {
                    state.call_id = format!("call_{key}");
                }
                if state.name.is_empty() {
                    state.name = "unknown_tool".to_string();
                }
                state.output_index = Some(assigned);
                state.item_id = response_tool_call_item_id_from_chat_name(
                    &state.call_id,
                    &state.name,
                    &self.tool_context,
                );
                let item = response_tool_call_item_from_chat_name(
                    &state.item_id,
                    "in_progress",
                    &state.call_id,
                    &state.name,
                    "",
                    Some(&state.reasoning_content),
                    &self.tool_context,
                );
                add_event = Some(sse_event(
                    "response.output_item.added",
                    json!({
                        "type": "response.output_item.added",
                        "output_index": assigned,
                        "item": item
                    }),
                ));
            }

            if let Some(event) = add_event {
                events.push(event);
            }

            let Some(state) = self.tools.get_mut(&key) else {
                continue;
            };
            let output_index = state.output_index.unwrap_or(0);
            let arguments = canonicalize_tool_arguments_str(&state.arguments);
            let is_custom_tool = self.tool_context.is_custom_tool_chat_name(&state.name);
            let item = response_tool_call_item_from_chat_name(
                &state.item_id,
                "completed",
                &state.call_id,
                &state.name,
                &arguments,
                Some(&state.reasoning_content),
                &self.tool_context,
            );
            state.done = true;
            self.output_items.push((output_index, item.clone()));

            if is_custom_tool {
                let input = custom_tool_input_from_chat_arguments(&arguments);
                if !input.is_empty() {
                    events.push(sse_event(
                        "response.custom_tool_call_input.delta",
                        json!({
                            "type": "response.custom_tool_call_input.delta",
                            "item_id": state.item_id,
                            "output_index": output_index,
                            "delta": input.clone()
                        }),
                    ));
                }
                events.push(sse_event(
                    "response.custom_tool_call_input.done",
                    json!({
                        "type": "response.custom_tool_call_input.done",
                        "item_id": state.item_id,
                        "output_index": output_index,
                        "input": input
                    }),
                ));
            } else {
                events.push(sse_event(
                    "response.function_call_arguments.done",
                    json!({
                        "type": "response.function_call_arguments.done",
                        "item_id": state.item_id,
                        "output_index": output_index,
                        "arguments": arguments
                    }),
                ));
            }
            events.push(sse_event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                }),
            ));
        }

        events
    }

    fn completed_output_items(&self) -> Vec<Value> {
        let mut output_items = self.output_items.clone();
        output_items.sort_by_key(|(output_index, _)| *output_index);
        output_items
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>()
    }

    fn base_response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": self.latest_usage.clone().unwrap_or_else(|| {
                json!({
                    "input_tokens": 0,
                    "output_tokens": 0,
                    "total_tokens": 0,
                    "output_tokens_details": { "reasoning_tokens": 0 }
                })
            })
        })
    }

    fn next_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    fn failed_event(&mut self, message: String, error_type: Option<String>) -> Bytes {
        self.completed = true;
        let mut error = json!({ "message": message });
        if let Some(error_type) = error_type.filter(|value| !value.is_empty()) {
            error["type"] = json!(error_type);
        }

        let mut response = self.base_response("failed", self.completed_output_items());
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

fn chat_delta_reasoning_text(delta: &Value) -> Option<String> {
    extract_reasoning_field_text(delta)
}

enum ThinkPrefixDecision {
    NeedMore,
    Reasoning,
    Text,
}

fn leading_think_prefix_decision(buffer: &str) -> ThinkPrefixDecision {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return ThinkPrefixDecision::NeedMore;
    }

    if trimmed.starts_with("<think>") {
        return ThinkPrefixDecision::Reasoning;
    }

    if "<think>".starts_with(trimmed) {
        return ThinkPrefixDecision::NeedMore;
    }

    ThinkPrefixDecision::Text
}

/// Create a stream that converts Chat Completions SSE chunks into Responses SSE events.
#[allow(dead_code)]
pub fn create_responses_sse_stream_from_chat<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_responses_sse_stream_from_chat_with_context(stream, CodexToolContext::default())
}

/// Create a stream that converts Chat Completions SSE chunks into Responses SSE
/// events while restoring Codex tool namespace/custom/tool_search metadata.
pub fn create_responses_sse_stream_from_chat_with_context<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: CodexToolContext,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = ChatToResponsesState::with_tool_context(tool_context);
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

                        let data = data_parts.join("\n");
                        if data.trim() == "[DONE]" {
                            for event in state.finalize() {
                                yield Ok(event);
                            }
                            continue;
                        }

                        let chunk: Value = match serde_json::from_str(&data) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };

                        if event_name.as_deref() == Some("error") || chunk.get("error").is_some() {
                            let (message, error_type) = extract_chat_sse_error(&chunk);
                            yield Ok(state.failed_event(message, error_type));
                            stream_failed = true;
                            break;
                        }

                        for event in state.handle_chat_chunk(&chunk) {
                            yield Ok(event);
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

        if !stream_failed {
            for event in state.finalize() {
                yield Ok(event);
            }
        }
    }
}

fn extract_chat_sse_error(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .or_else(|| error.get("code"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    async fn collect(chunks: Vec<&str>) -> String {
        collect_with_context(chunks, CodexToolContext::default()).await
    }

    async fn collect_with_context(chunks: Vec<&str>, tool_context: CodexToolContext) -> String {
        let chunks: Vec<Result<Bytes, std::io::Error>> = chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk.as_bytes())))
            .collect();
        let upstream = stream::iter(chunks);
        let converted = create_responses_sse_stream_from_chat_with_context(upstream, tool_context);
        let bytes: Vec<Bytes> = converted.map(|item| item.unwrap()).collect().await;
        String::from_utf8(bytes.concat()).unwrap()
    }

    #[tokio::test]
    async fn converts_text_chat_sse_to_responses_sse() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_1\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.created"));
        assert!(output.contains("event: response.output_text.delta"));
        assert!(output.contains("\"text\":\"Hello\""));
        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"input_tokens\":4"));
    }

    #[tokio::test]
    async fn converts_reasoning_content_chat_sse_to_responses_reasoning_events() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Need context. \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"reasoning\":\"Now answer. \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_reason\",\"created\":123,\"model\":\"deepseek-reasoner\",\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6,\"total_tokens\":10,\"completion_tokens_details\":{\"reasoning_tokens\":3}}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.reasoning_summary_part.added"));
        assert!(output.contains("event: response.reasoning_summary_text.delta"));
        assert!(output.contains("event: response.reasoning_summary_text.done"));
        assert!(output.contains("Need context. Now answer. "));
        assert!(output.contains("\"type\":\"reasoning\""));
        assert!(output.contains("\"text\":\"Done\""));
        assert!(output.contains("\"reasoning_tokens\":3"));

        let reasoning_pos = output.find("\"type\":\"reasoning\"").unwrap();
        let message_pos = output.find("\"type\":\"message\"").unwrap();
        assert!(reasoning_pos < message_pos);
    }

    #[tokio::test]
    async fn converts_inline_think_chat_sse_to_reasoning_without_leaking_tags() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"<think>\\nNeed\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[{\"delta\":{\"content\":\" context.</think>\\n\\npong\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_minimax\",\"created\":123,\"model\":\"MiniMax-M2.7\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6,\"total_tokens\":10,\"completion_tokens_details\":{\"reasoning_tokens\":3}}}\n\n",
        ])
        .await;

        assert!(output.contains("event: response.reasoning_summary_text.delta"));
        assert!(output.contains("Need context."));
        assert!(output.contains("\"text\":\"pong\""));
        assert!(output.contains("\"reasoning_tokens\":3"));
        assert!(!output.contains("<think>"));
        assert!(!output.contains("</think>"));
        assert!(output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn converts_tool_call_chat_sse_to_responses_sse() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.function_call_arguments.delta"));
        assert!(output.contains("event: response.function_call_arguments.done"));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"call_id\":\"call_1\""));
    }

    #[tokio::test]
    async fn restores_custom_tool_input_stream_events() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{ "type": "custom", "name": "exec" }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_custom\",\"type\":\"function\",\"function\":{\"name\":\"exec\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"input\\\":\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_custom\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls -la\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("event: response.custom_tool_call_input.delta"));
        assert!(output.contains("event: response.custom_tool_call_input.done"));
        assert!(!output.contains("event: response.function_call_arguments.delta"));
        assert!(!output.contains("event: response.function_call_arguments.done"));
        assert!(output.contains("\"id\":\"ctc_call_custom\""));
        assert!(output.contains("\"type\":\"custom_tool_call\""));
        assert!(output.contains("\"name\":\"exec\""));
        assert!(output.contains("\"input\":\"ls -la\""));
    }

    #[tokio::test]
    async fn canonicalizes_streamed_tool_call_arguments_on_done_events() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{ \\\"b\\\": 2,\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_args\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" \\\"a\\\": 1 }\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains(r#""arguments":"{\"a\":1,\"b\":2}""#));
    }

    #[tokio::test]
    async fn preserves_reasoning_content_on_streamed_tool_call_items() {
        let output = collect(vec![
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Need file.\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_tool_reasoning\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.output_item.done"));
        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"reasoning_content\":\"Need file.\""));
    }

    #[tokio::test]
    async fn restores_namespace_on_streamed_tool_call_items() {
        let request = json!({
            "model": "gpt-5.4",
            "input": [{
                "type": "tool_search_output",
                "call_id": "call_tool_search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__codex_apps__gmail",
                    "tools": [{
                        "type": "function",
                        "name": "_search_emails",
                        "description": "Search Gmail.",
                        "parameters": {"type": "object"}
                    }]
                }]
            }]
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_gmail\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_gmail\",\"type\":\"function\",\"function\":{\"name\":\"mcp__codex_apps__gmail___search_emails\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_gmail\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\":\\\"in:inbox\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("\"type\":\"function_call\""));
        assert!(output.contains("\"namespace\":\"mcp__codex_apps__gmail\""));
        assert!(output.contains("\"name\":\"_search_emails\""));
        assert!(output.contains(r#""arguments":"{\"query\":\"in:inbox\"}""#));
    }

    #[tokio::test]
    async fn restores_tool_search_on_streamed_tool_call_items() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": "Search for Gmail tools."
        });
        let context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let output = collect_with_context(
            vec![
                "data: {\"id\":\"chatcmpl_tool_search\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_tool_search_1\",\"type\":\"function\",\"function\":{\"name\":\"tool_search\"}}]}}]}\n\n",
                "data: {\"id\":\"chatcmpl_tool_search\",\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\":\\\"Gmail search emails\\\",\\\"limit\\\":10}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            context,
        )
        .await;

        assert!(output.contains("\"type\":\"tool_search_call\""));
        assert!(output.contains("\"execution\":\"client\""));
        assert!(output.contains("\"call_id\":\"call_tool_search_1\""));
        assert!(output.contains("\"query\":\"Gmail search emails\""));
    }

    #[tokio::test]
    async fn stream_error_emits_failed_without_completed() {
        let upstream = stream::iter(vec![Err::<Bytes, std::io::Error>(std::io::Error::other(
            "boom",
        ))]);
        let converted = create_responses_sse_stream_from_chat(upstream);
        let bytes: Vec<Bytes> = converted.map(|item| item.unwrap()).collect().await;
        let output = String::from_utf8(bytes.concat()).unwrap();

        assert!(output.contains("event: response.failed"));
        assert!(!output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn chat_sse_error_event_emits_failed_without_completed() {
        let output = collect(vec![
            "event: error\ndata: {\"error\":{\"message\":\"bad request\",\"type\":\"invalid_request_error\"}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("bad request"));
        assert!(output.contains("invalid_request_error"));
        assert!(!output.contains("event: response.completed"));
    }

    #[tokio::test]
    async fn chat_sse_data_only_error_emits_failed_without_completed() {
        let output = collect(vec![
            "data: {\"error\":{\"message\":\"quota exceeded\",\"code\":\"rate_limit_exceeded\"}}\n\n",
            "data: [DONE]\n\n",
        ])
        .await;

        assert!(output.contains("event: response.failed"));
        assert!(output.contains("quota exceeded"));
        assert!(output.contains("rate_limit_exceeded"));
        assert!(!output.contains("event: response.completed"));
    }
}
