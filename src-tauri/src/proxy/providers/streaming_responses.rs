//! OpenAI Responses API 流式转换模块
//!
//! 实现 Responses API SSE → Anthropic SSE 格式转换。
//!
//! Responses API 使用命名事件 (named events) 的生命周期模型：
//! response.created → output_item.added → content_part.added →
//! output_text.delta → content_part.done → output_item.done → response.completed
//!
//! 与 Chat Completions 的 delta chunk 模型完全不同，需要独立的状态机处理。

use super::reasoning_bridge::{encode_openai_reasoning_item, reasoning_summary_text};
use super::transform_responses::{
    build_anthropic_usage_from_responses, map_responses_stop_reason,
    merge_web_search_result_metadata, responses_to_anthropic_with_web_search_options,
    sanitize_anthropic_tool_use_input_json, text_with_url_citations, web_search_action_input,
    web_search_max_uses_exceeded_error, web_search_results_from_action,
    web_search_results_from_output_item, web_search_tool_result_error,
};
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

#[inline]
fn response_object_from_event(data: &Value) -> &Value {
    data.get("response").unwrap_or(data)
}

fn anthropic_sse(event_name: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

fn responses_error_details(data: &Value, fallback: &str) -> (String, String) {
    let response = response_object_from_event(data);
    let error = response.get("error").unwrap_or(response);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string();
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or("upstream_error")
        .to_string();
    (message, error_type)
}

fn anthropic_error_sse(message: &str, error_type: &str) -> Bytes {
    anthropic_sse(
        "error",
        &json!({
            "type": "error",
            "error": {"type": error_type, "message": message}
        }),
    )
}

fn anthropic_ping_sse() -> Bytes {
    anthropic_sse("ping", &json!({"type": "ping"}))
}

/// Convert a compatible gateway's non-streaming Responses JSON into a complete
/// Anthropic SSE lifecycle. This is used when the client requested streaming but
/// the upstream ignored `stream:true` and returned `application/json`.
fn responses_json_to_anthropic_sse(
    body: Value,
    hosted_web_search_name: Option<&str>,
    max_web_search_uses: Option<u64>,
) -> Vec<Bytes> {
    let message = match responses_to_anthropic_with_web_search_options(
        body,
        hosted_web_search_name,
        max_web_search_uses,
    ) {
        Ok(message) => message,
        Err(error) => {
            return vec![anthropic_error_sse(
                &error.to_string(),
                "response_transform_error",
            )]
        }
    };

    let usage = message.get("usage").cloned().unwrap_or_else(|| json!({}));
    let mut start_usage = usage.clone();
    start_usage["output_tokens"] = json!(0);
    let mut events = vec![anthropic_sse(
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": message.get("id").cloned().unwrap_or_else(|| json!("")),
                "type": "message",
                "role": "assistant",
                "model": message.get("model").cloned().unwrap_or_else(|| json!("")),
                "usage": start_usage
            }
        }),
    )];

    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for (index, block) in content.iter().enumerate() {
            let index = index as u64;
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}}),
                    ));
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(anthropic_sse(
                                "content_block_delta",
                                &json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":text}}),
                            ));
                        }
                    }
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                Some("tool_use") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({
                            "type":"content_block_start",
                            "index":index,
                            "content_block":{
                                "type":"tool_use",
                                "id":block.get("id").cloned().unwrap_or_else(|| json!("")),
                                "name":block.get("name").cloned().unwrap_or_else(|| json!("")),
                                "input":{}
                            }
                        }),
                    ));
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    events.push(anthropic_sse(
                        "content_block_delta",
                        &json!({
                            "type":"content_block_delta",
                            "index":index,
                            "delta":{"type":"input_json_delta","partial_json":serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())}
                        }),
                    ));
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                Some("server_tool_use") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({
                            "type":"content_block_start",
                            "index":index,
                            "content_block":{
                                "type":"server_tool_use",
                                "id":block.get("id").cloned().unwrap_or_else(|| json!("")),
                                "name":block.get("name").cloned().unwrap_or_else(|| json!("web_search")),
                                "input":{},
                                "caller":{"type":"direct"}
                            }
                        }),
                    ));
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    events.push(anthropic_sse(
                        "content_block_delta",
                        &json!({
                            "type":"content_block_delta",
                            "index":index,
                            "delta":{
                                "type":"input_json_delta",
                                "partial_json":serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                            }
                        }),
                    ));
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                Some("web_search_tool_result") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({
                            "type":"content_block_start",
                            "index":index,
                            "content_block":block
                        }),
                    ));
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                Some("thinking") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({"type":"content_block_start","index":index,"content_block":{"type":"thinking","thinking":""}}),
                    ));
                    if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                        if !thinking.is_empty() {
                            events.push(anthropic_sse(
                                "content_block_delta",
                                &json!({"type":"content_block_delta","index":index,"delta":{"type":"thinking_delta","thinking":thinking}}),
                            ));
                        }
                    }
                    if let Some(signature) = block.get("signature").and_then(Value::as_str) {
                        if !signature.is_empty() {
                            events.push(anthropic_sse(
                                "content_block_delta",
                                &json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":signature}}),
                            ));
                        }
                    }
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                Some("redacted_thinking") => {
                    events.push(anthropic_sse(
                        "content_block_start",
                        &json!({"type":"content_block_start","index":index,"content_block":block}),
                    ));
                    events.push(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                _ => {}
            }
        }
    }

    events.push(anthropic_sse(
        "message_delta",
        &json!({
            "type":"message_delta",
            "delta":{
                "stop_reason":message.get("stop_reason").cloned().unwrap_or(Value::Null),
                "stop_sequence":null
            },
            "usage":usage
        }),
    ));
    events.push(anthropic_sse(
        "message_stop",
        &json!({"type":"message_stop"}),
    ));
    events
}

#[inline]
fn content_part_key(data: &Value) -> Option<String> {
    if let (Some(item_id), Some(content_index)) = (
        data.get("item_id").and_then(|v| v.as_str()),
        data.get("content_index").and_then(|v| v.as_u64()),
    ) {
        return Some(format!("part:{item_id}:{content_index}"));
    }
    if let (Some(output_index), Some(content_index)) = (
        data.get("output_index").and_then(|v| v.as_u64()),
        data.get("content_index").and_then(|v| v.as_u64()),
    ) {
        return Some(format!("part:out:{output_index}:{content_index}"));
    }
    None
}

#[derive(Default)]
struct StreamedTextPart {
    text: String,
    output_keys: Vec<(u64, u64)>,
    item_keys: Vec<(String, u64)>,
    discarded: bool,
}

#[derive(Default)]
struct StreamedTextState {
    parts: Vec<StreamedTextPart>,
    by_output_part: HashMap<(u64, u64), usize>,
    by_item_part: HashMap<(String, u64), usize>,
    active_keyed_part: Option<usize>,
    unkeyed: String,
    unkeyed_follows_keyed: bool,
}

impl StreamedTextState {
    fn output_part_index(&self, key: (u64, u64)) -> Option<usize> {
        self.by_output_part
            .get(&key)
            .copied()
            .filter(|index| self.parts.get(*index).is_some_and(|part| !part.discarded))
    }

    fn item_part_index(&self, key: &(String, u64)) -> Option<usize> {
        self.by_item_part
            .get(key)
            .copied()
            .filter(|index| self.parts.get(*index).is_some_and(|part| !part.discarded))
    }

    fn key_pair_conflicts(
        &self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> bool {
        let output_index = output_key.and_then(|key| self.output_part_index(key));
        let item_index = item_key.and_then(|key| self.item_part_index(key));
        if let (Some(output_index), Some(item_key)) = (output_index, item_key) {
            let part = &self.parts[output_index];
            if !part.item_keys.is_empty() && !part.item_keys.contains(item_key) {
                return true;
            }
        }
        if let (Some(item_index), Some(output_key)) = (item_index, output_key) {
            let part = &self.parts[item_index];
            if !part.output_keys.is_empty() && !part.output_keys.contains(&output_key) {
                return true;
            }
        }
        false
    }

    fn push_part(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
    ) -> usize {
        let index = self.parts.len();
        let mut part = StreamedTextPart::default();
        if let Some(key) = output_key {
            part.output_keys.push(key);
            self.by_output_part.insert(key, index);
        }
        if let Some(key) = item_key {
            part.item_keys.push(key.clone());
            self.by_item_part.insert(key, index);
        }
        self.parts.push(part);
        index
    }

    fn bind_keys(
        &mut self,
        index: usize,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
    ) {
        let Some(part) = self.parts.get_mut(index).filter(|part| !part.discarded) else {
            return;
        };
        if let Some(key) = output_key {
            if !part.output_keys.contains(&key) {
                part.output_keys.push(key);
            }
            self.by_output_part.insert(key, index);
        }
        if let Some(key) = item_key {
            if !part.item_keys.contains(&key) {
                part.item_keys.push(key.clone());
            }
            self.by_item_part.insert(key, index);
        }
    }

    fn merge_part_indices(&mut self, first: usize, second: usize) -> usize {
        if first == second {
            return first;
        }
        // Parts are created by their first delta, so index order is also delta
        // arrival order. Distinct pre-alias delta streams are additive even when
        // their payload text happens to be identical.
        let (keep, discard) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        let discarded = std::mem::take(&mut self.parts[discard]);
        self.parts[discard].discarded = true;
        self.parts[keep].text.push_str(&discarded.text);
        for key in discarded.output_keys {
            if !self.parts[keep].output_keys.contains(&key) {
                self.parts[keep].output_keys.push(key);
            }
            self.by_output_part.insert(key, keep);
        }
        for key in discarded.item_keys {
            if !self.parts[keep].item_keys.contains(&key) {
                self.parts[keep].item_keys.push(key.clone());
            }
            self.by_item_part.insert(key, keep);
        }
        if self.active_keyed_part == Some(discard) {
            self.active_keyed_part = Some(keep);
        }
        keep
    }

    fn existing_keyed_part_index(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> Option<usize> {
        if self.key_pair_conflicts(output_key, item_key) {
            return None;
        }
        let output_index = output_key.and_then(|key| self.output_part_index(key));
        let item_index = item_key.and_then(|key| self.item_part_index(key));
        match (output_index, item_index) {
            (Some(output), Some(item)) => Some(self.merge_part_indices(output, item)),
            (Some(index), None) | (None, Some(index)) => Some(index),
            (None, None) => None,
        }
    }

    fn resolve_keyed_part(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
    ) -> Option<usize> {
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return None;
        }
        let index = self
            .existing_keyed_part_index(output_key, item_key.as_ref())
            .unwrap_or_else(|| self.push_part(output_key, item_key.clone()));
        self.bind_keys(index, output_key, item_key);
        Some(index)
    }

    fn has_part_matching_terminal(
        &self,
        full_text: &str,
        output_index: Option<u64>,
        item_id: Option<&str>,
        content_index: u64,
    ) -> bool {
        output_index.is_some_and(|index| self.output_part_index((index, content_index)).is_some())
            || item_id.is_some_and(|id| {
                self.item_part_index(&(id.to_string(), content_index))
                    .is_some()
            })
            || (!self.unkeyed.is_empty()
                && (self.unkeyed.starts_with(full_text) || full_text.starts_with(&self.unkeyed)))
    }

    fn record_delta(&mut self, data: &Value, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let content_index = data.get("content_index").and_then(Value::as_u64);
        let output_key = data
            .get("output_index")
            .and_then(Value::as_u64)
            .zip(content_index);
        let item_key = data
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .zip(content_index);
        if output_key.is_some() || item_key.is_some() {
            let Some(index) = self.resolve_keyed_part(output_key, item_key) else {
                return;
            };
            self.parts[index].text.push_str(delta);
            self.active_keyed_part = Some(index);
            return;
        }
        if self.unkeyed.is_empty() {
            self.unkeyed_follows_keyed = self.active_keyed_part.is_some();
        }
        self.unkeyed.push_str(delta);
    }

    fn finish_part(&mut self, data: &Value) {
        let content_index = data.get("content_index").and_then(Value::as_u64);
        let output_key = data
            .get("output_index")
            .and_then(Value::as_u64)
            .zip(content_index);
        let item_key = data
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .zip(content_index);
        if output_key.is_none() && item_key.is_none() {
            self.active_keyed_part = None;
            return;
        }
        let finished = self.existing_keyed_part_index(output_key, item_key.as_ref());
        if finished.is_some() && finished == self.active_keyed_part {
            self.active_keyed_part = None;
        }
    }

    fn missing_suffix(
        &mut self,
        full_text: &str,
        output_index: Option<u64>,
        item_id: Option<&str>,
        content_index: u64,
    ) -> String {
        let output_key = output_index.map(|index| (index, content_index));
        let item_key = item_id.map(|id| (id.to_string(), content_index));
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return String::new();
        }
        let keyed_index = self.existing_keyed_part_index(output_key, item_key.as_ref());
        let emitted = keyed_index.map(|index| self.parts[index].text.clone());

        let missing = if !self.unkeyed.is_empty() {
            let unkeyed = self.unkeyed.clone();
            let combined = emitted.as_deref().map(|keyed| {
                if self.unkeyed_follows_keyed {
                    format!("{keyed}{unkeyed}")
                } else {
                    format!("{unkeyed}{keyed}")
                }
            });
            if let Some(suffix) = combined
                .as_deref()
                .and_then(|candidate| full_text.strip_prefix(candidate))
            {
                self.unkeyed.clear();
                self.unkeyed_follows_keyed = false;
                suffix.to_string()
            } else if self.unkeyed_follows_keyed
                && emitted.as_deref().is_some_and(|keyed| {
                    keyed.starts_with(full_text) || full_text.starts_with(keyed)
                })
            {
                emitted
                    .as_deref()
                    .and_then(|keyed| full_text.strip_prefix(keyed))
                    .unwrap_or_default()
                    .to_string()
            } else if combined
                .as_deref()
                .is_some_and(|candidate| candidate.starts_with(full_text))
            {
                if let Some(remaining) = unkeyed.strip_prefix(full_text) {
                    self.unkeyed = remaining.to_string();
                } else {
                    self.unkeyed.clear();
                    self.unkeyed_follows_keyed = false;
                }
                String::new()
            } else if let Some(remaining) = unkeyed.strip_prefix(full_text) {
                self.unkeyed = remaining.to_string();
                String::new()
            } else if let Some(suffix) = full_text.strip_prefix(&unkeyed) {
                self.unkeyed.clear();
                self.unkeyed_follows_keyed = false;
                if emitted.as_deref().is_some_and(|keyed| suffix == keyed) {
                    String::new()
                } else {
                    suffix.to_string()
                }
            } else if let Some(emitted) = emitted.as_deref() {
                if let Some(suffix) = full_text.strip_prefix(emitted) {
                    suffix.to_string()
                } else if emitted.starts_with(full_text) {
                    String::new()
                } else {
                    log::warn!(
                        "[Claude/Responses] Terminal text did not extend the streamed text; avoiding duplicate replay"
                    );
                    String::new()
                }
            } else {
                log::warn!(
                    "[Claude/Responses] Could not correlate terminal text with an unkeyed streamed delta; avoiding duplicate replay"
                );
                String::new()
            }
        } else if let Some(emitted) = emitted.as_deref() {
            if let Some(suffix) = full_text.strip_prefix(emitted) {
                suffix.to_string()
            } else if emitted.starts_with(full_text) {
                String::new()
            } else {
                log::warn!(
                    "[Claude/Responses] Terminal text did not extend the streamed text; avoiding duplicate replay"
                );
                String::new()
            }
        } else {
            full_text.to_string()
        };

        if output_key.is_some() || item_key.is_some() {
            let index = keyed_index.unwrap_or_else(|| self.push_part(output_key, item_key.clone()));
            self.bind_keys(index, output_key, item_key);
            self.parts[index].text = full_text.to_string();
            if self.active_keyed_part == Some(index) {
                self.active_keyed_part = None;
            }
        }
        missing
    }
}

#[derive(Clone)]
struct BufferedCitationAnnotation {
    value: Value,
    observed_text_end: usize,
    emitted: bool,
}

#[derive(Clone, Default)]
struct BufferedCitationPart {
    text: String,
    annotations: Vec<BufferedCitationAnnotation>,
    output_keys: Vec<(u64, u64)>,
    item_keys: Vec<(String, u64)>,
    emitted_bytes: usize,
    discarded: bool,
    originated_unkeyed: bool,
    received_delta: bool,
}

type BufferedCitationKeys = (Option<(u64, u64)>, Option<(String, u64)>);

enum EmittedUnkeyedTerminalMatch {
    NoMatch,
    FullyEmitted(Vec<Value>),
    MissingSuffix {
        suffix: String,
        emitted_annotations: Vec<Value>,
    },
}

#[derive(Default)]
struct BufferedCitationTextState {
    parts: Vec<BufferedCitationPart>,
    open_part: Option<usize>,
    last_unkeyed_part: Option<usize>,
    emitted_unkeyed_history: String,
    emitted_unkeyed_annotations: Vec<BufferedCitationAnnotation>,
    emitted_output_parts: HashSet<(u64, u64)>,
    emitted_item_parts: HashSet<(String, u64)>,
}

impl BufferedCitationTextState {
    fn keys(data: &Value) -> BufferedCitationKeys {
        let content_index = data.get("content_index").and_then(Value::as_u64);
        let output_key = data
            .get("output_index")
            .and_then(Value::as_u64)
            .zip(content_index);
        let item_key = data
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .zip(content_index);
        (output_key, item_key)
    }

    fn append_annotation(part: &mut BufferedCitationPart, annotation: &Value) {
        let observed_text_end = part.text.chars().count();
        if !part.annotations.iter().any(|buffered| {
            buffered.value == *annotation && buffered.observed_text_end == observed_text_end
        }) {
            part.annotations.push(BufferedCitationAnnotation {
                value: annotation.clone(),
                observed_text_end,
                emitted: false,
            });
        }
    }

    fn pending_annotation_values(part: &BufferedCitationPart) -> Vec<Value> {
        let mut values = Vec::new();
        for annotation in part
            .annotations
            .iter()
            .filter(|annotation| !annotation.emitted)
        {
            if !values.contains(&annotation.value) {
                values.push(annotation.value.clone());
            }
        }
        values
    }

    fn has_pending_output(part: &BufferedCitationPart) -> bool {
        part.emitted_bytes < part.text.len()
            || part
                .annotations
                .iter()
                .any(|annotation| !annotation.emitted)
    }

    fn push_part(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
        originated_unkeyed: bool,
    ) -> usize {
        let mut part = BufferedCitationPart {
            originated_unkeyed,
            ..BufferedCitationPart::default()
        };
        if let Some(key) = output_key {
            part.output_keys.push(key);
        }
        if let Some(key) = item_key {
            part.item_keys.push(key);
        }
        self.parts.push(part);
        self.parts.len() - 1
    }

    fn output_part_index(&self, key: (u64, u64)) -> Option<usize> {
        self.parts
            .iter()
            .rposition(|part| !part.discarded && part.output_keys.contains(&key))
    }

    fn item_part_index(&self, key: &(String, u64)) -> Option<usize> {
        self.parts
            .iter()
            .rposition(|part| !part.discarded && part.item_keys.contains(key))
    }

    fn key_pair_conflicts(
        &self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> bool {
        let output_index = output_key.and_then(|key| self.output_part_index(key));
        let item_index = item_key.and_then(|key| self.item_part_index(key));
        if let (Some(output_index), Some(item_key)) = (output_index, item_key) {
            let part = &self.parts[output_index];
            if !part.item_keys.is_empty() && !part.item_keys.contains(item_key) {
                return true;
            }
        }
        if let (Some(item_index), Some(output_key)) = (item_index, output_key) {
            let part = &self.parts[item_index];
            if !part.output_keys.is_empty() && !part.output_keys.contains(&output_key) {
                return true;
            }
        }
        false
    }

    fn mark_part_emitted(&mut self, index: usize) {
        if self.parts.get(index).is_none_or(|part| part.discarded) {
            return;
        }
        let output_keys = self.parts[index].output_keys.clone();
        let item_keys = self.parts[index].item_keys.clone();
        self.parts[index].emitted_bytes = self.parts[index].text.len();
        for annotation in &mut self.parts[index].annotations {
            annotation.emitted = true;
        }
        self.emitted_output_parts.extend(output_keys);
        self.emitted_item_parts.extend(item_keys);
        if self.open_part == Some(index) {
            self.open_part = None;
            if self.parts[index].originated_unkeyed {
                self.last_unkeyed_part = Some(index);
            }
        }
    }

    fn bind_keys(
        &mut self,
        index: usize,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
    ) {
        let Some(part) = self.parts.get_mut(index).filter(|part| !part.discarded) else {
            return;
        };
        if let Some(key) = output_key {
            if !part.output_keys.contains(&key) {
                part.output_keys.push(key);
            }
            if part.emitted_bytes > 0
                || part.annotations.iter().any(|annotation| annotation.emitted)
            {
                self.emitted_output_parts.insert(key);
            }
        }
        if let Some(key) = item_key {
            if !part.item_keys.contains(&key) {
                part.item_keys.push(key.clone());
            }
            if part.emitted_bytes > 0
                || part.annotations.iter().any(|annotation| annotation.emitted)
            {
                self.emitted_item_parts.insert(key);
            }
        }
    }

    fn merge_part_indices(&mut self, first: usize, second: usize) -> usize {
        if first == second {
            return first;
        }
        let (keep, discard) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        let discarded = self.parts[discard].clone();
        let kept_text = self.parts[keep].text.clone();
        let discarded_text = discarded.text.clone();
        let kept_emitted =
            kept_text[..self.parts[keep].emitted_bytes.min(kept_text.len())].to_string();
        let discarded_emitted =
            discarded_text[..discarded.emitted_bytes.min(discarded_text.len())].to_string();
        let additive_histories = self.parts[keep].received_delta && discarded.received_delta;
        let concatenated = !kept_text.is_empty()
            && !discarded_text.is_empty()
            && (additive_histories
                || (!discarded_text.contains(kept_text.as_str())
                    && !kept_text.contains(discarded_text.as_str())));
        let merged_text = if additive_histories {
            format!("{kept_text}{discarded_text}")
        } else if kept_text.is_empty() || discarded_text.contains(kept_text.as_str()) {
            discarded_text.clone()
        } else if discarded_text.is_empty() || kept_text.contains(discarded_text.as_str()) {
            kept_text.clone()
        } else {
            format!("{kept_text}{discarded_text}")
        };
        let mut emitted_bytes = [kept_emitted.as_str(), discarded_emitted.as_str()]
            .into_iter()
            .filter(|prefix| merged_text.starts_with(prefix))
            .map(str::len)
            .max()
            .unwrap_or_default();
        if concatenated && self.parts[keep].emitted_bytes == kept_text.len() {
            let concatenated_emitted = format!("{kept_text}{discarded_emitted}");
            if merged_text.starts_with(&concatenated_emitted) {
                emitted_bytes = emitted_bytes.max(concatenated_emitted.len());
            }
        }
        let merged_text_end = merged_text.chars().count();

        {
            let kept = &mut self.parts[keep];
            kept.text = merged_text;
            kept.emitted_bytes = emitted_bytes;
            for annotation in discarded.annotations {
                if let Some(existing) = kept
                    .annotations
                    .iter_mut()
                    .find(|candidate| candidate.value == annotation.value)
                {
                    existing.emitted |= annotation.emitted;
                } else {
                    kept.annotations.push(BufferedCitationAnnotation {
                        value: annotation.value,
                        observed_text_end: merged_text_end,
                        emitted: annotation.emitted,
                    });
                }
            }
            for key in discarded.output_keys {
                if !kept.output_keys.contains(&key) {
                    kept.output_keys.push(key);
                }
            }
            for key in discarded.item_keys {
                if !kept.item_keys.contains(&key) {
                    kept.item_keys.push(key);
                }
            }
            kept.originated_unkeyed |= discarded.originated_unkeyed;
            kept.received_delta |= discarded.received_delta;
        }
        self.parts[discard].discarded = true;
        self.parts[discard].emitted_bytes = self.parts[discard].text.len();
        if self.open_part == Some(discard) {
            self.open_part = Some(keep);
        }
        if self.last_unkeyed_part == Some(discard) {
            self.last_unkeyed_part = Some(keep);
        }
        if self.parts[keep].emitted_bytes > 0 {
            self.emitted_output_parts
                .extend(self.parts[keep].output_keys.iter().copied());
            self.emitted_item_parts
                .extend(self.parts[keep].item_keys.iter().cloned());
        }
        keep
    }

    fn existing_keyed_part_index(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> Option<usize> {
        if self.key_pair_conflicts(output_key, item_key) {
            return None;
        }
        let output_index = output_key.and_then(|key| self.output_part_index(key));
        let item_index = item_key.and_then(|key| self.item_part_index(key));
        match (output_index, item_index) {
            (Some(output), Some(item)) => Some(self.merge_part_indices(output, item)),
            (Some(index), None) | (None, Some(index)) => Some(index),
            (None, None) => None,
        }
    }

    fn snapshot_matches(part: &BufferedCitationPart, text: &str) -> bool {
        part.text.is_empty() || part.text.starts_with(text) || text.starts_with(&part.text)
    }

    fn keys_allow_open_part_adoption(
        part: &BufferedCitationPart,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> bool {
        let has_keys = !part.output_keys.is_empty() || !part.item_keys.is_empty();
        if !has_keys {
            return part.originated_unkeyed;
        }
        let shares_key = output_key.is_some_and(|key| part.output_keys.contains(&key))
            || item_key.is_some_and(|key| part.item_keys.contains(key));
        let output_conflicts = output_key
            .is_some_and(|key| !part.output_keys.is_empty() && !part.output_keys.contains(&key));
        let item_conflicts =
            item_key.is_some_and(|key| !part.item_keys.is_empty() && !part.item_keys.contains(key));
        shares_key && !output_conflicts && !item_conflicts
    }

    fn resolve_keyed_part(
        &mut self,
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
        adopt_open_part: bool,
        snapshot: Option<&str>,
    ) -> Option<usize> {
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return None;
        }
        let existing = self.existing_keyed_part_index(output_key, item_key.as_ref());
        let index = existing
            .or_else(|| {
                adopt_open_part
                    .then_some(self.open_part)
                    .flatten()
                    .filter(|index| {
                        self.parts.get(*index).is_some_and(|part| {
                            !part.discarded
                                && Self::keys_allow_open_part_adoption(
                                    part,
                                    output_key,
                                    item_key.as_ref(),
                                )
                        })
                    })
            })
            .or_else(|| {
                snapshot.and_then(|text| {
                    self.last_unkeyed_part.filter(|index| {
                        self.parts.get(*index).is_some_and(|part| {
                            !part.discarded
                                && Self::has_pending_output(part)
                                && part.originated_unkeyed
                                && Self::keys_allow_open_part_adoption(
                                    part,
                                    output_key,
                                    item_key.as_ref(),
                                )
                                && Self::snapshot_matches(part, text)
                        })
                    })
                })
            })
            .unwrap_or_else(|| self.push_part(output_key, item_key.clone(), false));
        self.bind_keys(index, output_key, item_key);
        Some(index)
    }

    fn close_open_part(&mut self) {
        let Some(index) = self.open_part.take() else {
            return;
        };
        if self
            .parts
            .get(index)
            .is_some_and(|part| !part.discarded && part.originated_unkeyed)
        {
            self.last_unkeyed_part = Some(index);
        }
    }

    fn finish_event_part(&mut self, data: &Value) {
        let (output_key, item_key) = Self::keys(data);
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return;
        }
        if output_key.is_none() && item_key.is_none() {
            self.close_open_part();
            return;
        }
        let finished = self.existing_keyed_part_index(output_key, item_key.as_ref());
        if finished.is_some() && finished == self.open_part {
            self.close_open_part();
        }
    }

    fn start_unkeyed_part(&mut self, value: &Value) {
        self.close_open_part();
        let index = self.push_part(None, None, true);
        self.open_part = Some(index);
        if let Some(text) = value
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            Self::merge_snapshot(&mut self.parts[index], text);
        }
        if let Some(annotations) = value.get("annotations").and_then(Value::as_array) {
            for annotation in annotations {
                Self::append_annotation(&mut self.parts[index], annotation);
            }
        }
    }

    fn merge_snapshot(part: &mut BufferedCitationPart, text: &str) {
        if text.starts_with(&part.text) {
            part.text = text.to_string();
            return;
        }
        if part.text.starts_with(text) {
            return;
        }
        if part.emitted_bytes == 0 {
            part.text = text.to_string();
            return;
        }
        let emitted_prefix = &part.text[..part.emitted_bytes.min(part.text.len())];
        if text.starts_with(emitted_prefix) && text.len() > part.text.len() {
            part.text = text.to_string();
        }
    }

    fn finish_unkeyed_text(&mut self, text: &str) -> bool {
        if let Some(index) = self.open_part {
            if self.parts.get(index).is_some_and(|part| part.text == text) {
                self.close_open_part();
                return true;
            }
            if self.emitted_unkeyed_history == text {
                return false;
            }
            let cumulative_continuation = if self.emitted_unkeyed_history.is_empty() {
                None
            } else {
                text.strip_prefix(&self.emitted_unkeyed_history)
                    .filter(|continuation| !continuation.is_empty())
            };
            if let Some(continuation) = cumulative_continuation {
                let continuation_matches = self
                    .parts
                    .get(index)
                    .is_some_and(|part| Self::snapshot_matches(part, continuation));
                if continuation_matches {
                    Self::merge_snapshot(&mut self.parts[index], continuation);
                    self.close_open_part();
                    return true;
                }
                return false;
            }
            let previous_matches = self.last_unkeyed_part.is_some_and(|previous| {
                previous != index
                    && self.parts.get(previous).is_some_and(|part| {
                        !part.discarded && part.originated_unkeyed && part.text == text
                    })
            });
            let active_matches = self
                .parts
                .get(index)
                .is_some_and(|part| Self::snapshot_matches(part, text));
            if previous_matches && (!active_matches || self.parts[index].text.is_empty()) {
                return false;
            }
            if !active_matches && !self.emitted_unkeyed_history.is_empty() {
                return false;
            }
            Self::merge_snapshot(&mut self.parts[index], text);
            self.close_open_part();
            return true;
        }

        if let Some(index) = self.last_unkeyed_part.filter(|index| {
            self.parts
                .get(*index)
                .is_some_and(|part| !part.discarded && part.originated_unkeyed)
        }) {
            if self.parts[index].text.contains(text) {
                return true;
            }
            if text.starts_with(&self.parts[index].text) {
                Self::merge_snapshot(&mut self.parts[index], text);
                return true;
            }
        }

        let index = self.push_part(None, None, true);
        self.parts[index].text = text.to_string();
        self.last_unkeyed_part = Some(index);
        true
    }

    fn finish_unkeyed_part(&mut self, value: &Value) -> bool {
        if let Some(text) = value
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            if !self.finish_unkeyed_text(text) {
                return false;
            }
        } else {
            self.close_open_part();
        }
        if let Some(annotations) = value.get("annotations").and_then(Value::as_array) {
            if let Some(index) = self.last_unkeyed_part {
                for annotation in annotations {
                    Self::append_annotation(&mut self.parts[index], annotation);
                }
            }
        }
        true
    }

    fn record_text(&mut self, data: &Value, text: &str) -> bool {
        let (output_key, item_key) = Self::keys(data);
        let unkeyed = output_key.is_none() && item_key.is_none();
        if unkeyed {
            return self.finish_unkeyed_text(text);
        }
        let Some(index) = self.resolve_keyed_part(output_key, item_key, true, Some(text)) else {
            return false;
        };
        Self::merge_snapshot(&mut self.parts[index], text);
        if self.open_part == Some(index) {
            self.close_open_part();
        }
        true
    }

    fn merge_part_value(part: &mut BufferedCitationPart, value: &Value) {
        if let Some(text) = value
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            Self::merge_snapshot(part, text);
        }
        if let Some(annotations) = value.get("annotations").and_then(Value::as_array) {
            for annotation in annotations {
                Self::append_annotation(part, annotation);
            }
        }
    }

    fn record_part(&mut self, data: &Value, part: &Value, finalize_unkeyed: bool) -> bool {
        let (output_key, item_key) = Self::keys(data);
        let unkeyed = output_key.is_none() && item_key.is_none();
        if unkeyed {
            if finalize_unkeyed {
                return self.finish_unkeyed_part(part);
            } else {
                self.start_unkeyed_part(part);
            }
            return true;
        }

        let snapshot = part
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty());
        let adopt_open_part = finalize_unkeyed
            || self.open_part.is_some_and(|index| {
                self.parts
                    .get(index)
                    .is_some_and(|candidate| candidate.text.is_empty())
            });
        if !finalize_unkeyed && !adopt_open_part {
            self.close_open_part();
        }
        let Some(index) = self.resolve_keyed_part(output_key, item_key, adopt_open_part, snapshot)
        else {
            return false;
        };
        Self::merge_part_value(&mut self.parts[index], part);
        if finalize_unkeyed {
            if self.open_part == Some(index) {
                self.close_open_part();
            }
        } else {
            self.open_part = Some(index);
        }
        true
    }

    fn record_delta(&mut self, data: &Value, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let (output_key, item_key) = Self::keys(data);
        let unkeyed = output_key.is_none() && item_key.is_none();
        if unkeyed {
            let index = self.open_part.unwrap_or_else(|| {
                let index = self.push_part(None, None, true);
                self.open_part = Some(index);
                index
            });
            self.parts[index].text.push_str(delta);
            self.parts[index].received_delta = true;
            return;
        }
        let Some(index) = self.resolve_keyed_part(output_key, item_key, true, None) else {
            return;
        };
        self.parts[index].text.push_str(delta);
        self.parts[index].received_delta = true;
        if self.open_part != Some(index) {
            self.close_open_part();
            self.open_part = Some(index);
        }
    }

    fn record_annotation(&mut self, data: &Value, annotation: &Value) {
        let (output_key, item_key) = Self::keys(data);
        let unkeyed = output_key.is_none() && item_key.is_none();
        if unkeyed {
            let index = self
                .open_part
                .or(self.last_unkeyed_part)
                .unwrap_or_else(|| {
                    let index = self.push_part(None, None, true);
                    self.open_part = Some(index);
                    index
                });
            Self::append_annotation(&mut self.parts[index], annotation);
            return;
        }
        let Some(index) = self.resolve_keyed_part(output_key, item_key, true, None) else {
            return;
        };
        Self::append_annotation(&mut self.parts[index], annotation);
    }

    fn was_emitted(
        &self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
    ) -> bool {
        output_key.is_some_and(|key| self.emitted_output_parts.contains(&key))
            || item_key.is_some_and(|key| self.emitted_item_parts.contains(key))
    }

    fn mark_emitted(&mut self, output_key: Option<(u64, u64)>, item_key: Option<(String, u64)>) {
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return;
        }
        if let Some(key) = output_key {
            self.emitted_output_parts.insert(key);
        }
        if let Some(key) = item_key.as_ref() {
            self.emitted_item_parts.insert(key.clone());
        }
        if let Some(index) = self.existing_keyed_part_index(output_key, item_key.as_ref()) {
            self.bind_keys(index, output_key, item_key);
            self.mark_part_emitted(index);
        }
    }

    fn collect_annotations(
        &self,
        output_key: Option<(u64, u64)>,
        item_key: Option<&(String, u64)>,
        part: Option<&Value>,
    ) -> Vec<Value> {
        let matching_parts = self
            .parts
            .iter()
            .filter(|buffered| {
                !buffered.discarded
                    && (output_key.is_some_and(|key| buffered.output_keys.contains(&key))
                        || item_key.is_some_and(|key| buffered.item_keys.contains(key)))
            })
            .collect::<Vec<_>>();
        let emitted_annotations = matching_parts
            .iter()
            .flat_map(|buffered| &buffered.annotations)
            .filter(|annotation| annotation.emitted)
            .map(|annotation| &annotation.value)
            .collect::<Vec<_>>();
        let mut annotations = Vec::new();
        let mut append = |candidate: &Value| {
            if !annotations.contains(candidate) {
                annotations.push(candidate.clone());
            }
        };
        if let Some(part_annotations) = part
            .and_then(|part| part.get("annotations"))
            .and_then(Value::as_array)
        {
            for annotation in part_annotations {
                if !emitted_annotations.contains(&annotation) {
                    append(annotation);
                }
            }
        }
        for buffered in matching_parts {
            for annotation in buffered
                .annotations
                .iter()
                .filter(|annotation| !annotation.emitted)
            {
                append(&annotation.value);
            }
        }
        annotations
    }

    fn consume_part_annotations(
        part: &mut BufferedCitationPart,
        consumed_chars: usize,
        consume_all: bool,
    ) -> Vec<Value> {
        let mut consumed = Vec::new();
        let mut remaining = Vec::new();
        for mut annotation in std::mem::take(&mut part.annotations) {
            if consume_all || annotation.observed_text_end <= consumed_chars {
                if !consumed.contains(&annotation.value) {
                    consumed.push(annotation.value);
                }
            } else {
                annotation.observed_text_end -= consumed_chars;
                remaining.push(annotation);
            }
        }
        part.annotations = remaining;
        consumed
    }

    fn consume_emitted_unkeyed_annotations(
        &mut self,
        consumed_chars: usize,
        consume_all: bool,
    ) -> Vec<Value> {
        let mut consumed = Vec::new();
        let mut remaining = Vec::new();
        for mut annotation in std::mem::take(&mut self.emitted_unkeyed_annotations) {
            if consume_all || annotation.observed_text_end <= consumed_chars {
                if !consumed.contains(&annotation.value) {
                    consumed.push(annotation.value);
                }
            } else {
                annotation.observed_text_end -= consumed_chars;
                remaining.push(annotation);
            }
        }
        self.emitted_unkeyed_annotations = remaining;
        consumed
    }

    fn reconcile_unkeyed_terminal_text(
        &mut self,
        terminal_text: &str,
        has_terminal_key: bool,
    ) -> Vec<Value> {
        if !has_terminal_key {
            return Vec::new();
        }

        let Some(index) = self.parts.iter().position(|part| {
            !part.discarded
                && Self::has_pending_output(part)
                && part.originated_unkeyed
                && part.output_keys.is_empty()
                && part.item_keys.is_empty()
                && !part.text.is_empty()
        }) else {
            return Vec::new();
        };
        let unkeyed_text = self.parts[index].text.clone();
        if let Some(remaining) = unkeyed_text.strip_prefix(terminal_text) {
            let consumed_chars = terminal_text.chars().count();
            let consume_all = remaining.is_empty();
            let annotations =
                Self::consume_part_annotations(&mut self.parts[index], consumed_chars, consume_all);
            self.parts[index].text = remaining.to_string();
            if consume_all {
                self.mark_part_emitted(index);
            }
            return annotations;
        }
        if terminal_text.starts_with(&unkeyed_text) {
            let annotations = Self::consume_part_annotations(
                &mut self.parts[index],
                unkeyed_text.chars().count(),
                true,
            );
            self.parts[index].text.clear();
            self.mark_part_emitted(index);
            return annotations;
        }

        log::warn!(
            "[Claude/Responses] Terminal text did not match the next unkeyed buffered part; preferring the terminal snapshot"
        );
        self.mark_part_emitted(index);
        Vec::new()
    }

    fn reconcile_emitted_unkeyed_terminal_text(
        &mut self,
        terminal_text: &str,
    ) -> EmittedUnkeyedTerminalMatch {
        if self.emitted_unkeyed_history.is_empty() {
            return EmittedUnkeyedTerminalMatch::NoMatch;
        }

        let emitted = self.emitted_unkeyed_history.clone();
        if let Some(remaining) = emitted.strip_prefix(terminal_text) {
            let consumed_chars = terminal_text.chars().count();
            let consume_all = remaining.is_empty();
            let emitted_annotations =
                self.consume_emitted_unkeyed_annotations(consumed_chars, consume_all);
            self.emitted_unkeyed_history = remaining.to_string();
            return EmittedUnkeyedTerminalMatch::FullyEmitted(emitted_annotations);
        }
        if let Some(suffix) = terminal_text.strip_prefix(&emitted) {
            let emitted_annotations =
                self.consume_emitted_unkeyed_annotations(emitted.chars().count(), true);
            self.emitted_unkeyed_history.clear();
            return if suffix.is_empty() {
                EmittedUnkeyedTerminalMatch::FullyEmitted(emitted_annotations)
            } else {
                EmittedUnkeyedTerminalMatch::MissingSuffix {
                    suffix: suffix.to_string(),
                    emitted_annotations,
                }
            };
        }

        EmittedUnkeyedTerminalMatch::NoMatch
    }

    fn record_done_event(&mut self, data: &Value) {
        let mut accept_annotations = true;
        if let Some(part) = data.get("part") {
            accept_annotations = self.record_part(data, part, true);
        }
        if let Some(text) = data
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            if accept_annotations {
                accept_annotations = self.record_text(data, text);
            }
        }
        if !accept_annotations {
            return;
        }
        if let Some(annotation) = data.get("annotation") {
            self.record_annotation(data, annotation);
        }
        if let Some(annotations) = data.get("annotations").and_then(Value::as_array) {
            for annotation in annotations {
                self.record_annotation(data, annotation);
            }
        }
        self.finish_event_part(data);
    }

    fn render_pending_parts(&mut self) -> Vec<String> {
        let mut rendered = Vec::new();
        for index in 0..self.parts.len() {
            if let Some(text) = self.render_part_pending(index) {
                rendered.push(text);
            }
        }
        rendered
    }

    fn render_part_pending(&mut self, index: usize) -> Option<String> {
        let part = self
            .parts
            .get(index)
            .filter(|part| !part.discarded && Self::has_pending_output(part))?;
        let emitted_bytes = part.emitted_bytes.min(part.text.len());
        let missing = part.text[emitted_bytes..].to_string();
        let annotations = Self::pending_annotation_values(part);
        let unkeyed = part.output_keys.is_empty() && part.item_keys.is_empty();
        let emitted_unkeyed_annotations = if unkeyed {
            let emitted_chars = part.text[..emitted_bytes].chars().count();
            let missing_chars = missing.chars().count();
            part.annotations
                .iter()
                .filter(|annotation| !annotation.emitted)
                .map(|annotation| {
                    (
                        annotation.value.clone(),
                        annotation
                            .observed_text_end
                            .saturating_sub(emitted_chars)
                            .min(missing_chars),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if unkeyed {
            let history_chars = self.emitted_unkeyed_history.chars().count();
            self.emitted_unkeyed_history.push_str(&missing);
            for (value, relative_end) in emitted_unkeyed_annotations {
                let observed_text_end = history_chars + relative_end;
                if !self.emitted_unkeyed_annotations.iter().any(|annotation| {
                    annotation.value == value && annotation.observed_text_end == observed_text_end
                }) {
                    self.emitted_unkeyed_annotations
                        .push(BufferedCitationAnnotation {
                            value,
                            observed_text_end,
                            emitted: true,
                        });
                }
            }
        }
        self.mark_part_emitted(index);

        if emitted_bytes == 0 && !missing.is_empty() {
            return Some(text_with_url_citations(&missing, &annotations));
        }
        let sources = text_with_url_citations("", &annotations);
        match (missing.is_empty(), sources.is_empty()) {
            (false, false) => Some(format!("{missing}\n\n{sources}")),
            (false, true) => Some(missing),
            (true, false) => Some(sources),
            (true, true) => None,
        }
    }

    fn append_part_annotations(&mut self, index: usize, value: &Value) {
        if let Some(annotations) = value.get("annotations").and_then(Value::as_array) {
            for annotation in annotations {
                Self::append_annotation(&mut self.parts[index], annotation);
            }
        }
    }

    fn remember_rendered_terminal_part(
        &mut self,
        text: &str,
        annotations: &[Value],
        output_key: Option<(u64, u64)>,
        item_key: Option<(String, u64)>,
    ) {
        if output_key.is_none() && item_key.is_none() {
            return;
        }
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return;
        }
        let index = self
            .existing_keyed_part_index(output_key, item_key.as_ref())
            .unwrap_or_else(|| self.push_part(output_key, item_key.clone(), false));
        self.bind_keys(index, output_key, item_key);
        Self::merge_snapshot(&mut self.parts[index], text);
        for annotation in annotations {
            Self::append_annotation(&mut self.parts[index], annotation);
        }
        self.mark_part_emitted(index);
    }

    fn render_message_part(
        &mut self,
        part: &Value,
        output_index: Option<u64>,
        item_id: Option<&str>,
        content_index: u64,
    ) -> Option<String> {
        let output_key = output_index.map(|index| (index, content_index));
        let item_key = item_id.map(|id| (id.to_string(), content_index));
        if self.key_pair_conflicts(output_key, item_key.as_ref()) {
            return None;
        }
        let keyed_index = self.existing_keyed_part_index(output_key, item_key.as_ref());
        let text = part
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())?;
        if let Some(index) = keyed_index.filter(|index| {
            Self::has_pending_output(&self.parts[*index])
                && Self::snapshot_matches(&self.parts[*index], text)
        }) {
            self.bind_keys(index, output_key, item_key.clone());
            Self::merge_snapshot(&mut self.parts[index], text);
            self.append_part_annotations(index, part);
            return self.render_part_pending(index);
        }
        match self.reconcile_emitted_unkeyed_terminal_text(text) {
            EmittedUnkeyedTerminalMatch::FullyEmitted(emitted_annotations) => {
                let annotations = part
                    .get("annotations")
                    .and_then(Value::as_array)
                    .map(|annotations| {
                        annotations
                            .iter()
                            .filter(|annotation| !emitted_annotations.contains(annotation))
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if let Some(index) =
                    keyed_index.filter(|index| Self::has_pending_output(&self.parts[*index]))
                {
                    self.bind_keys(index, output_key, item_key.clone());
                    if let Some(terminal_annotations) =
                        part.get("annotations").and_then(Value::as_array)
                    {
                        for annotation in terminal_annotations {
                            Self::append_annotation(&mut self.parts[index], annotation);
                        }
                    }
                    return self.render_part_pending(index);
                }
                let terminal_annotations = part
                    .get("annotations")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                self.remember_rendered_terminal_part(
                    text,
                    terminal_annotations,
                    output_key,
                    item_key,
                );
                let sources = text_with_url_citations("", &annotations);
                return (!sources.is_empty()).then_some(sources);
            }
            EmittedUnkeyedTerminalMatch::MissingSuffix {
                suffix,
                emitted_annotations,
            } => {
                if let Some(index) = keyed_index {
                    self.bind_keys(index, output_key, item_key.clone());
                    Self::merge_snapshot(&mut self.parts[index], &suffix);
                    if let Some(annotations) = part.get("annotations").and_then(Value::as_array) {
                        for annotation in annotations
                            .iter()
                            .filter(|annotation| !emitted_annotations.contains(annotation))
                        {
                            Self::append_annotation(&mut self.parts[index], annotation);
                        }
                    }
                    return self.render_part_pending(index);
                }
                let mut annotations: Vec<Value> = part
                    .get("annotations")
                    .and_then(Value::as_array)
                    .map(|annotations| {
                        annotations
                            .iter()
                            .filter(|annotation| !emitted_annotations.contains(annotation))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                for annotation in self.reconcile_unkeyed_terminal_text(
                    &suffix,
                    output_key.is_some() || item_key.is_some(),
                ) {
                    if !annotations.contains(&annotation) {
                        annotations.push(annotation);
                    }
                }
                let mut remembered_annotations = part
                    .get("annotations")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for annotation in &annotations {
                    if !remembered_annotations.contains(annotation) {
                        remembered_annotations.push(annotation.clone());
                    }
                }
                self.remember_rendered_terminal_part(
                    text,
                    &remembered_annotations,
                    output_key,
                    item_key,
                );
                let sources = text_with_url_citations("", &annotations);
                return match (suffix.is_empty(), sources.is_empty()) {
                    (false, false) => Some(format!("{suffix}\n\n{sources}")),
                    (false, true) => Some(suffix),
                    (true, false) => Some(sources),
                    (true, true) => None,
                };
            }
            EmittedUnkeyedTerminalMatch::NoMatch => {}
        }
        if keyed_index.is_none() && self.was_emitted(output_key, item_key.as_ref()) {
            return None;
        }
        if let Some(index) = keyed_index {
            self.bind_keys(index, output_key, item_key.clone());
            Self::merge_snapshot(&mut self.parts[index], text);
            self.append_part_annotations(index, part);
            return self.render_part_pending(index);
        }
        let mut annotations = self.collect_annotations(output_key, item_key.as_ref(), Some(part));
        for annotation in
            self.reconcile_unkeyed_terminal_text(text, output_key.is_some() || item_key.is_some())
        {
            if !annotations.contains(&annotation) {
                annotations.push(annotation);
            }
        }
        let rendered = text_with_url_citations(text, &annotations);
        self.remember_rendered_terminal_part(text, &annotations, output_key, item_key);
        Some(rendered)
    }
}

fn missing_message_text_parts(
    item: &Value,
    output_index: Option<u64>,
    streamed_text: &mut StreamedTextState,
    mut buffered_citations: Option<&mut BufferedCitationTextState>,
) -> Vec<String> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return Vec::new();
    }
    let item_id = item.get("id").and_then(Value::as_str);
    let mut missing_parts = Vec::new();
    for (content_index, part) in item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        match part.get("type").and_then(Value::as_str) {
            Some("output_text") => {
                if let Some(buffered) = buffered_citations.as_deref_mut() {
                    let content_index = content_index as u64;
                    if let Some(full_text) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                    {
                        if streamed_text.has_part_matching_terminal(
                            full_text,
                            output_index,
                            item_id,
                            content_index,
                        ) {
                            let missing = streamed_text.missing_suffix(
                                full_text,
                                output_index,
                                item_id,
                                content_index,
                            );
                            let output_key = output_index.map(|index| (index, content_index));
                            let item_key = item_id.map(|id| (id.to_string(), content_index));
                            let annotations = buffered.collect_annotations(
                                output_key,
                                item_key.as_ref(),
                                Some(part),
                            );
                            buffered.mark_emitted(output_key, item_key);
                            let sources = text_with_url_citations("", &annotations);
                            match (missing.is_empty(), sources.is_empty()) {
                                (false, false) => {
                                    missing_parts.push(format!("{missing}\n\n{sources}"));
                                }
                                (false, true) => missing_parts.push(missing),
                                (true, false) => missing_parts.push(sources),
                                (true, true) => {}
                            }
                        } else if let Some(text) =
                            buffered.render_message_part(part, output_index, item_id, content_index)
                        {
                            missing_parts.push(text);
                        }
                    }
                } else if let Some(full_text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    let missing = streamed_text.missing_suffix(
                        full_text,
                        output_index,
                        item_id,
                        content_index as u64,
                    );
                    if !missing.is_empty() {
                        missing_parts.push(missing);
                    }
                }
            }
            Some("refusal") => {
                if let Some(full_text) = part
                    .get("refusal")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    let missing = streamed_text.missing_suffix(
                        full_text,
                        output_index,
                        item_id,
                        content_index as u64,
                    );
                    if !missing.is_empty() {
                        missing_parts.push(missing);
                    }
                }
            }
            _ => {}
        }
    }
    missing_parts
}

fn text_block_events(index: u32, text: &str) -> [Bytes; 3] {
    [
        anthropic_sse(
            "content_block_start",
            &json!({
                "type":"content_block_start",
                "index":index,
                "content_block":{"type":"text","text":""}
            }),
        ),
        anthropic_sse(
            "content_block_delta",
            &json!({
                "type":"content_block_delta",
                "index":index,
                "delta":{"type":"text_delta","text":text}
            }),
        ),
        anthropic_sse(
            "content_block_stop",
            &json!({"type":"content_block_stop","index":index}),
        ),
    ]
}

#[inline]
fn tool_item_key_from_added(data: &Value, item: &Value) -> Option<String> {
    if let Some(item_id) = item.get("id").and_then(|v| v.as_str()) {
        return Some(format!("tool:{item_id}"));
    }
    if let Some(item_id) = data.get("item_id").and_then(|v| v.as_str()) {
        return Some(format!("tool:{item_id}"));
    }
    if let Some(output_index) = data.get("output_index").and_then(|v| v.as_u64()) {
        return Some(format!("tool:out:{output_index}"));
    }
    None
}

#[inline]
fn tool_item_key_from_event(data: &Value) -> Option<String> {
    if let Some(item_id) = data.get("item_id").and_then(|v| v.as_str()) {
        return Some(format!("tool:{item_id}"));
    }
    if let Some(output_index) = data.get("output_index").and_then(|v| v.as_u64()) {
        return Some(format!("tool:out:{output_index}"));
    }
    None
}

fn web_search_item_keys(data: &Value, item: Option<&Value>) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    if let Some(item_id) = item
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| data.get("item_id").and_then(Value::as_str))
        .filter(|item_id| !item_id.is_empty())
    {
        keys.push(format!("web-search:{item_id}"));
    }
    if let Some(output_index) = data.get("output_index").and_then(Value::as_u64) {
        keys.push(format!("web-search:out:{output_index}"));
    }
    keys
}

fn web_search_result_events(index: u32, tool_use_id: &str, content: Value) -> [Bytes; 2] {
    [
        anthropic_sse(
            "content_block_start",
            &json!({
                "type":"content_block_start",
                "index":index,
                "content_block":{
                    "type":"web_search_tool_result",
                    "tool_use_id":tool_use_id,
                    "content":content,
                    "caller":{"type":"direct"}
                }
            }),
        ),
        anthropic_sse(
            "content_block_stop",
            &json!({"type":"content_block_stop","index":index}),
        ),
    ]
}

fn take_web_search_result_events(
    id_order: &[String],
    results_by_id: &mut HashMap<String, Vec<Value>>,
    errors_by_id: &mut HashMap<String, Value>,
    result_index_by_id: &mut HashMap<String, u32>,
    next_content_index: &mut u32,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    for search_id in id_order {
        let index = result_index_by_id.remove(search_id).unwrap_or_else(|| {
            let index = *next_content_index;
            *next_content_index += 1;
            index
        });
        let results = results_by_id.remove(search_id).unwrap_or_default();
        let content = errors_by_id
            .remove(search_id)
            .unwrap_or(Value::Array(results));
        events.extend(web_search_result_events(index, search_id, content));
    }
    events
}

fn reserve_web_search_result_index(
    search_id: &str,
    result_index_by_id: &mut HashMap<String, u32>,
    next_content_index: &mut u32,
) {
    result_index_by_id
        .entry(search_id.to_string())
        .or_insert_with(|| {
            let index = *next_content_index;
            *next_content_index += 1;
            index
        });
}

fn take_open_web_search_block_stop_events(
    open_indices: &mut HashSet<u32>,
    web_search_id_by_index: &HashMap<u32, String>,
) -> Vec<Bytes> {
    let mut indices: Vec<u32> = open_indices
        .iter()
        .copied()
        .filter(|index| web_search_id_by_index.contains_key(index))
        .collect();
    indices.sort_unstable();
    indices
        .into_iter()
        .map(|index| {
            open_indices.remove(&index);
            anthropic_sse(
                "content_block_stop",
                &json!({"type":"content_block_stop","index":index}),
            )
        })
        .collect()
}

fn web_search_limit_stop_events(web_search_count: u64, has_tool_use: bool) -> [Bytes; 2] {
    let stop_reason = if has_tool_use { "tool_use" } else { "end_turn" };
    [
        anthropic_sse(
            "message_delta",
            &json!({
                "type":"message_delta",
                "delta":{"stop_reason":stop_reason,"stop_sequence":null},
                "usage":{
                    "input_tokens":0,
                    "output_tokens":0,
                    "server_tool_use":{"web_search_requests":web_search_count}
                }
            }),
        ),
        anthropic_sse("message_stop", &json!({"type":"message_stop"})),
    ]
}

fn append_unique_web_search_results(target: &mut Vec<Value>, results: Vec<Value>) {
    merge_web_search_result_metadata(target, &results);
    let mut seen: HashSet<String> = target
        .iter()
        .filter_map(|result| result.get("url").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect();
    for result in results {
        let Some(url) = result.get("url").and_then(Value::as_str) else {
            continue;
        };
        if seen.insert(url.to_string()) {
            target.push(result);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebSearchCallDisposition {
    Accepted,
    LimitExceeded,
    Ignored,
}

struct WebSearchRecordState<'a> {
    ids_seen: &'a mut HashSet<String>,
    id_order: &'a mut Vec<String>,
    results_by_id: &'a mut HashMap<String, Vec<Value>>,
    errors_by_id: &'a mut HashMap<String, Value>,
    request_count: &'a mut u64,
    max_uses: Option<u64>,
    limit_exceeded_id: &'a mut Option<String>,
}

fn record_web_search_call(
    search_id: &str,
    item: &Value,
    state: &mut WebSearchRecordState<'_>,
) -> WebSearchCallDisposition {
    if state.ids_seen.contains(search_id) {
        if state.limit_exceeded_id.as_deref() == Some(search_id) {
            return WebSearchCallDisposition::LimitExceeded;
        }
        if !state.id_order.iter().any(|existing| existing == search_id) {
            return WebSearchCallDisposition::Ignored;
        }
    } else {
        state.ids_seen.insert(search_id.to_string());
        if state
            .max_uses
            .is_some_and(|limit| *state.request_count >= limit)
        {
            if state.limit_exceeded_id.is_some() {
                return WebSearchCallDisposition::Ignored;
            }
            *state.limit_exceeded_id = Some(search_id.to_string());
            state.id_order.push(search_id.to_string());
            state
                .errors_by_id
                .insert(search_id.to_string(), web_search_max_uses_exceeded_error());
            return WebSearchCallDisposition::LimitExceeded;
        }
        *state.request_count += 1;
        state.id_order.push(search_id.to_string());
    }
    append_unique_web_search_results(
        state
            .results_by_id
            .entry(search_id.to_string())
            .or_default(),
        web_search_results_from_action(item),
    );
    if let Some(error) = web_search_tool_result_error(item) {
        state.errors_by_id.insert(search_id.to_string(), error);
    } else {
        // A later successful/terminal item is authoritative if an earlier
        // lifecycle event was still in progress or transiently carried an
        // error field. Compatible gateways sometimes omit the final status.
        state.errors_by_id.remove(search_id);
    }
    WebSearchCallDisposition::Accepted
}

#[inline]
fn reasoning_item_key(data: &Value, item: Option<&Value>) -> Option<String> {
    if let Some(item_id) = item
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| data.get("item_id").and_then(Value::as_str))
    {
        return Some(format!("reasoning:{item_id}"));
    }
    data.get("output_index")
        .and_then(Value::as_u64)
        .map(|index| format!("reasoning:out:{index}"))
}

/// Resolve content index for a text/refusal content part event.
///
/// Uses `content_part_key` to look up or assign a stable index, falling back to
/// `fallback_open_index` when no key is available.
#[inline]
fn resolve_content_index(
    data: &Value,
    next_content_index: &mut u32,
    index_by_key: &mut HashMap<String, u32>,
    fallback_open_index: &mut Option<u32>,
) -> u32 {
    if let Some(k) = content_part_key(data) {
        if let Some(existing) = index_by_key.get(&k).copied() {
            existing
        } else {
            let assigned = *next_content_index;
            *next_content_index += 1;
            index_by_key.insert(k, assigned);
            assigned
        }
    } else if let Some(existing) = *fallback_open_index {
        existing
    } else {
        let assigned = *next_content_index;
        *next_content_index += 1;
        *fallback_open_index = Some(assigned);
        assigned
    }
}

#[derive(Default)]
struct WebSearchResultOrderingState {
    waiting_tool_use_id: Option<String>,
    server_block_index: Option<u64>,
    result_block_index: Option<u64>,
}

impl WebSearchResultOrderingState {
    fn is_waiting(&self) -> bool {
        self.waiting_tool_use_id.is_some()
    }

    fn can_emit(&self, event: Option<&Value>) -> bool {
        if !self.is_waiting() {
            return true;
        }
        let Some(event) = event else {
            return false;
        };
        let event_type = event.get("type").and_then(Value::as_str);
        if matches!(event_type, Some("ping" | "error")) {
            return true;
        }
        let event_index = event.get("index").and_then(Value::as_u64);
        if let Some(result_index) = self.result_block_index {
            return event_index == Some(result_index)
                && matches!(
                    event_type,
                    Some("content_block_delta" | "content_block_stop")
                );
        }
        if event_index == self.server_block_index
            && matches!(
                event_type,
                Some("content_block_delta" | "content_block_stop")
            )
        {
            return true;
        }
        event_type == Some("content_block_start")
            && event.pointer("/content_block/type").and_then(Value::as_str)
                == Some("web_search_tool_result")
            && event
                .pointer("/content_block/tool_use_id")
                .and_then(Value::as_str)
                == self.waiting_tool_use_id.as_deref()
    }

    fn observe_emitted(&mut self, event: Option<&Value>, hosted_web_search_name: &str) {
        let Some(event) = event else {
            return;
        };
        let event_type = event.get("type").and_then(Value::as_str);
        if !self.is_waiting()
            && event_type == Some("content_block_start")
            && event.pointer("/content_block/type").and_then(Value::as_str)
                == Some("server_tool_use")
            && event.pointer("/content_block/name").and_then(Value::as_str)
                == Some(hosted_web_search_name)
        {
            self.waiting_tool_use_id = event
                .pointer("/content_block/id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            self.server_block_index = event.get("index").and_then(Value::as_u64);
            return;
        }
        if !self.is_waiting() {
            return;
        }
        if self.result_block_index.is_none()
            && event_type == Some("content_block_start")
            && event.pointer("/content_block/type").and_then(Value::as_str)
                == Some("web_search_tool_result")
            && event
                .pointer("/content_block/tool_use_id")
                .and_then(Value::as_str)
                == self.waiting_tool_use_id.as_deref()
        {
            self.result_block_index = event.get("index").and_then(Value::as_u64);
            return;
        }
        if self.result_block_index.is_some()
            && event_type == Some("content_block_stop")
            && event.get("index").and_then(Value::as_u64) == self.result_block_index
        {
            *self = Self::default();
        }
    }
}

fn anthropic_event_value(bytes: &Bytes) -> Option<Value> {
    let body = std::str::from_utf8(bytes).ok()?;
    let data = body
        .lines()
        .filter_map(|line| strip_sse_field(line, "data"))
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(data.trim()).ok()
}

fn order_anthropic_web_search_result_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    hosted_web_search_name: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut state = WebSearchResultOrderingState::default();
        let mut pending = VecDeque::<Bytes>::new();
        let mut upstream_done = false;
        tokio::pin!(stream);

        loop {
            let can_emit_pending = pending.front().is_some_and(|bytes| {
                let event = anthropic_event_value(bytes);
                state.can_emit(event.as_ref())
            });
            if can_emit_pending {
                let Some(bytes) = pending.pop_front() else {
                    continue;
                };
                let event = anthropic_event_value(&bytes);
                state.observe_emitted(event.as_ref(), &hosted_web_search_name);
                yield Ok(bytes);
                continue;
            }

            if upstream_done {
                if state.is_waiting() {
                    yield Ok(anthropic_error_sse(
                        "Responses upstream ended before a hosted web-search result could be ordered",
                        "stream_truncated",
                    ));
                }
                break;
            }

            match stream.next().await {
                Some(Ok(bytes)) => {
                    let event = anthropic_event_value(&bytes);
                    if event
                        .as_ref()
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        == Some("error")
                    {
                        pending.clear();
                        yield Ok(bytes);
                        break;
                    }
                    if state.can_emit(event.as_ref()) {
                        state.observe_emitted(event.as_ref(), &hosted_web_search_name);
                        yield Ok(bytes);
                    } else {
                        pending.push_back(bytes);
                        // Keep the downstream stream observably active while semantic
                        // content waits for the paired hosted-search result.
                        yield Ok(anthropic_ping_sse());
                    }
                }
                Some(Err(error)) => {
                    yield Err(error);
                    break;
                }
                None => upstream_done = true,
            }
        }
    }
}

/// 创建从 Responses API SSE 到 Anthropic SSE 的转换流
///
/// 状态机跟踪: message_id, current_model, has_sent_message_start, item/content index map
/// SSE 解析支持 named events (event: + data: 行)
pub fn create_anthropic_sse_stream_from_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_anthropic_sse_stream_from_responses_with_web_search_options(stream, None, None)
}

pub(crate) fn create_anthropic_sse_stream_from_responses_with_web_search_options<
    E: std::error::Error + Send + 'static,
>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    hosted_web_search_name: Option<String>,
    max_web_search_uses: Option<u64>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let can_preserve_web_search_citations = hosted_web_search_name
        .as_deref()
        .is_some_and(|name| !name.is_empty());
    let hosted_web_search_name = hosted_web_search_name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "web_search".to_string());
    let raw_stream = create_anthropic_sse_stream_from_responses_raw(
        stream,
        hosted_web_search_name.clone(),
        max_web_search_uses,
        can_preserve_web_search_citations,
    );
    order_anthropic_web_search_result_stream(raw_stream, hosted_web_search_name)
}

fn create_anthropic_sse_stream_from_responses_raw<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    hosted_web_search_name: String,
    max_web_search_uses: Option<u64>,
    can_preserve_web_search_citations: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id: Option<String> = None;
        let mut current_model: Option<String> = None;
        let mut has_sent_message_start = false;
        let mut has_tool_use = false;
        let mut next_content_index: u32 = 0;
        let mut index_by_key: HashMap<String, u32> = HashMap::new();
        let mut open_indices: HashSet<u32> = HashSet::new();
        let mut fallback_open_index: Option<u32> = None;
        let mut current_text_index: Option<u32> = None;
        let mut streamed_text = StreamedTextState::default();
        let mut buffered_citation_text = BufferedCitationTextState::default();
        let mut preserve_web_search_citations = false;
        let mut tool_index_by_item_id: HashMap<String, u32> = HashMap::new();
        let mut tool_name_by_index: HashMap<u32, String> = HashMap::new();
        let mut tool_args_by_index: HashMap<u32, String> = HashMap::new();
        let mut tool_had_delta: HashSet<u32> = HashSet::new();
        let mut last_tool_index: Option<u32> = None;
        let mut web_search_index_by_item_id: HashMap<String, u32> = HashMap::new();
        let mut web_search_id_by_index: HashMap<u32, String> = HashMap::new();
        let mut web_search_result_index_by_id: HashMap<String, u32> = HashMap::new();
        let mut web_search_ids_seen: HashSet<String> = HashSet::new();
        let mut web_search_ids_completed: HashSet<String> = HashSet::new();
        let mut web_search_limit_exceeded_id: Option<String> = None;
        let mut web_search_id_order: Vec<String> = Vec::new();
        let mut web_search_results_by_id: HashMap<String, Vec<Value>> = HashMap::new();
        let mut web_search_errors_by_id: HashMap<String, Value> = HashMap::new();
        let mut pending_web_search_results: Vec<Value> = Vec::new();
        let mut seen_web_search_result_urls: HashSet<String> = HashSet::new();
        let mut web_search_count = 0_u64;
        let mut reasoning_index_by_item_id: HashMap<String, u32> = HashMap::new();
        let mut reasoning_item_by_index: HashMap<u32, Value> = HashMap::new();
        let mut reasoning_text_by_index: HashMap<u32, String> = HashMap::new();
        let mut legacy_reasoning_index: Option<u32> = None;
        let mut has_substantive_output = false;
        let mut terminated = false;

        // Append an EOF sentinel so the same parser handles a final SSE event that
        // omitted its trailing blank line. The boolean distinguishes the sentinel
        // from a legitimate empty upstream chunk.
        let stream = stream
            .map(|result| (result, false))
            .chain(futures::stream::once(async {
                (Ok::<Bytes, E>(Bytes::new()), true)
            }));
        tokio::pin!(stream);

        'stream_loop: while let Some((chunk, is_eof)) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    // A few compatible gateways ignore stream:true and return one
                    // JSON document. Hold it intact until EOF, including any pretty-
                    // printed blank lines that would otherwise look like SSE separators.
                    let looks_like_json = matches!(
                        buffer
                            .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
                            .as_bytes()
                            .first(),
                        Some(b'{') | Some(b'[')
                    );
                    if looks_like_json && !is_eof {
                        continue;
                    }
                    if looks_like_json && is_eof {
                        match serde_json::from_str::<Value>(buffer.trim()) {
                            Ok(body) => {
                                for event in responses_json_to_anthropic_sse(
                                    body,
                                    Some(hosted_web_search_name.as_str()),
                                    max_web_search_uses,
                                ) {
                                    yield Ok(event);
                                }
                                terminated = true;
                            }
                            Err(error) => {
                                yield Ok(anthropic_error_sse(
                                    &format!("Invalid JSON response from Responses upstream: {error}"),
                                    "response_parse_error",
                                ));
                                terminated = true;
                            }
                        }
                        buffer.clear();
                        continue;
                    }

                    if is_eof && !buffer.trim().is_empty() {
                        buffer.push_str("\n\n");
                    }

                    // SSE 事件由 \n\n 分隔
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }

                        // 解析 SSE 块：提取 event: 和 data: 行
                        let mut event_type: Option<String> = None;
                        let mut data_parts: Vec<String> = Vec::new();

                        for line in block.lines() {
                            if let Some(evt) = strip_sse_field(line, "event") {
                                event_type = Some(evt.trim().to_string());
                            } else if let Some(d) = strip_sse_field(line, "data") {
                                data_parts.push(d.to_string());
                            }
                        }

                        if data_parts.is_empty() {
                            continue;
                        }

                        let data_str = data_parts.join("\n");

                        // 解析 JSON 数据
                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        // Official streams use both a named SSE event and `type` in
                        // the JSON payload. Compatible gateways sometimes omit the
                        // `event:` line, so fall back to the payload type.
                        let event_name = event_type
                            .as_deref()
                            .filter(|name| !name.is_empty())
                            .or_else(|| data.get("type").and_then(Value::as_str))
                            .unwrap_or("");

                        log::debug!("[Claude/Responses] <<< SSE event: {event_name}");

                        // Ignore every event after a terminal response. In particular,
                        // do not synthesize message_start if a broken gateway emits a
                        // late delta after response.failed/error.
                        if terminated {
                            continue;
                        }

                        let delta_requires_message_start = matches!(
                            event_name,
                            "response.output_text.delta"
                                | "response.refusal.delta"
                                | "response.function_call_arguments.delta"
                                | "response.reasoning_summary_text.delta"
                                | "response.reasoning_text.delta"
                                | "response.reasoning.delta"
                        );
                        if delta_requires_message_start {
                            has_substantive_output = true;
                        }
                        if delta_requires_message_start && !has_sent_message_start {
                            yield Ok(anthropic_sse(
                                "message_start",
                                &json!({
                                    "type":"message_start",
                                    "message":{
                                        "id":message_id.clone().unwrap_or_default(),
                                        "type":"message",
                                        "role":"assistant",
                                        "model":current_model.clone().unwrap_or_default(),
                                        "usage":{"input_tokens":0,"output_tokens":0}
                                    }
                                }),
                            ));
                            has_sent_message_start = true;
                        }

                        match event_name {
                            // ================================================
                            // response.created → message_start
                            // ================================================
                            "response.created" => {
                                let response_obj = response_object_from_event(&data);
                                if let Some(id) = response_obj.get("id").and_then(|i| i.as_str()) {
                                    message_id = Some(id.to_string());
                                }
                                if let Some(model) =
                                    response_obj.get("model").and_then(|m| m.as_str())
                                {
                                    current_model = Some(model.to_string());
                                }

                                has_sent_message_start = true;
                                // Build usage with defensive null handling
                                // Some() wrapper ensures build function always receives valid input
                                // Fallback to empty object {} if usage field missing, ensuring message_start
                                // event always has valid usage structure for VSCode Extension compatibility
                                let start_usage = build_anthropic_usage_from_responses(
                                    Some(response_obj.get("usage").unwrap_or(&json!({}))),
                                );

                                let event = json!({
                                    "type": "message_start",
                                    "message": {
                                        "id": message_id.clone().unwrap_or_default(),
                                        "type": "message",
                                        "role": "assistant",
                                        "model": current_model.clone().unwrap_or_default(),
                                        "usage": start_usage
                                    }
                                });
                                let sse = format!("event: message_start\ndata: {}\n\n",
                                    serde_json::to_string(&event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_start");
                                yield Ok(Bytes::from(sse));
                            }

                            // ================================================
                            // response.content_part.added → content_block_start (text)
                            // ================================================
                            "response.content_part.added" => {
                                // 确保 message_start 已发送
                                if !has_sent_message_start {
                                    let start_event = json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": message_id.clone().unwrap_or_default(),
                                            "type": "message",
                                            "role": "assistant",
                                            "model": current_model.clone().unwrap_or_default(),
                                            "usage": { "input_tokens": 0, "output_tokens": 0 }
                                        }
                                    });
                                    let sse = format!("event: message_start\ndata: {}\n\n",
                                        serde_json::to_string(&start_event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                    has_sent_message_start = true;
                                }

                                if let Some(part) = data.get("part") {
                                    let part_type = part.get("type").and_then(|t| t.as_str());
                                    if matches!(part_type, Some("output_text") | Some("refusal")) {
                                        if preserve_web_search_citations
                                            && part_type == Some("output_text")
                                        {
                                            buffered_citation_text.record_part(&data, part, false);
                                            if current_text_index.is_none()
                                                && part
                                                    .get("text")
                                                    .and_then(Value::as_str)
                                                    .is_some_and(|text| !text.is_empty())
                                            {
                                                current_text_index =
                                                    Some(resolve_content_index(
                                                        &data,
                                                        &mut next_content_index,
                                                        &mut index_by_key,
                                                        &mut fallback_open_index,
                                                    ));
                                            }
                                            continue;
                                        }

                                        let index = if let Some(index) = current_text_index {
                                            index
                                        } else {
                                            let index = resolve_content_index(
                                                &data,
                                                &mut next_content_index,
                                                &mut index_by_key,
                                                &mut fallback_open_index,
                                            );
                                            current_text_index = Some(index);
                                            index
                                        };

                                        if open_indices.contains(&index) {
                                            continue;
                                        }

                                        let event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "text",
                                                "text": ""
                                            }
                                        });
                                        let sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                        open_indices.insert(index);
                                    }
                                }
                            }

                            // ================================================
                            // response.output_text.delta → content_block_delta (text_delta)
                            // ================================================
                            "response.output_text.delta" => {
                                if let Some(delta) = data.get("delta").and_then(|d| d.as_str()) {
                                    if preserve_web_search_citations {
                                        buffered_citation_text.record_delta(&data, delta);
                                        if current_text_index.is_none() {
                                            current_text_index = Some(resolve_content_index(
                                                &data,
                                                &mut next_content_index,
                                                &mut index_by_key,
                                                &mut fallback_open_index,
                                            ));
                                        }
                                        yield Ok(anthropic_ping_sse());
                                        continue;
                                    }
                                    streamed_text.record_delta(&data, delta);
                                    let index = if let Some(index) = current_text_index {
                                        index
                                    } else {
                                        let index = resolve_content_index(
                                            &data,
                                            &mut next_content_index,
                                            &mut index_by_key,
                                            &mut fallback_open_index,
                                        );
                                        current_text_index = Some(index);
                                        index
                                    };

                                    if !open_indices.contains(&index) {
                                        let start_event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "text",
                                                "text": ""
                                            }
                                        });
                                        let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&start_event).unwrap_or_default());
                                        yield Ok(Bytes::from(start_sse));
                                        open_indices.insert(index);
                                    }
                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": index,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            "response.output_text.annotation.added"
                                if preserve_web_search_citations =>
                            {
                                if let Some(annotation) = data.get("annotation") {
                                    buffered_citation_text.record_annotation(&data, annotation);
                                }
                            }
                            "response.output_text.annotation.added" => {}

                            // ================================================
                            // response.refusal.delta → content_block_delta (text_delta)
                            // ================================================
                            "response.refusal.delta" => {
                                if let Some(delta) = data.get("delta").and_then(|d| d.as_str()) {
                                    streamed_text.record_delta(&data, delta);
                                    let index = if let Some(index) = current_text_index {
                                        index
                                    } else {
                                        let index = resolve_content_index(
                                            &data,
                                            &mut next_content_index,
                                            &mut index_by_key,
                                            &mut fallback_open_index,
                                        );
                                        current_text_index = Some(index);
                                        index
                                    };

                                    if !open_indices.contains(&index) {
                                        let start_event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "text",
                                                "text": ""
                                            }
                                        });
                                        let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&start_event).unwrap_or_default());
                                        yield Ok(Bytes::from(start_sse));
                                        open_indices.insert(index);
                                    }

                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": index,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            // ================================================
                            // response.content_part.done → content_block_stop
                            // ================================================
                            "response.content_part.done" if preserve_web_search_citations => {
                                let accepted = if let Some(part) = data
                                    .get("part")
                                    .filter(|part| {
                                        part.get("type").and_then(Value::as_str)
                                            == Some("output_text")
                                    })
                                {
                                    buffered_citation_text.record_part(&data, part, true)
                                } else {
                                    true
                                };
                                if accepted {
                                    buffered_citation_text.finish_event_part(&data);
                                }
                            }
                            "response.content_part.done" => {
                                streamed_text.finish_part(&data);
                            }

                            // ================================================
                            // response.output_item.added (function_call) → content_block_start (tool_use)
                            // ================================================
                            "response.output_item.added" => {
                                if let Some(item) = data.get("item") {
                                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if can_preserve_web_search_citations
                                        && item_type == "web_search_call"
                                    {
                                        preserve_web_search_citations = true;
                                    }
                                    if preserve_web_search_citations && item_type != "message" {
                                        let pending_text =
                                            buffered_citation_text.render_pending_parts();
                                        let mut reusable_text_index = None;
                                        if !pending_text.is_empty() {
                                            if let Some(index) = current_text_index.take() {
                                                let was_open = open_indices.remove(&index);
                                                if was_open {
                                                    yield Ok(anthropic_sse(
                                                        "content_block_stop",
                                                        &json!({"type":"content_block_stop","index":index}),
                                                    ));
                                                } else {
                                                    reusable_text_index = Some(index);
                                                }
                                                if fallback_open_index == Some(index) {
                                                    fallback_open_index = None;
                                                }
                                            }
                                        }
                                        for text in pending_text {
                                            let index =
                                                reusable_text_index.take().unwrap_or_else(|| {
                                                    let index = next_content_index;
                                                    next_content_index += 1;
                                                    index
                                                });
                                            for event in text_block_events(index, &text) {
                                                yield Ok(event);
                                            }
                                        }
                                    }
                                    if item_type == "function_call" {
                                        has_tool_use = true;
                                        has_substantive_output = true;
                                        if let Some(index) = current_text_index.take() {
                                            if open_indices.remove(&index) {
                                                let stop_event = json!({
                                                    "type": "content_block_stop",
                                                    "index": index
                                                });
                                                let stop_sse = format!("event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&stop_event).unwrap_or_default());
                                                yield Ok(Bytes::from(stop_sse));
                                            }
                                            if fallback_open_index == Some(index) {
                                                fallback_open_index = None;
                                            }
                                        }
                                        // 确保 message_start 已发送
                                        if !has_sent_message_start {
                                            let start_event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                                }
                                            });
                                            let sse = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&start_event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                            has_sent_message_start = true;
                                        }

                                        let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                        let index = if let Some(k) = tool_item_key_from_added(&data, item) {
                                            if let Some(existing) = index_by_key.get(&k).copied() {
                                                existing
                                            } else {
                                                let assigned = next_content_index;
                                                next_content_index += 1;
                                                index_by_key.insert(k, assigned);
                                                assigned
                                            }
                                        } else {
                                            let assigned = next_content_index;
                                            next_content_index += 1;
                                            assigned
                                        };
                                        if let Some(item_id) = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .or_else(|| data.get("item_id").and_then(|v| v.as_str()))
                                        {
                                            tool_index_by_item_id.insert(item_id.to_string(), index);
                                        }
                                        tool_name_by_index.insert(index, name.to_string());
                                        last_tool_index = Some(index);

                                        if open_indices.contains(&index) {
                                            continue;
                                        }

                                        tool_args_by_index.insert(index, String::new());

                                        let event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "tool_use",
                                                "id": call_id,
                                                "name": name
                                            }
                                        });
                                        let sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                        open_indices.insert(index);
                                    } else if item_type == "web_search_call" {
                                        has_substantive_output = true;
                                        if let Some(index) = current_text_index.take() {
                                            if open_indices.remove(&index) {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":index}),
                                                ));
                                            }
                                            if fallback_open_index == Some(index) {
                                                fallback_open_index = None;
                                            }
                                        }
                                        if !has_sent_message_start {
                                            yield Ok(anthropic_sse(
                                                "message_start",
                                                &json!({
                                                    "type":"message_start",
                                                    "message":{
                                                        "id":message_id.clone().unwrap_or_default(),
                                                        "type":"message",
                                                        "role":"assistant",
                                                        "model":current_model.clone().unwrap_or_default(),
                                                        "usage":{"input_tokens":0,"output_tokens":0}
                                                    }
                                                }),
                                            ));
                                            has_sent_message_start = true;
                                        }

                                        let item_keys = web_search_item_keys(&data, Some(item));
                                        let index = if let Some(existing) = item_keys
                                            .iter()
                                            .find_map(|key| index_by_key.get(key).copied())
                                        {
                                            existing
                                        } else {
                                            let assigned = next_content_index;
                                            next_content_index += 1;
                                            assigned
                                        };
                                        for key in item_keys {
                                            index_by_key.insert(key, index);
                                        }
                                        let search_id = web_search_id_by_index
                                            .get(&index)
                                            .cloned()
                                            .or_else(|| {
                                                item.get("id")
                                                    .and_then(Value::as_str)
                                                    .or_else(|| {
                                                        data.get("item_id").and_then(Value::as_str)
                                                    })
                                                    .filter(|id| !id.is_empty())
                                                    .map(ToString::to_string)
                                            })
                                            .unwrap_or_else(|| format!("ws_stream_{index}"));
                                        let disposition = record_web_search_call(
                                            &search_id,
                                            item,
                                            &mut WebSearchRecordState {
                                                ids_seen: &mut web_search_ids_seen,
                                                id_order: &mut web_search_id_order,
                                                results_by_id: &mut web_search_results_by_id,
                                                errors_by_id: &mut web_search_errors_by_id,
                                                request_count: &mut web_search_count,
                                                max_uses: max_web_search_uses,
                                                limit_exceeded_id:
                                                    &mut web_search_limit_exceeded_id,
                                            },
                                        );
                                        if disposition == WebSearchCallDisposition::Ignored {
                                            continue;
                                        }
                                        web_search_index_by_item_id
                                            .insert(search_id.clone(), index);
                                        web_search_id_by_index.insert(index, search_id.clone());
                                        reserve_web_search_result_index(
                                            &search_id,
                                            &mut web_search_result_index_by_id,
                                            &mut next_content_index,
                                        );

                                        if !open_indices.contains(&index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_start",
                                                &json!({
                                                    "type":"content_block_start",
                                                    "index":index,
                                                    "content_block":{
                                                        "type":"server_tool_use",
                                                        "id":search_id,
                                                        "name":hosted_web_search_name.as_str(),
                                                        "input":{},
                                                        "caller":{"type":"direct"}
                                                    }
                                                }),
                                            ));
                                            open_indices.insert(index);
                                        }
                                        if disposition
                                            == WebSearchCallDisposition::LimitExceeded
                                        {
                                            yield Ok(anthropic_sse(
                                                "content_block_delta",
                                                &json!({
                                                    "type":"content_block_delta",
                                                    "index":index,
                                                    "delta":{
                                                        "type":"input_json_delta",
                                                        "partial_json":"{}"
                                                    }
                                                }),
                                            ));
                                            if open_indices.remove(&index) {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":index}),
                                                ));
                                            }
                                            web_search_ids_completed.insert(search_id);
                                            for event in take_open_web_search_block_stop_events(
                                                &mut open_indices,
                                                &web_search_id_by_index,
                                            ) {
                                                yield Ok(event);
                                            }
                                            for event in take_web_search_result_events(
                                                &web_search_id_order,
                                                &mut web_search_results_by_id,
                                                &mut web_search_errors_by_id,
                                                &mut web_search_result_index_by_id,
                                                &mut next_content_index,
                                            ) {
                                                yield Ok(event);
                                            }
                                            if open_indices.is_empty() {
                                                for event in web_search_limit_stop_events(
                                                    web_search_count,
                                                    has_tool_use,
                                                ) {
                                                    yield Ok(event);
                                                }
                                            } else {
                                                yield Ok(anthropic_error_sse(
                                                    "Responses upstream started a web search beyond max_uses while another content block was incomplete",
                                                    "stream_truncated",
                                                ));
                                            }
                                            terminated = true;
                                            break 'stream_loop;
                                        }
                                    } else if item_type == "reasoning" {
                                        if !has_sent_message_start {
                                            let start_event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                                }
                                            });
                                            let sse = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&start_event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                            has_sent_message_start = true;
                                        }

                                        let index = if let Some(key) = reasoning_item_key(&data, Some(item)) {
                                            if let Some(existing) = index_by_key.get(&key).copied() {
                                                existing
                                            } else {
                                                let assigned = next_content_index;
                                                next_content_index += 1;
                                                index_by_key.insert(key, assigned);
                                                assigned
                                            }
                                        } else {
                                            let assigned = next_content_index;
                                            next_content_index += 1;
                                            assigned
                                        };
                                        if let Some(item_id) = item
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .or_else(|| data.get("item_id").and_then(Value::as_str))
                                        {
                                            reasoning_index_by_item_id.insert(item_id.to_string(), index);
                                        }
                                        reasoning_item_by_index.insert(index, item.clone());
                                        reasoning_text_by_index.entry(index).or_default();
                                    }
                                    // message type output_item.added is handled via content_part.added
                                }
                            }

                            // ================================================
                            // response.function_call_arguments.delta → content_block_delta (input_json_delta)
                            // ================================================
                            "response.function_call_arguments.delta" => {
                                if let Some(delta) = data.get("delta").and_then(|d| d.as_str()) {
                                    has_tool_use = true;
                                    let item_id = data.get("item_id").and_then(|v| v.as_str());
                                    let index = if let Some(id) = item_id {
                                        tool_index_by_item_id.get(id).copied()
                                    } else {
                                        None
                                    }
                                    .or_else(|| {
                                        tool_item_key_from_event(&data)
                                            .and_then(|k| index_by_key.get(&k).copied())
                                    })
                                    .or(last_tool_index)
                                    .unwrap_or_else(|| {
                                        let assigned = next_content_index;
                                        next_content_index += 1;
                                        assigned
                                    });

                                    if let Some(id) = item_id {
                                        tool_index_by_item_id.insert(id.to_string(), index);
                                    }
                                    if let Some(name) = data.get("name").and_then(Value::as_str) {
                                        tool_name_by_index.insert(index, name.to_string());
                                    } else {
                                        tool_name_by_index.entry(index).or_default();
                                    }
                                    last_tool_index = Some(index);

                                    if !open_indices.contains(&index) {
                                        let start_event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "tool_use",
                                                "id": data
                                                    .get("call_id")
                                                    .and_then(|v| v.as_str())
                                                    .or(item_id)
                                                    .unwrap_or(""),
                                                "name": data
                                                    .get("name")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                            }
                                        });
                                        let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&start_event).unwrap_or_default());
                                        yield Ok(Bytes::from(start_sse));
                                        open_indices.insert(index);
                                    }

                                    tool_args_by_index
                                        .entry(index)
                                        .or_default()
                                        .push_str(delta);
                                    tool_had_delta.insert(index);

                                    if tool_name_by_index.get(&index).map(String::as_str) == Some("Read") {
                                        continue;
                                    }

                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": index,
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            // ================================================
                            // response.function_call_arguments.done → content_block_stop
                            // ================================================
                            "response.function_call_arguments.done" => {
                                has_tool_use = true;
                                let item_id = data.get("item_id").and_then(|v| v.as_str());
                                let index = if let Some(id) = item_id {
                                    tool_index_by_item_id.get(id).copied()
                                } else {
                                    None
                                }
                                .or_else(|| {
                                    tool_item_key_from_event(&data)
                                        .and_then(|k| index_by_key.get(&k).copied())
                                })
                                .or(last_tool_index);
                                if let Some(index) = index {
                                    if !open_indices.remove(&index) {
                                        continue;
                                    }
                                    if tool_name_by_index.get(&index).map(String::as_str) == Some("Read") {
                                        let raw = data
                                            .get("arguments")
                                            .or_else(|| data.pointer("/item/arguments"))
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string)
                                            .unwrap_or_else(|| {
                                                tool_args_by_index
                                                    .get(&index)
                                                    .cloned()
                                                    .unwrap_or_default()
                                            });
                                        let sanitized = sanitize_anthropic_tool_use_input_json("Read", &raw);
                                        if !sanitized.is_empty() {
                                            let event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {
                                                    "type": "input_json_delta",
                                                    "partial_json": sanitized
                                                }
                                            });
                                            let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                        }
                                    } else if !tool_had_delta.contains(&index) {
                                        // Some compatible gateways skip delta events and only
                                        // provide the complete arguments on the done event.
                                        if let Some(arguments) = data
                                            .get("arguments")
                                            .or_else(|| data.pointer("/item/arguments"))
                                            .and_then(Value::as_str)
                                            .filter(|value| !value.is_empty())
                                        {
                                            let event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {
                                                    "type": "input_json_delta",
                                                    "partial_json": arguments
                                                }
                                            });
                                            let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                        }
                                    }
                                    let event = json!({
                                        "type": "content_block_stop",
                                        "index": index
                                    });
                                    let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                    if let Some(item_id) = item_id {
                                        tool_index_by_item_id.remove(item_id);
                                    }
                                    tool_name_by_index.remove(&index);
                                    tool_args_by_index.remove(&index);
                                    tool_had_delta.remove(&index);
                                }
                            }

                            // ================================================
                            // response.refusal.done → content_block_stop
                            // ================================================
                            "response.refusal.done" => {
                                streamed_text.finish_part(&data);
                                let index = current_text_index.take().or_else(|| {
                                    let key = content_part_key(&data);
                                    if let Some(k) = key {
                                        index_by_key.get(&k).copied()
                                    } else {
                                        fallback_open_index
                                    }
                                });
                                if let Some(index) = index {
                                    if !open_indices.remove(&index) {
                                        continue;
                                    }
                                    let event = json!({
                                        "type": "content_block_stop",
                                        "index": index
                                    });
                                    let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                    if fallback_open_index == Some(index) {
                                        fallback_open_index = None;
                                    }
                                }
                            }

                            // ================================================
                            // Official reasoning text events → thinking_delta.
                            // response.reasoning.delta is kept as a compatibility alias.
                            // ================================================
                            "response.reasoning_summary_text.delta"
                            | "response.reasoning_text.delta"
                            | "response.reasoning.delta" => {
                                if let Some(delta) = data
                                    .get("delta")
                                    .or_else(|| data.get("text"))
                                    .and_then(|d| d.as_str())
                                {
                                    if let Some(index) = current_text_index.take() {
                                        if open_indices.remove(&index) {
                                            let stop_event = json!({
                                                "type": "content_block_stop",
                                                "index": index
                                            });
                                            let stop_sse = format!("event: content_block_stop\ndata: {}\n\n",
                                                serde_json::to_string(&stop_event).unwrap_or_default());
                                            yield Ok(Bytes::from(stop_sse));
                                        }
                                        if fallback_open_index == Some(index) {
                                            fallback_open_index = None;
                                        }
                                    }
                                    let item_id = data.get("item_id").and_then(Value::as_str);
                                    let item_key = reasoning_item_key(&data, None);
                                    let is_keyless = item_id.is_none() && item_key.is_none();
                                    let index = item_id
                                        .and_then(|id| reasoning_index_by_item_id.get(id).copied())
                                        .or_else(|| {
                                            item_key
                                                .as_ref()
                                                .and_then(|key| index_by_key.get(key).copied())
                                        })
                                        .or_else(|| {
                                            is_keyless
                                                .then_some(legacy_reasoning_index)
                                                .flatten()
                                        })
                                        .unwrap_or_else(|| {
                                            let assigned = next_content_index;
                                            next_content_index += 1;
                                            if let Some(key) = item_key {
                                                index_by_key.insert(key, assigned);
                                            }
                                            if let Some(id) = item_id {
                                                reasoning_index_by_item_id
                                                    .insert(id.to_string(), assigned);
                                            } else if is_keyless {
                                                legacy_reasoning_index = Some(assigned);
                                            }
                                            assigned
                                        });

                                    if !open_indices.contains(&index) {
                                        let start_event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "thinking",
                                                "thinking": ""
                                            }
                                        });
                                        let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&start_event).unwrap_or_default());
                                        yield Ok(Bytes::from(start_sse));
                                        open_indices.insert(index);
                                    }

                                    reasoning_text_by_index
                                        .entry(index)
                                        .or_default()
                                        .push_str(delta);

                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": index,
                                        "delta": {
                                            "type": "thinking_delta",
                                            "thinking": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            // ================================================
                            // Official done events carry the complete visible text. If a
                            // gateway omitted deltas, emit the text here. The block stays
                            // open until output_item.done supplies encrypted_content.
                            // ================================================
                            "response.reasoning_summary_text.done"
                            | "response.reasoning_text.done" => {
                                let item_id = data.get("item_id").and_then(Value::as_str);
                                let item_key = reasoning_item_key(&data, None);
                                let index = item_id
                                    .and_then(|id| reasoning_index_by_item_id.get(id).copied())
                                    .or_else(|| {
                                        item_key
                                            .as_ref()
                                            .and_then(|key| index_by_key.get(key).copied())
                                    })
                                    .or_else(|| {
                                        (item_id.is_none() && item_key.is_none())
                                            .then_some(legacy_reasoning_index)
                                            .flatten()
                                    });
                                if let Some(index) = index {
                                    let already_emitted = reasoning_text_by_index
                                        .get(&index)
                                        .is_some_and(|value| !value.is_empty());
                                    if !already_emitted {
                                        if let Some(text) = data
                                            .get("text")
                                            .and_then(Value::as_str)
                                            .filter(|value| !value.is_empty())
                                        {
                                            if !open_indices.contains(&index) {
                                                let start_event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {"type": "thinking", "thinking": ""}
                                                });
                                                let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&start_event).unwrap_or_default());
                                                yield Ok(Bytes::from(start_sse));
                                                open_indices.insert(index);
                                            }
                                            reasoning_text_by_index
                                                .entry(index)
                                                .or_default()
                                                .push_str(text);
                                            let event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {"type": "thinking_delta", "thinking": text}
                                            });
                                            let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                        }
                                    }
                                }
                            }

                            // Legacy gateways do not emit output_item.done, so retain the
                            // old close behavior for their non-standard done event.
                            "response.reasoning.done" => {
                                let item_id = data.get("item_id").and_then(Value::as_str);
                                let item_key = reasoning_item_key(&data, None);
                                let index = item_id
                                    .and_then(|id| reasoning_index_by_item_id.get(id).copied())
                                    .or_else(|| {
                                        item_key
                                            .as_ref()
                                            .and_then(|key| index_by_key.get(key).copied())
                                    })
                                    .or_else(|| {
                                        (item_id.is_none() && item_key.is_none())
                                            .then_some(legacy_reasoning_index)
                                            .flatten()
                                    });
                                if let Some(index) = index {
                                    if open_indices.remove(&index) {
                                        let event = json!({"type": "content_block_stop", "index": index});
                                        let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                    }
                                    if legacy_reasoning_index == Some(index) {
                                        legacy_reasoning_index = None;
                                    }
                                }
                            }

                            // ================================================
                            // response.completed / response.incomplete → message_delta + message_stop
                            // ================================================
                            "response.completed" | "response.incomplete" => {
                                let response_obj = response_object_from_event(&data);
                                if matches!(
                                    response_obj.get("status").and_then(Value::as_str),
                                    Some("failed" | "cancelled")
                                ) || response_obj
                                    .get("error")
                                    .is_some_and(|error| !error.is_null())
                                {
                                    let (message, error_type) = responses_error_details(
                                        &data,
                                        "Responses upstream returned a failed terminal response",
                                    );
                                    yield Ok(anthropic_error_sse(&message, &error_type));
                                    terminated = true;
                                    continue;
                                }
                                if !has_sent_message_start {
                                    if let Some(id) = response_obj.get("id").and_then(Value::as_str) {
                                        message_id = Some(id.to_string());
                                    }
                                    if let Some(model) =
                                        response_obj.get("model").and_then(Value::as_str)
                                    {
                                        current_model = Some(model.to_string());
                                    }
                                    yield Ok(anthropic_sse(
                                        "message_start",
                                        &json!({
                                            "type":"message_start",
                                            "message":{
                                                "id":message_id.clone().unwrap_or_default(),
                                                "type":"message",
                                                "role":"assistant",
                                                "model":current_model.clone().unwrap_or_default(),
                                                "usage":{"input_tokens":0,"output_tokens":0}
                                            }
                                        }),
                                    ));
                                    has_sent_message_start = true;
                                }

                                let mut terminal_web_search_results =
                                    std::mem::take(&mut pending_web_search_results);
                                let mut terminal_message_items = Vec::new();
                                let mut terminal_web_search_limit_exceeded = false;
                                let mut terminal_reusable_text_index = None;
                                if let Some(output) =
                                    response_obj.get("output").and_then(Value::as_array)
                                {
                                    let has_web_search_output = output.iter().any(|item| {
                                        item.get("type").and_then(Value::as_str)
                                            == Some("web_search_call")
                                    });
                                    if has_web_search_output {
                                        if can_preserve_web_search_citations {
                                            preserve_web_search_citations = true;
                                        }
                                        if let Some(text_index) = current_text_index.take() {
                                            let was_open = open_indices.remove(&text_index);
                                            if was_open {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":text_index}),
                                                ));
                                            } else if preserve_web_search_citations {
                                                terminal_reusable_text_index = Some(text_index);
                                            }
                                            if fallback_open_index == Some(text_index) {
                                                fallback_open_index = None;
                                            }
                                        }
                                    }

                                    for (output_index, item) in output.iter().enumerate() {
                                        if item.get("type").and_then(Value::as_str)
                                            == Some("message")
                                        {
                                            terminal_message_items
                                                .push((output_index as u64, item.clone()));
                                        }
                                        if item.get("type").and_then(Value::as_str)
                                            == Some("web_search_call")
                                        {
                                            has_substantive_output = true;
                                            let provisional_index = index_by_key
                                                .get(&format!(
                                                    "web-search:out:{output_index}"
                                                ))
                                                .copied();
                                            let search_id = provisional_index
                                                .and_then(|index| {
                                                    web_search_id_by_index.get(&index).cloned()
                                                })
                                                .or_else(|| {
                                                    item.get("id")
                                                        .and_then(Value::as_str)
                                                        .filter(|id| !id.is_empty())
                                                        .map(ToString::to_string)
                                                })
                                                .or_else(|| {
                                                    provisional_index.map(|index| {
                                                        format!("ws_stream_{index}")
                                                    })
                                                })
                                                .unwrap_or_else(|| {
                                                    format!("ws_terminal_{output_index}")
                                                });
                                            let disposition = record_web_search_call(
                                                &search_id,
                                                item,
                                                &mut WebSearchRecordState {
                                                    ids_seen: &mut web_search_ids_seen,
                                                    id_order: &mut web_search_id_order,
                                                    results_by_id:
                                                        &mut web_search_results_by_id,
                                                    errors_by_id:
                                                        &mut web_search_errors_by_id,
                                                    request_count: &mut web_search_count,
                                                    max_uses: max_web_search_uses,
                                                    limit_exceeded_id:
                                                        &mut web_search_limit_exceeded_id,
                                                },
                                            );
                                            if disposition
                                                == WebSearchCallDisposition::Ignored
                                            {
                                                continue;
                                            }

                                            if !web_search_ids_completed.contains(&search_id) {
                                                let index = web_search_index_by_item_id
                                                    .get(&search_id)
                                                    .copied()
                                                    .or(provisional_index)
                                                    .unwrap_or_else(|| {
                                                        let assigned = next_content_index;
                                                        next_content_index += 1;
                                                        assigned
                                                    });
                                                web_search_index_by_item_id
                                                    .insert(search_id.clone(), index);
                                                web_search_id_by_index
                                                    .insert(index, search_id.clone());
                                                reserve_web_search_result_index(
                                                    &search_id,
                                                    &mut web_search_result_index_by_id,
                                                    &mut next_content_index,
                                                );
                                                if !open_indices.contains(&index) {
                                                    yield Ok(anthropic_sse(
                                                        "content_block_start",
                                                        &json!({
                                                            "type":"content_block_start",
                                                            "index":index,
                                                            "content_block":{
                                                                "type":"server_tool_use",
                                                                "id":search_id,
                                                                "name":hosted_web_search_name.as_str(),
                                                                "input":{},
                                                                "caller":{"type":"direct"}
                                                            }
                                                        }),
                                                    ));
                                                    open_indices.insert(index);
                                                }
                                                let input = web_search_action_input(item);
                                                yield Ok(anthropic_sse(
                                                    "content_block_delta",
                                                    &json!({
                                                        "type":"content_block_delta",
                                                        "index":index,
                                                        "delta":{
                                                            "type":"input_json_delta",
                                                            "partial_json":serde_json::to_string(&input)
                                                                .unwrap_or_else(|_| "{}".to_string())
                                                        }
                                                    }),
                                                ));
                                                if open_indices.remove(&index) {
                                                    yield Ok(anthropic_sse(
                                                        "content_block_stop",
                                                        &json!({"type":"content_block_stop","index":index}),
                                                    ));
                                                }
                                                web_search_ids_completed.insert(search_id);
                                            }
                                            if disposition
                                                == WebSearchCallDisposition::LimitExceeded
                                            {
                                                terminal_web_search_limit_exceeded = true;
                                                break;
                                            }
                                        }

                                        for result in web_search_results_from_output_item(item) {
                                            let Some(url) =
                                                result.get("url").and_then(Value::as_str)
                                            else {
                                                continue;
                                            };
                                            if seen_web_search_result_urls
                                                .insert(url.to_string())
                                            {
                                                terminal_web_search_results.push(result);
                                            }
                                        }
                                    }
                                }
                                if terminal_web_search_limit_exceeded {
                                    terminal_message_items.clear();
                                    terminal_web_search_results.clear();
                                }

                                for (search_id, results) in &mut web_search_results_by_id {
                                    if !web_search_errors_by_id.contains_key(search_id) {
                                        merge_web_search_result_metadata(
                                            results,
                                            &terminal_web_search_results,
                                        );
                                    }
                                }
                                let attributed_web_search_urls: HashSet<String> =
                                    web_search_results_by_id
                                        .iter()
                                        .filter(|(search_id, _)| {
                                            !web_search_errors_by_id.contains_key(*search_id)
                                        })
                                        .flat_map(|(_, results)| results)
                                        .filter_map(|result| {
                                            result
                                                .get("url")
                                                .and_then(Value::as_str)
                                                .map(ToString::to_string)
                                        })
                                        .collect();
                                // Final-message citations have no search-call ID.
                                // Prefer action.sources recorded for each call,
                                // then put only otherwise-unassigned citations on
                                // the last call. Every earlier call still receives
                                // an empty successful result rather than remaining
                                // structurally unmatched.
                                if let Some(last_search_id) = web_search_id_order
                                    .iter()
                                    .rev()
                                    .find(|search_id| {
                                        !web_search_errors_by_id.contains_key(*search_id)
                                    })
                                    .cloned()
                                {
                                    terminal_web_search_results.retain(|result| {
                                        result
                                            .get("url")
                                            .and_then(Value::as_str)
                                            .is_some_and(|url| {
                                                !attributed_web_search_urls.contains(url)
                                            })
                                    });
                                    append_unique_web_search_results(
                                        web_search_results_by_id
                                            .entry(last_search_id)
                                            .or_default(),
                                        terminal_web_search_results,
                                    );
                                }

                                if !web_search_id_order.is_empty() {
                                    if let Some(text_index) = current_text_index.take() {
                                        let was_open = open_indices.remove(&text_index);
                                        if was_open {
                                            yield Ok(anthropic_sse(
                                                "content_block_stop",
                                                &json!({"type":"content_block_stop","index":text_index}),
                                            ));
                                        } else if preserve_web_search_citations
                                            && terminal_reusable_text_index.is_none()
                                        {
                                            terminal_reusable_text_index = Some(text_index);
                                        }
                                        if fallback_open_index == Some(text_index) {
                                            fallback_open_index = None;
                                        }
                                    }
                                    // An incomplete terminal response can arrive before
                                    // output_item.done. Close any still-open server tool
                                    // block before emitting its paired error result.
                                    for search_id in web_search_id_order.clone() {
                                        if web_search_ids_completed.contains(&search_id) {
                                            continue;
                                        }
                                        let Some(index) = web_search_index_by_item_id
                                            .get(&search_id)
                                            .copied()
                                        else {
                                            continue;
                                        };
                                        if open_indices.contains(&index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_delta",
                                                &json!({
                                                    "type":"content_block_delta",
                                                    "index":index,
                                                    "delta":{
                                                        "type":"input_json_delta",
                                                        "partial_json":"{}"
                                                    }
                                                }),
                                            ));
                                        }
                                        if open_indices.remove(&index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_stop",
                                                &json!({"type":"content_block_stop","index":index}),
                                            ));
                                        }
                                        web_search_ids_completed.insert(search_id);
                                    }
                                    for search_id in web_search_id_order.clone() {
                                        let index = web_search_result_index_by_id
                                            .remove(&search_id)
                                            .unwrap_or_else(|| {
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                index
                                            });
                                        let results = web_search_results_by_id
                                            .remove(&search_id)
                                            .unwrap_or_default();
                                        let content = web_search_errors_by_id
                                            .remove(&search_id)
                                            .unwrap_or(Value::Array(results));
                                        for event in
                                            web_search_result_events(index, &search_id, content)
                                        {
                                            yield Ok(event);
                                        }
                                    }
                                }

                                for (output_index, item) in terminal_message_items {
                                    let buffered_citations =
                                        preserve_web_search_citations
                                            .then_some(&mut buffered_citation_text);
                                    let missing_text = missing_message_text_parts(
                                        &item,
                                        Some(output_index),
                                        &mut streamed_text,
                                        buffered_citations,
                                    );
                                    if missing_text.is_empty() {
                                        continue;
                                    }
                                    has_substantive_output = true;
                                    let mut reusable_text_index =
                                        terminal_reusable_text_index.take();
                                    if let Some(text_index) = current_text_index.take() {
                                        let was_open = open_indices.remove(&text_index);
                                        if was_open {
                                            yield Ok(anthropic_sse(
                                                "content_block_stop",
                                                &json!({"type":"content_block_stop","index":text_index}),
                                            ));
                                        } else if preserve_web_search_citations {
                                            reusable_text_index.get_or_insert(text_index);
                                        }
                                        if fallback_open_index == Some(text_index) {
                                            fallback_open_index = None;
                                        }
                                    }
                                    for text in missing_text {
                                        let index =
                                            reusable_text_index.take().unwrap_or_else(|| {
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                index
                                            });
                                        for event in text_block_events(index, &text) {
                                            yield Ok(event);
                                        }
                                    }
                                }
                                if preserve_web_search_citations {
                                    let pending_text =
                                        buffered_citation_text.render_pending_parts();
                                    let mut reusable_text_index =
                                        terminal_reusable_text_index.take();
                                    if !pending_text.is_empty() {
                                        has_substantive_output = true;
                                        if let Some(text_index) = current_text_index.take() {
                                            let was_open = open_indices.remove(&text_index);
                                            if was_open {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":text_index}),
                                                ));
                                            } else {
                                                reusable_text_index.get_or_insert(text_index);
                                            }
                                        }
                                    }
                                    for text in pending_text {
                                        let index =
                                            reusable_text_index.take().unwrap_or_else(|| {
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                index
                                            });
                                        for event in text_block_events(index, &text) {
                                            yield Ok(event);
                                        }
                                    }
                                }

                                let terminal_status = response_obj
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .or(match event_name {
                                        "response.incomplete" => Some("incomplete"),
                                        "response.completed" => Some("completed"),
                                        _ => None,
                                    });
                                let stop_reason = map_responses_stop_reason(
                                    terminal_status,
                                    has_tool_use,
                                    response_obj
                                        .pointer("/incomplete_details/reason")
                                        .and_then(|r| r.as_str()),
                                );

                                // Best effort: close any dangling blocks before message_delta/message_stop.
                                if !open_indices.is_empty() {
                                    let mut remaining: Vec<u32> = open_indices.iter().copied().collect();
                                    remaining.sort_unstable();
                                    for index in remaining {
                                        let stop_event = json!({
                                            "type": "content_block_stop",
                                            "index": index
                                        });
                                        let stop_sse = format!("event: content_block_stop\ndata: {}\n\n",
                                            serde_json::to_string(&stop_event).unwrap_or_default());
                                        yield Ok(Bytes::from(stop_sse));
                                        open_indices.remove(&index);
                                    }
                                }
                                fallback_open_index = None;

                                // Defensive: Always build usage_json, even if usage field missing
                                // Some() wrapper with fallback to {} ensures build_anthropic_usage_from_responses
                                // always receives valid input, preventing null pointer errors in VSCode Extension
                                let mut usage_json = build_anthropic_usage_from_responses(
                                    Some(response_obj.get("usage").unwrap_or(&json!({})))
                                );
                                if web_search_count > 0 {
                                    usage_json["server_tool_use"] = json!({
                                        "web_search_requests": web_search_count
                                    });
                                }

                                // Emit message_delta (with usage + stop_reason)
                                let delta_event = json!({
                                    "type": "message_delta",
                                    "delta": {
                                        "stop_reason": stop_reason,
                                        "stop_sequence": null
                                    },
                                    "usage": usage_json
                                });
                                let sse = format!("event: message_delta\ndata: {}\n\n",
                                    serde_json::to_string(&delta_event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_delta");
                                yield Ok(Bytes::from(sse));

                                // Emit message_stop
                                let stop_event = json!({"type": "message_stop"});
                                let stop_sse = format!("event: message_stop\ndata: {}\n\n",
                                    serde_json::to_string(&stop_event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_stop");
                                yield Ok(Bytes::from(stop_sse));
                                terminated = true;
                            }

                            // ================================================
                            // Semantic failures can be carried inside an HTTP 2xx SSE.
                            // Preserve the upstream details instead of silently ending.
                            // ================================================
                            "response.failed" | "error" => {
                                let (message, error_type) = responses_error_details(
                                    &data,
                                    if event_name == "response.failed" {
                                        "Responses upstream reported response.failed"
                                    } else {
                                        "Responses upstream emitted an error event"
                                    },
                                );
                                yield Ok(anthropic_error_sse(&message, &error_type));
                                terminated = true;
                            }

                            // Lifecycle events that don't need Anthropic counterparts.
                            // Listed explicitly so new events trigger a match-completeness review.
                            "response.output_text.done" => {
                                if preserve_web_search_citations {
                                    buffered_citation_text.record_done_event(&data);
                                    continue;
                                }
                                streamed_text.finish_part(&data);
                                if let Some(index) = current_text_index.take() {
                                    if open_indices.remove(&index) {
                                        let stop_event = json!({
                                            "type": "content_block_stop",
                                            "index": index
                                        });
                                        let stop_sse = format!("event: content_block_stop\ndata: {}\n\n",
                                            serde_json::to_string(&stop_event).unwrap_or_default());
                                        yield Ok(Bytes::from(stop_sse));
                                    }
                                    if fallback_open_index == Some(index) {
                                        fallback_open_index = None;
                                    }
                                }
                            }
                            "response.output_item.done" => {
                                let Some(item) = data.get("item") else {
                                    continue;
                                };
                                let item_type = item.get("type").and_then(Value::as_str);
                                if can_preserve_web_search_citations
                                    && item_type == Some("web_search_call")
                                {
                                    preserve_web_search_citations = true;
                                }
                                match item_type {
                                    Some("function_call") => {
                                        has_tool_use = true;
                                        let item_id = item
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .or_else(|| data.get("item_id").and_then(Value::as_str));
                                        let index = item_id
                                            .and_then(|id| tool_index_by_item_id.get(id).copied())
                                            .or_else(|| {
                                                tool_item_key_from_event(&data)
                                                    .and_then(|key| index_by_key.get(&key).copied())
                                            })
                                            .or(last_tool_index);
                                        if let Some(index) = index.filter(|value| open_indices.contains(value)) {
                                            let name = tool_name_by_index
                                                .get(&index)
                                                .map(String::as_str)
                                                .unwrap_or("");
                                            if !tool_had_delta.contains(&index) || name == "Read" {
                                                let raw = item
                                                    .get("arguments")
                                                    .and_then(Value::as_str)
                                                    .filter(|value| !value.is_empty())
                                                    .map(str::to_string)
                                                    .unwrap_or_else(|| {
                                                        tool_args_by_index
                                                            .get(&index)
                                                            .cloned()
                                                            .unwrap_or_default()
                                                    });
                                                let arguments = if name == "Read" {
                                                    sanitize_anthropic_tool_use_input_json(name, &raw)
                                                } else {
                                                    raw
                                                };
                                                if !arguments.is_empty() {
                                                    let event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "input_json_delta",
                                                            "partial_json": arguments
                                                        }
                                                    });
                                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse));
                                                }
                                            }
                                            open_indices.remove(&index);
                                            let event = json!({"type": "content_block_stop", "index": index});
                                            let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                            if let Some(id) = item_id {
                                                tool_index_by_item_id.remove(id);
                                            }
                                            tool_name_by_index.remove(&index);
                                            tool_args_by_index.remove(&index);
                                            tool_had_delta.remove(&index);
                                        }
                                    }
                                    Some("web_search_call") => {
                                        has_substantive_output = true;
                                        if preserve_web_search_citations {
                                            let pending_text =
                                                buffered_citation_text.render_pending_parts();
                                            let mut reusable_text_index = None;
                                            if !pending_text.is_empty() {
                                                if let Some(text_index) =
                                                    current_text_index.take()
                                                {
                                                    let was_open =
                                                        open_indices.remove(&text_index);
                                                    if was_open {
                                                        yield Ok(anthropic_sse(
                                                            "content_block_stop",
                                                            &json!({"type":"content_block_stop","index":text_index}),
                                                        ));
                                                    } else {
                                                        reusable_text_index =
                                                            Some(text_index);
                                                    }
                                                    if fallback_open_index
                                                        == Some(text_index)
                                                    {
                                                        fallback_open_index = None;
                                                    }
                                                }
                                            }
                                            for text in pending_text {
                                                let index = reusable_text_index
                                                    .take()
                                                    .unwrap_or_else(|| {
                                                        let index = next_content_index;
                                                        next_content_index += 1;
                                                        index
                                                    });
                                                for event in text_block_events(index, &text) {
                                                    yield Ok(event);
                                                }
                                            }
                                        }
                                        if !has_sent_message_start {
                                            yield Ok(anthropic_sse(
                                                "message_start",
                                                &json!({
                                                    "type":"message_start",
                                                    "message":{
                                                        "id":message_id.clone().unwrap_or_default(),
                                                        "type":"message",
                                                        "role":"assistant",
                                                        "model":current_model.clone().unwrap_or_default(),
                                                        "usage":{"input_tokens":0,"output_tokens":0}
                                                    }
                                                }),
                                            ));
                                            has_sent_message_start = true;
                                        }

                                        let item_keys = web_search_item_keys(&data, Some(item));
                                        let provisional_index = item_keys
                                            .iter()
                                            .find_map(|key| index_by_key.get(key).copied());
                                        let search_id = provisional_index
                                            .and_then(|index| {
                                                web_search_id_by_index.get(&index).cloned()
                                            })
                                            .or_else(|| {
                                                item.get("id")
                                                    .and_then(Value::as_str)
                                                    .or_else(|| {
                                                        data.get("item_id").and_then(Value::as_str)
                                                    })
                                                    .filter(|id| !id.is_empty())
                                                    .map(ToString::to_string)
                                            })
                                            .or_else(|| {
                                                provisional_index.map(|index| {
                                                    format!("ws_stream_{index}")
                                                })
                                            })
                                            .unwrap_or_else(|| {
                                                format!("ws_stream_{next_content_index}")
                                            });
                                        let disposition = record_web_search_call(
                                            &search_id,
                                            item,
                                            &mut WebSearchRecordState {
                                                ids_seen: &mut web_search_ids_seen,
                                                id_order: &mut web_search_id_order,
                                                results_by_id: &mut web_search_results_by_id,
                                                errors_by_id: &mut web_search_errors_by_id,
                                                request_count: &mut web_search_count,
                                                max_uses: max_web_search_uses,
                                                limit_exceeded_id:
                                                    &mut web_search_limit_exceeded_id,
                                            },
                                        );
                                        if disposition == WebSearchCallDisposition::Ignored {
                                            continue;
                                        }
                                        if web_search_ids_completed.contains(&search_id) {
                                            continue;
                                        }

                                        let index = web_search_index_by_item_id
                                            .get(&search_id)
                                            .copied()
                                            .or(provisional_index)
                                            .unwrap_or_else(|| {
                                                let assigned = next_content_index;
                                                next_content_index += 1;
                                                assigned
                                            });
                                        for key in item_keys {
                                            index_by_key.insert(key, index);
                                        }
                                        web_search_index_by_item_id
                                            .insert(search_id.clone(), index);
                                        web_search_id_by_index.insert(index, search_id.clone());
                                        reserve_web_search_result_index(
                                            &search_id,
                                            &mut web_search_result_index_by_id,
                                            &mut next_content_index,
                                        );

                                        if let Some(text_index) = current_text_index.take() {
                                            if open_indices.remove(&text_index) {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":text_index}),
                                                ));
                                            }
                                            if fallback_open_index == Some(text_index) {
                                                fallback_open_index = None;
                                            }
                                        }

                                        if !open_indices.contains(&index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_start",
                                                &json!({
                                                    "type":"content_block_start",
                                                    "index":index,
                                                    "content_block":{
                                                        "type":"server_tool_use",
                                                        "id":search_id,
                                                        "name":hosted_web_search_name.as_str(),
                                                        "input":{},
                                                        "caller":{"type":"direct"}
                                                    }
                                                }),
                                            ));
                                            open_indices.insert(index);
                                        }

                                        let input = web_search_action_input(item);
                                        yield Ok(anthropic_sse(
                                            "content_block_delta",
                                            &json!({
                                                "type":"content_block_delta",
                                                "index":index,
                                                "delta":{
                                                    "type":"input_json_delta",
                                                    "partial_json":serde_json::to_string(&input)
                                                        .unwrap_or_else(|_| "{}".to_string())
                                                }
                                            }),
                                        ));
                                        if open_indices.remove(&index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_stop",
                                                &json!({"type":"content_block_stop","index":index}),
                                            ));
                                        }
                                        web_search_ids_completed.insert(search_id);
                                        if disposition
                                            == WebSearchCallDisposition::LimitExceeded
                                        {
                                            for event in take_open_web_search_block_stop_events(
                                                &mut open_indices,
                                                &web_search_id_by_index,
                                            ) {
                                                yield Ok(event);
                                            }
                                            for event in take_web_search_result_events(
                                                &web_search_id_order,
                                                &mut web_search_results_by_id,
                                                &mut web_search_errors_by_id,
                                                &mut web_search_result_index_by_id,
                                                &mut next_content_index,
                                            ) {
                                                yield Ok(event);
                                            }
                                            if open_indices.is_empty() {
                                                for event in web_search_limit_stop_events(
                                                    web_search_count,
                                                    has_tool_use,
                                                ) {
                                                    yield Ok(event);
                                                }
                                            } else {
                                                yield Ok(anthropic_error_sse(
                                                    "Responses upstream started a web search beyond max_uses while another content block was incomplete",
                                                    "stream_truncated",
                                                ));
                                            }
                                            terminated = true;
                                            break 'stream_loop;
                                        }
                                    }
                                    Some("reasoning") => {
                                        let item_id = item
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .or_else(|| data.get("item_id").and_then(Value::as_str));
                                        let index = item_id
                                            .and_then(|id| reasoning_index_by_item_id.get(id).copied())
                                            .or_else(|| {
                                                reasoning_item_key(&data, Some(item))
                                                    .and_then(|key| index_by_key.get(&key).copied())
                                            })
                                            .unwrap_or_else(|| {
                                                let assigned = next_content_index;
                                                next_content_index += 1;
                                                assigned
                                            });
                                        reasoning_item_by_index.insert(index, item.clone());

                                        let final_item = reasoning_item_by_index
                                            .get(&index)
                                            .cloned()
                                            .unwrap_or_else(|| item.clone());
                                        let full_text = reasoning_summary_text(&final_item);
                                        let emitted_text = reasoning_text_by_index
                                            .get(&index)
                                            .cloned()
                                            .unwrap_or_default();
                                        if emitted_text.is_empty() && !full_text.is_empty() {
                                            let start_event = json!({
                                                "type": "content_block_start",
                                                "index": index,
                                                "content_block": {"type": "thinking", "thinking": ""}
                                            });
                                            let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                                serde_json::to_string(&start_event).unwrap_or_default());
                                            yield Ok(Bytes::from(start_sse));
                                            open_indices.insert(index);
                                            let delta_event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {"type": "thinking_delta", "thinking": full_text}
                                            });
                                            let delta_sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&delta_event).unwrap_or_default());
                                            yield Ok(Bytes::from(delta_sse));
                                        }

                                        let encrypted = final_item
                                            .get("encrypted_content")
                                            .and_then(Value::as_str)
                                            .is_some_and(|value| !value.is_empty());
                                        if encrypted {
                                            if let Some(envelope) = encode_openai_reasoning_item(&final_item) {
                                                if open_indices.contains(&index) {
                                                    let signature_event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "signature_delta",
                                                            "signature": envelope
                                                        }
                                                    });
                                                    let signature_sse = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&signature_event).unwrap_or_default());
                                                    yield Ok(Bytes::from(signature_sse));
                                                } else {
                                                    let start_event = json!({
                                                        "type": "content_block_start",
                                                        "index": index,
                                                        "content_block": {
                                                            "type": "redacted_thinking",
                                                            "data": envelope
                                                        }
                                                    });
                                                    let start_sse = format!("event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&start_event).unwrap_or_default());
                                                    yield Ok(Bytes::from(start_sse));
                                                    open_indices.insert(index);
                                                }
                                            }
                                        }
                                        if open_indices.remove(&index) {
                                            let stop_event = json!({"type": "content_block_stop", "index": index});
                                            let stop_sse = format!("event: content_block_stop\ndata: {}\n\n",
                                                serde_json::to_string(&stop_event).unwrap_or_default());
                                            yield Ok(Bytes::from(stop_sse));
                                        }
                                        if let Some(id) = item_id {
                                            reasoning_index_by_item_id.remove(id);
                                        }
                                        reasoning_item_by_index.remove(&index);
                                        reasoning_text_by_index.remove(&index);
                                    }
                                    Some("message") => {
                                        let buffered_citations =
                                            preserve_web_search_citations
                                                .then_some(&mut buffered_citation_text);
                                        let missing_text = missing_message_text_parts(
                                            item,
                                            data.get("output_index").and_then(Value::as_u64),
                                            &mut streamed_text,
                                            buffered_citations,
                                        );
                                        if !missing_text.is_empty() {
                                            has_substantive_output = true;
                                            if !has_sent_message_start {
                                                yield Ok(anthropic_sse(
                                                    "message_start",
                                                    &json!({
                                                        "type":"message_start",
                                                        "message":{
                                                            "id":message_id.clone().unwrap_or_default(),
                                                            "type":"message",
                                                            "role":"assistant",
                                                            "model":current_model.clone().unwrap_or_default(),
                                                            "usage":{"input_tokens":0,"output_tokens":0}
                                                        }
                                                    }),
                                            ));
                                            has_sent_message_start = true;
                                        }
                                        let mut reusable_text_index = None;
                                        if let Some(index) = current_text_index.take() {
                                            let was_open = open_indices.remove(&index);
                                            if was_open {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":index}),
                                                ));
                                            } else if preserve_web_search_citations {
                                                reusable_text_index = Some(index);
                                            }
                                            if fallback_open_index == Some(index) {
                                                fallback_open_index = None;
                                            }
                                        }
                                        for text in missing_text {
                                            let index =
                                                reusable_text_index.take().unwrap_or_else(|| {
                                                    let index = next_content_index;
                                                    next_content_index += 1;
                                                    index
                                                });
                                            for event in text_block_events(index, &text) {
                                                yield Ok(event);
                                            }
                                        }
                                        }

                                        let mut new_results = Vec::new();
                                        for result in web_search_results_from_output_item(item) {
                                            let Some(url) = result
                                                .get("url")
                                                .and_then(Value::as_str)
                                            else {
                                                continue;
                                            };
                                            if seen_web_search_result_urls
                                                .insert(url.to_string())
                                            {
                                                new_results.push(result);
                                            }
                                        }
                                        append_unique_web_search_results(
                                            &mut pending_web_search_results,
                                            new_results,
                                        );
                                    }
                                    _ => {}
                                }
                            }
                            "response.reasoning_summary_part.added"
                            | "response.reasoning_summary_part.done"
                            | "response.in_progress" => {}

                            // Any other unknown/future events — silently skip.
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    log::error!("Responses stream error: {e}");
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse));
                    terminated = true;
                    break;
                }
            }
        }

        if !terminated {
            let has_open_tool = open_indices.iter().any(|index| {
                tool_name_by_index.contains_key(index) || tool_args_by_index.contains_key(index)
            });
            // Hosted-search results are intentionally deferred until the
            // response terminal event so final citations can be paired. Any
            // observed search at clean EOF is therefore still structurally
            // unpaired, even if its server_tool_use block was already closed.
            let has_unpaired_server_tool = !web_search_id_order.is_empty();
            let has_open_reasoning = open_indices.iter().any(|index| {
                reasoning_item_by_index.contains_key(index)
                    || reasoning_text_by_index.contains_key(index)
                    || legacy_reasoning_index == Some(*index)
            });

            if has_substantive_output
                && !has_open_tool
                && !has_unpaired_server_tool
                && !has_open_reasoning
            {
                if preserve_web_search_citations {
                    let pending_text = buffered_citation_text.render_pending_parts();
                    let mut reusable_text_index = None;
                    if !pending_text.is_empty() {
                        if let Some(index) = current_text_index.take() {
                            let was_open = open_indices.remove(&index);
                            if was_open {
                                yield Ok(anthropic_sse(
                                    "content_block_stop",
                                    &json!({"type":"content_block_stop","index":index}),
                                ));
                            } else {
                                reusable_text_index = Some(index);
                            }
                        }
                    }
                    for text in pending_text {
                        let index = reusable_text_index.take().unwrap_or_else(|| {
                            let index = next_content_index;
                            next_content_index += 1;
                            index
                        });
                        for event in text_block_events(index, &text) {
                            yield Ok(event);
                        }
                    }
                }
                // Text-only partial output is safe to expose as a max-token style
                // incomplete turn. Close blocks before the terminal events.
                let mut remaining: Vec<u32> = open_indices.iter().copied().collect();
                remaining.sort_unstable();
                for index in remaining {
                    yield Ok(anthropic_sse(
                        "content_block_stop",
                        &json!({"type":"content_block_stop","index":index}),
                    ));
                }
                if !has_sent_message_start {
                    yield Ok(anthropic_sse(
                        "message_start",
                        &json!({
                            "type":"message_start",
                            "message":{
                                "id":message_id.clone().unwrap_or_default(),
                                "type":"message",
                                "role":"assistant",
                                "model":current_model.clone().unwrap_or_default(),
                                "usage":{"input_tokens":0,"output_tokens":0}
                            }
                        }),
                    ));
                }
                yield Ok(anthropic_sse(
                    "message_delta",
                    &json!({
                        "type":"message_delta",
                        "delta":{"stop_reason":"max_tokens","stop_sequence":null},
                        "usage":{"input_tokens":0,"output_tokens":0}
                    }),
                ));
                yield Ok(anthropic_sse("message_stop", &json!({"type":"message_stop"})));
            } else {
                // A truncated tool/reasoning block cannot be safely finalized: tool
                // JSON may be partial and thinking may be missing its signature.
                yield Ok(anthropic_error_sse(
                    "Responses upstream stream ended before a terminal event",
                    "stream_truncated",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;
    use std::collections::HashMap;

    async fn convert_stream_text(input: impl Into<Bytes>) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
        create_anthropic_sse_stream_from_responses(upstream)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect()
    }

    async fn convert_stream_text_with_web_search_name(
        input: impl Into<Bytes>,
        hosted_web_search_name: &str,
    ) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
        create_anthropic_sse_stream_from_responses_with_web_search_options(
            upstream,
            Some(hosted_web_search_name.to_string()),
            None,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect()
    }

    async fn convert_stream_text_with_web_search_limit(
        input: impl Into<Bytes>,
        hosted_web_search_name: &str,
        max_web_search_uses: u64,
    ) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
        create_anthropic_sse_stream_from_responses_with_web_search_options(
            upstream,
            Some(hosted_web_search_name.to_string()),
            Some(max_web_search_uses),
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect()
    }

    fn sse_data_values(output: &str) -> Vec<Value> {
        output
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str(data).ok())
            .collect()
    }

    async fn convert_raw_stream_text_with_web_search(input: impl Into<Bytes>) -> String {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
        create_anthropic_sse_stream_from_responses_raw(
            upstream,
            "web_search".to_string(),
            None,
            true,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect()
    }

    #[test]
    fn test_streamed_text_state_returns_only_the_missing_terminal_suffix() {
        let mut state = StreamedTextState::default();
        let delta = json!({
            "item_id": "msg_partial",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&delta, "Already ");
        state.record_delta(&delta, "streamed");

        assert_eq!(
            state.missing_suffix(
                "Already streamed and completed.",
                Some(0),
                Some("msg_partial"),
                0
            ),
            " and completed."
        );
        assert_eq!(
            state.missing_suffix(
                "Already streamed and completed.",
                Some(0),
                Some("msg_partial"),
                0
            ),
            ""
        );
    }

    #[test]
    fn test_unkeyed_streamed_text_reconciles_with_later_terminal_keys() {
        let mut streamed = StreamedTextState::default();
        streamed.record_delta(&json!({}), "Before search.");
        assert!(streamed.has_part_matching_terminal(
            "Before search.",
            Some(0),
            Some("msg_before"),
            0
        ));

        let item = json!({
            "id": "msg_before",
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": "Before search.",
                "annotations": []
            }]
        });
        let mut buffered = BufferedCitationTextState::default();
        assert!(
            missing_message_text_parts(&item, Some(0), &mut streamed, Some(&mut buffered))
                .is_empty()
        );
    }

    #[test]
    fn test_streamed_text_preserves_keyed_then_unkeyed_delta_order() {
        let mut state = StreamedTextState::default();
        let keyed = json!({
            "item_id": "msg_transition",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Hello ");
        state.record_delta(&json!({}), "world");

        assert_eq!(
            state.missing_suffix("Hello world!", Some(0), Some("msg_transition"), 0),
            "!"
        );
    }

    #[test]
    fn test_streamed_text_scopes_unkeyed_order_after_prior_part_finishes() {
        let mut state = StreamedTextState::default();
        let old = json!({
            "item_id": "msg_old",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&old, "Old.");
        state.finish_part(&old);

        state.record_delta(&json!({}), "Hello ");
        let current = json!({
            "item_id": "msg_current",
            "output_index": 1,
            "content_index": 0
        });
        state.record_delta(&current, "world");

        assert_eq!(
            state.missing_suffix("Hello world!", Some(1), Some("msg_current"), 0),
            "!"
        );
    }

    #[test]
    fn test_streamed_text_binds_output_and_item_aliases_to_one_aggregate() {
        let mut state = StreamedTextState::default();
        state.record_delta(
            &json!({"item_id": "msg_alias", "content_index": 0}),
            "Item ",
        );
        state.record_delta(&json!({"output_index": 3, "content_index": 0}), "output ");
        state.record_delta(
            &json!({
                "item_id": "msg_alias",
                "output_index": 3,
                "content_index": 0
            }),
            "joined",
        );

        assert_eq!(
            state.missing_suffix("Item output joined!", Some(3), Some("msg_alias"), 0),
            "!"
        );
        assert_eq!(
            state.missing_suffix("Item output joined!", None, Some("msg_alias"), 0),
            ""
        );
    }

    #[test]
    fn test_streamed_text_preserves_repeated_deltas_when_aliases_merge() {
        let mut state = StreamedTextState::default();
        state.record_delta(&json!({"output_index": 3, "content_index": 0}), "abc");
        state.record_delta(&json!({"item_id": "msg_repeat", "content_index": 0}), "abc");
        state.record_delta(
            &json!({
                "item_id": "msg_repeat",
                "output_index": 3,
                "content_index": 0
            }),
            "!",
        );

        assert_eq!(
            state.missing_suffix("abcabc! tail", Some(3), Some("msg_repeat"), 0),
            " tail"
        );
    }

    #[test]
    fn test_streamed_text_rejects_crossed_established_aliases() {
        let mut state = StreamedTextState::default();
        let first = json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        });
        let second = json!({
            "item_id": "msg_second",
            "output_index": 1,
            "content_index": 0
        });
        state.record_delta(&first, "First");
        state.record_delta(&second, "Second");
        state.record_delta(
            &json!({
                "item_id": "msg_second",
                "output_index": 0,
                "content_index": 0
            }),
            " crossed",
        );

        assert_eq!(
            state.missing_suffix("First!", Some(0), Some("msg_first"), 0),
            "!"
        );
    }

    #[test]
    fn test_streamed_text_keeps_distinct_unkeyed_tail_after_keyed_terminal_part() {
        let mut state = StreamedTextState::default();
        state.record_delta(
            &json!({
                "item_id": "msg_first",
                "output_index": 0,
                "content_index": 0
            }),
            "First.",
        );
        state.record_delta(&json!({}), "Second.");

        assert_eq!(
            state.missing_suffix("First.", Some(0), Some("msg_first"), 0),
            ""
        );
        assert_eq!(
            state.missing_suffix("Second.", Some(1), Some("msg_second"), 0),
            ""
        );
    }

    #[test]
    fn test_buffered_citations_reject_stale_done_before_next_unkeyed_delta() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});

        state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
        state.record_delta(&unkeyed, "First.");
        state.record_done_event(&json!({"text": "First."}));
        state.record_part(
            &unkeyed,
            &json!({"type": "output_text", "text": "First.", "annotations": []}),
            true,
        );

        state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
        assert!(!state.record_part(
            &unkeyed,
            &json!({"type": "output_text", "text": "First.", "annotations": []}),
            true,
        ));
        state.record_delta(&unkeyed, "Second.");
        state.record_done_event(&json!({"text": "Second."}));

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_preserve_new_unkeyed_substring_after_part_boundary() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});

        state.record_delta(&unkeyed, "foobar");
        state.record_done_event(&json!({"text": "foobar"}));
        assert_eq!(state.render_pending_parts(), vec!["foobar"]);

        state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
        assert!(state.record_part(
            &unkeyed,
            &json!({
                "type": "output_text",
                "text": "bar",
                "annotations": [{
                    "type": "url_citation",
                    "start_index": 0,
                    "end_index": 3,
                    "url": "https://example.com/bar",
                    "title": "Bar"
                }]
            }),
            true,
        ));

        assert_eq!(
            state.render_pending_parts(),
            vec!["[bar](https://example.com/bar)"]
        );
    }

    #[test]
    fn test_buffered_citations_merge_unkeyed_prefix_when_keys_appear() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({}), "Prefix ");
        state.record_delta(&json!({"output_index": 2, "content_index": 0}), "suffix.");

        assert_eq!(state.render_pending_parts(), vec!["Prefix suffix."]);
    }

    #[test]
    fn test_buffered_citations_preserve_unkeyed_then_keyed_part_order() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "First.");
        state.record_done_event(&json!({"text": "First."}));
        state.record_delta(&json!({"output_index": 2, "content_index": 0}), "Second.");

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_do_not_alias_empty_keyed_part_to_prior_unkeyed_part() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "First.");
        state.record_done_event(&json!({"text": "First."}));

        let keyed = json!({"output_index": 2, "content_index": 0});
        state.record_part(&keyed, &json!({"type": "output_text", "text": ""}), false);
        state.record_delta(&keyed, "Second.");

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_merge_item_only_part_when_output_key_appears() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(
            &json!({"item_id": "msg_transition", "content_index": 0}),
            "Full ",
        );
        state.record_delta(
            &json!({
                "item_id": "msg_transition",
                "output_index": 3,
                "content_index": 0
            }),
            "text.",
        );

        assert_eq!(state.render_pending_parts(), vec!["Full text."]);
    }

    #[test]
    fn test_buffered_citations_keep_item_only_parts_in_arrival_order() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({"item_id": "z_item", "content_index": 0}), "First.");
        state
            .record_done_event(&json!({"item_id": "z_item", "content_index": 0, "text": "First."}));
        state.record_delta(&json!({"item_id": "a_item", "content_index": 0}), "Second.");

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_render_fuller_aggregate_than_stale_terminal_snapshot() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_fuller",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Full text.");

        let terminal = json!({"text": "Full", "annotations": []});
        assert_eq!(
            state.render_message_part(&terminal, Some(0), Some("msg_fuller"), 0),
            Some("Full text.".to_string())
        );
        assert!(state.render_pending_parts().is_empty());
    }

    #[test]
    fn test_buffered_citations_preserve_keyed_delta_after_part_was_flushed() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_delayed",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        state.record_delta(&keyed, " after");
        state.record_done_event(&json!({
            "item_id": "msg_delayed",
            "output_index": 0,
            "content_index": 0,
            "text": "Before after"
        }));
        let terminal = json!({"text": "Before", "annotations": []});
        assert_eq!(
            state.render_message_part(&terminal, Some(0), Some("msg_delayed"), 0),
            Some(" after".to_string())
        );
        assert!(state.render_pending_parts().is_empty());
    }

    #[test]
    fn test_buffered_citations_merge_emitted_and_pending_aliases_without_dropping_tail() {
        let mut state = BufferedCitationTextState::default();
        let output_only = json!({"output_index": 0, "content_index": 0});
        state.record_delta(&output_only, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        let item_only = json!({"item_id": "msg_alias", "content_index": 0});
        state.record_delta(&item_only, " after");
        state.record_delta(
            &json!({
                "item_id": "msg_alias",
                "output_index": 0,
                "content_index": 0
            }),
            "!",
        );

        assert_eq!(state.render_pending_parts(), vec![" after!"]);
    }

    #[test]
    fn test_buffered_citations_keep_post_flush_delta_across_stale_done_snapshot() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_stale",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);
        state.record_delta(&keyed, " after");
        state.record_done_event(&json!({
            "item_id": "msg_stale",
            "output_index": 0,
            "content_index": 0,
            "text": "Before"
        }));

        assert_eq!(state.render_pending_parts(), vec![" after"]);
    }

    #[test]
    fn test_buffered_citations_reject_stale_unkeyed_done_after_flush() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        state.record_delta(&unkeyed, " after");
        state.record_done_event(&json!({"text": "Before"}));

        assert_eq!(state.render_pending_parts(), vec![" after"]);
    }

    #[test]
    fn test_buffered_citations_reconcile_cumulative_unkeyed_done_after_flush() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        state.record_delta(&unkeyed, " after");
        state.record_done_event(&json!({"text": "Before after"}));

        assert_eq!(state.render_pending_parts(), vec![" after"]);
    }

    #[test]
    fn test_buffered_citations_ignore_partial_stale_unkeyed_done_after_flush() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        state.record_delta(&unkeyed, " after");
        state.record_done_event(&json!({"text": "Bef"}));

        assert_eq!(state.render_pending_parts(), vec![" after"]);
    }

    #[test]
    fn test_buffered_citations_reject_incompatible_cumulative_unkeyed_done() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({}), "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        state.record_delta(&json!({}), " after");
        state.record_done_event(&json!({"text": "Before stale"}));

        assert_eq!(state.render_pending_parts(), vec![" after"]);
    }

    #[test]
    fn test_buffered_citations_merge_fuller_duplicate_unkeyed_done() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({}), "Before");
        state.record_done_event(&json!({"text": "Before"}));
        state.record_done_event(&json!({"text": "Before after"}));

        assert_eq!(state.render_pending_parts(), vec!["Before after"]);
    }

    #[test]
    fn test_buffered_citations_metadata_only_done_closes_keyed_part() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "First.");
        state.record_done_event(&keyed);
        state.record_delta(&json!({}), "Second.");

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_do_not_adopt_open_part_with_conflicting_keys() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(
            &json!({
                "item_id": "msg_first",
                "output_index": 0,
                "content_index": 0
            }),
            "First.",
        );
        state.record_delta(
            &json!({
                "item_id": "msg_second",
                "output_index": 1,
                "content_index": 0
            }),
            "Second.",
        );

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_preserve_repeated_post_flush_delta() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({"output_index": 0, "content_index": 0});
        state.record_delta(&keyed, "abc");
        assert_eq!(state.render_pending_parts(), vec!["abc"]);

        state.record_delta(&keyed, "abc");
        assert_eq!(state.render_pending_parts(), vec!["abc"]);
    }

    #[test]
    fn test_buffered_citations_preserve_repeated_alias_deltas() {
        let mut state = BufferedCitationTextState::default();
        let output_only = json!({"output_index": 0, "content_index": 0});
        state.record_delta(&output_only, "abc");
        assert_eq!(state.render_pending_parts(), vec!["abc"]);

        state.record_delta(&json!({"item_id": "msg_repeat", "content_index": 0}), "abc");
        state.record_delta(
            &json!({
                "item_id": "msg_repeat",
                "output_index": 0,
                "content_index": 0
            }),
            "!",
        );

        assert_eq!(state.render_pending_parts(), vec!["abc!"]);
    }

    #[test]
    fn test_buffered_citations_empty_delta_does_not_make_snapshots_additive() {
        let mut state = BufferedCitationTextState::default();
        let output_only = json!({"output_index": 0, "content_index": 0});
        assert!(state.record_text(&output_only, "abc"));
        state.record_delta(&output_only, "");

        let item_only = json!({"item_id": "msg_snapshot", "content_index": 0});
        state.record_delta(&item_only, "abc");
        let both = json!({
            "item_id": "msg_snapshot",
            "output_index": 0,
            "content_index": 0
        });
        assert!(state.record_text(&both, "abc"));

        assert_eq!(state.render_pending_parts(), vec!["abc"]);
    }

    #[test]
    fn test_buffered_citations_do_not_reuse_last_unkeyed_part_for_conflicting_snapshot_key() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({}), "Same.");
        assert!(state.record_text(
            &json!({
                "item_id": "msg_first",
                "output_index": 0,
                "content_index": 0
            }),
            "Same."
        ));
        assert!(state.record_text(
            &json!({
                "item_id": "msg_second",
                "output_index": 1,
                "content_index": 0
            }),
            "Same."
        ));

        assert_eq!(state.render_pending_parts(), vec!["Same.", "Same."]);
    }

    #[test]
    fn test_buffered_citations_reject_crossed_established_aliases() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(
            &json!({
                "item_id": "msg_first",
                "output_index": 0,
                "content_index": 0
            }),
            "First.",
        );
        state.record_done_event(&json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0,
            "text": "First."
        }));
        state.record_delta(
            &json!({
                "item_id": "msg_second",
                "output_index": 1,
                "content_index": 0
            }),
            "Second.",
        );
        state.record_delta(
            &json!({
                "item_id": "msg_second",
                "output_index": 0,
                "content_index": 0
            }),
            "Crossed.",
        );

        assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
    }

    #[test]
    fn test_buffered_citations_reconcile_fuller_terminal_after_keyed_flush() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_terminal",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        let terminal = json!({
            "text": "Before after",
            "annotations": [{
                "type": "url_citation",
                "start_index": 7,
                "end_index": 12,
                "url": "https://example.com/after",
                "title": "After"
            }]
        });
        let rendered = state
            .render_message_part(&terminal, Some(0), Some("msg_terminal"), 0)
            .expect("fuller terminal suffix and citation should be emitted");
        assert!(rendered.starts_with(" after"));
        assert!(rendered.contains("https://example.com/after"));
    }

    #[test]
    fn test_buffered_citations_consume_pending_unkeyed_suffix_after_emitted_history() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "First");
        assert_eq!(state.render_pending_parts(), vec!["First"]);
        state.record_delta(&unkeyed, "Second");

        let terminal = json!({"text": "FirstSecond", "annotations": []});
        assert_eq!(
            state.render_message_part(&terminal, Some(0), Some("msg_combined"), 0),
            Some("Second".to_string())
        );
        assert!(state.render_pending_parts().is_empty());
    }

    #[test]
    fn test_buffered_citations_do_not_discard_keyed_tail_for_stale_unkeyed_snapshot() {
        let mut state = BufferedCitationTextState::default();
        state.record_delta(&json!({}), "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"]);

        let keyed = json!({
            "item_id": "msg_tail",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, " after");
        let stale = json!({"text": "Before", "annotations": []});
        assert_eq!(
            state.render_message_part(&stale, Some(0), Some("msg_tail"), 0),
            Some(" after".to_string())
        );

        let complete = json!({"text": "Before after", "annotations": []});
        assert_eq!(
            state.render_message_part(&complete, Some(0), Some("msg_tail"), 0),
            None
        );
    }

    #[test]
    fn test_buffered_citations_render_later_unkeyed_text_after_keyed_part() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({"output_index": 0, "content_index": 0});
        state.record_delta(&keyed, "Keyed.");
        assert_eq!(state.render_pending_parts(), vec!["Keyed."]);

        let unkeyed = json!({});
        state.record_delta(&unkeyed, "Rust docs.");
        state.record_annotation(
            &unkeyed,
            &json!({
                "type": "url_citation",
                "start_index": 0,
                "end_index": 4,
                "url": "https://www.rust-lang.org/",
                "title": "Rust"
            }),
        );

        assert_eq!(
            state.render_pending_parts(),
            vec!["[Rust](https://www.rust-lang.org/) docs."]
        );
    }

    #[test]
    fn test_buffered_citations_do_not_replay_emitted_unkeyed_history() {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "First.");
        assert_eq!(state.render_pending_parts(), vec!["First."]);

        state.record_delta(&unkeyed, "Second.");
        let first = json!({"text": "First.", "annotations": []});
        assert_eq!(
            state.render_message_part(&first, Some(0), Some("msg_first"), 0),
            None
        );
        let second = json!({"text": "Second.", "annotations": []});
        assert_eq!(
            state.render_message_part(&second, Some(1), Some("msg_second"), 0),
            Some("Second.".to_string())
        );
        assert!(state.render_pending_parts().is_empty());
    }

    #[test]
    fn test_buffered_citations_filter_citations_emitted_with_unkeyed_history() {
        let mut state = BufferedCitationTextState::default();
        let already_emitted = json!({
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://www.rust-lang.org/",
            "title": "Rust"
        });
        state.record_delta(&json!({}), "Rust");
        state.record_annotation(&json!({}), &already_emitted);
        assert_eq!(
            state.render_pending_parts(),
            vec!["[Rust](https://www.rust-lang.org/)"]
        );

        let terminal_only = json!({
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://doc.rust-lang.org/",
            "title": "Rust docs"
        });
        let terminal = json!({
            "text": "Rust",
            "annotations": [already_emitted, terminal_only]
        });
        let rendered = state
            .render_message_part(&terminal, Some(0), Some("msg_rust"), 0)
            .expect("the terminal-only citation should still be emitted");
        assert!(!rendered.contains("https://www.rust-lang.org/"));
        assert!(rendered.contains("https://doc.rust-lang.org/"));
    }

    #[test]
    fn test_buffered_citations_keep_same_citation_on_distinct_keyed_tail() {
        let mut state = BufferedCitationTextState::default();
        let citation = json!({
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://www.rust-lang.org/",
            "title": "Rust"
        });
        state.record_delta(&json!({}), "Rust");
        state.record_annotation(&json!({}), &citation);
        assert_eq!(
            state.render_pending_parts(),
            vec!["[Rust](https://www.rust-lang.org/)"]
        );

        let keyed = json!({
            "item_id": "msg_tail",
            "output_index": 0,
            "content_index": 0
        });
        state.record_delta(&keyed, "Rust");
        let terminal = json!({"text": "Rust", "annotations": [citation]});
        assert_eq!(
            state.render_message_part(&terminal, Some(0), Some("msg_tail"), 0),
            Some("[Rust](https://www.rust-lang.org/)".to_string())
        );
    }

    #[test]
    fn test_buffered_citations_collects_only_pending_keyed_annotations() {
        let mut state = BufferedCitationTextState::default();
        let keyed = json!({
            "item_id": "msg_annotation",
            "output_index": 0,
            "content_index": 0
        });
        let citation = json!({
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://www.rust-lang.org/",
            "title": "Rust"
        });
        state.record_annotation(&keyed, &citation);
        assert_eq!(
            state.render_pending_parts(),
            vec!["Sources: [Rust](https://www.rust-lang.org/)"]
        );

        let terminal = json!({"text": "Rust", "annotations": [citation]});
        assert!(state
            .collect_annotations(
                Some((0, 0)),
                Some(&("msg_annotation".to_string(), 0)),
                Some(&terminal)
            )
            .is_empty());
    }

    #[test]
    fn test_buffered_citations_retain_terminal_only_part_for_late_enrichment() {
        let mut state = BufferedCitationTextState::default();
        let first = json!({"text": "Before", "annotations": []});
        assert_eq!(
            state.render_message_part(&first, Some(0), Some("msg_terminal_only"), 0),
            Some("Before".to_string())
        );

        let enriched = json!({
            "text": "Before after",
            "annotations": [{
                "type": "url_citation",
                "start_index": 7,
                "end_index": 12,
                "url": "https://example.com/after",
                "title": "After"
            }]
        });
        let rendered = state
            .render_message_part(&enriched, Some(0), Some("msg_terminal_only"), 0)
            .expect("late suffix and citation should be emitted");
        assert!(rendered.starts_with(" after"));
        assert!(rendered.contains("https://example.com/after"));
    }

    #[test]
    fn test_buffered_citations_win_over_unrelated_pre_search_unkeyed_text() {
        let mut streamed = StreamedTextState::default();
        streamed.record_delta(&json!({}), "Before search.");

        let mut buffered = BufferedCitationTextState::default();
        buffered.record_delta(
            &json!({
                "item_id": "msg_after",
                "output_index": 2,
                "content_index": 0
            }),
            "After search.",
        );
        let item = json!({
            "id": "msg_after",
            "type": "message",
            "content": [{
                "type": "output_text",
                "text": "After search.",
                "annotations": []
            }]
        });

        assert_eq!(
            missing_message_text_parts(&item, Some(2), &mut streamed, Some(&mut buffered)),
            vec!["After search."]
        );
        assert_eq!(
            streamed.missing_suffix("Before search.", Some(0), Some("msg_before"), 0),
            ""
        );
    }

    #[test]
    fn test_map_responses_stop_reason_tool_use() {
        assert_eq!(
            map_responses_stop_reason(Some("completed"), true, None),
            Some("tool_use")
        );
        assert_eq!(
            map_responses_stop_reason(Some("completed"), false, None),
            Some("end_turn")
        );
        assert_eq!(
            map_responses_stop_reason(Some("incomplete"), false, Some("max_output_tokens")),
            Some("max_tokens")
        );
        assert_eq!(
            map_responses_stop_reason(Some("incomplete"), false, Some("content_filter")),
            Some("end_turn")
        );
    }

    #[test]
    fn test_response_object_from_event_with_wrapper() {
        let data = json!({
            "type": "response.created",
            "response": {
                "id": "resp_1",
                "model": "gpt-4o"
            }
        });
        let obj = response_object_from_event(&data);
        assert_eq!(obj["id"], "resp_1");
        assert_eq!(obj["model"], "gpt-4o");
    }

    #[tokio::test]
    async fn test_response_failed_event_becomes_anthropic_error() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"type\":\"server_error\",\"message\":\"backend exploded\"}}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(merged.contains("backend exploded"));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_late_delta_after_failure_does_not_emit_message_start() {
        let input = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"boom\"}}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"too late\"}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(!merged.contains("event: message_start"));
        assert!(!merged.contains("too late"));
    }

    #[tokio::test]
    async fn test_completed_event_with_failed_status_is_error() {
        let input = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\",\"error\":{\"type\":\"server_error\",\"message\":\"failed wrapper\"},\"output\":[]}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(merged.contains("failed wrapper"));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_response_incomplete_event_terminates_with_max_tokens() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":10,\"output_tokens\":3}}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
        assert!(merged.contains("event: message_stop"));
        assert!(!merged.contains("event: error"));
    }

    #[tokio::test]
    async fn test_response_incomplete_event_without_status_uses_event_fallback() {
        let input = concat!(
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"usage\":{\"output_tokens\":3}}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_hosted_web_search_emits_anthropic_server_tool_blocks() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_search\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_123\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_123\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust official documentation\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\"}]}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"output_index\":1,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"output_index\":1,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":12}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
        let events = sse_data_values(&merged);
        let content_block_types: Vec<&str> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter_map(|block| block.get("type").and_then(Value::as_str))
            .collect();
        assert_eq!(
            content_block_types,
            vec!["server_tool_use", "web_search_tool_result", "text"]
        );
        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
        assert_eq!(
            merged
                .matches("\"type\":\"web_search_tool_result\"")
                .count(),
            1
        );
        assert!(merged.contains("\"id\":\"ws_123\""));
        assert!(merged.contains("\"name\":\"web_search_next\""));
        assert!(merged
            .contains("\"partial_json\":\"{\\\"query\\\":\\\"Rust official documentation\\\"}\""));
        assert!(merged.contains("https://doc.rust-lang.org/"));
        assert!(merged.contains("\"title\":\"Rust Documentation\""));
        let text_deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
            .collect();
        assert_eq!(
            text_deltas,
            vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
        );
        assert!(merged.contains("\"stop_reason\":\"end_turn\""));
        assert!(!merged.contains("\"stop_reason\":\"tool_use\""));
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_buffered_hosted_web_search_text_emits_keepalive_ping() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_keepalive\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_keepalive\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_keepalive\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Still synthesizing the answer.\"}\n\n"
        );

        let merged = convert_raw_stream_text_with_web_search(input).await;

        assert_eq!(merged.matches("event: ping").count(), 1);
        assert!(!merged.contains("\"type\":\"text_delta\""));
    }

    #[tokio::test]
    async fn test_terminal_search_recovery_reuses_reserved_text_index() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_recovery\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_recovery\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}},{\"id\":\"msg_recovery\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
        let events = sse_data_values(&merged);
        let started_indices: Vec<u64> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
            })
            .filter_map(|event| event.get("index").and_then(Value::as_u64))
            .collect();
        let text_deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
            .collect();

        assert_eq!(started_indices, vec![0, 1, 2]);
        assert_eq!(
            text_deltas,
            vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
        );
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_terminal_search_reconciles_unkeyed_buffered_text() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unkeyed\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Rust docs are online.\"}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_unkeyed\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}},{\"id\":\"msg_unkeyed\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(
            text_deltas,
            vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
        );
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_terminal_search_partitions_unkeyed_citations_across_text_parts() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unkeyed_parts\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust and Cargo docs\"}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Rust docs.\"}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Cargo docs.\"}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"text\":\"Cargo docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Cargo docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_unkeyed_parts\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust and Cargo docs\"}},{\"id\":\"msg_unkeyed_parts\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[]},{\"type\":\"output_text\",\"text\":\"Cargo docs.\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(
            text_deltas,
            vec![
                "[Rust](https://www.rust-lang.org/) docs.",
                "[Cargo](https://doc.rust-lang.org/cargo/) docs."
            ]
        );
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_text_streamed_before_search_is_not_replayed_after_buffering_starts() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_transition\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"delta\":\"Before search.\"}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"text\":\"Before search.\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
            "event: response.output_text.annotation.added\n",
            "data: {\"type\":\"response.output_text.annotation.added\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"msg_after\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_transition\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_before\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Before search.\",\"annotations\":[]}]},{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}},{\"id\":\"msg_after\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":8}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(
            text_deltas,
            vec![
                "Before search.",
                "[Rust docs](https://doc.rust-lang.org/) are online."
            ]
        );
    }

    #[tokio::test]
    async fn test_streaming_hosted_web_search_preserves_failed_result() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_failed_search\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"failed\",\"action\":{\"type\":\"search\",\"query\":\"latest docs\"},\"error\":{\"code\":\"invalid_search_query\"}}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_failed_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
        let events = sse_data_values(&merged);
        let result = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .find(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .unwrap();

        assert_eq!(result["tool_use_id"], "ws_failed");
        assert_eq!(
            result["content"],
            json!({
                "type": "web_search_tool_result_error",
                "error_code": "invalid_tool_input"
            })
        );
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_incomplete_hosted_web_search_emits_error_result() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unfinished_search\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unfinished\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_unfinished_search\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
        let events = sse_data_values(&merged);
        let result = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .find(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .unwrap();

        assert_eq!(result["tool_use_id"], "ws_unfinished");
        assert_eq!(
            result["content"],
            json!({
                "type": "web_search_tool_result_error",
                "error_code": "unavailable"
            })
        );
        assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_hosted_web_search_enforces_max_uses() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_search_limit\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed query\",\"sources\":[{\"type\":\"url\",\"url\":\"https://example.com/allowed\",\"title\":\"Allowed\"}]}}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"must not leak\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_search_limit\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":3}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
        assert_eq!(result_blocks.len(), 2);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_allowed");
        assert_eq!(result_blocks[0]["content"][0]["title"], "Allowed");
        assert_eq!(result_blocks[1]["tool_use_id"], "ws_over_limit");
        assert_eq!(
            result_blocks[1]["content"],
            json!({
                "type": "web_search_tool_result_error",
                "error_code": "max_uses_exceeded"
            })
        );
        assert!(!merged.contains("must not leak"));
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_post_search_activity_preserves_mixed_block_order_and_emits_ping() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_mixed_order\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_order\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_order\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Search answer before function.\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"fc_after_search\",\"type\":\"function_call\",\"call_id\":\"call_after_search\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_after_search\",\"output_index\":2,\"arguments\":\"{\\\"query\\\":\\\"rust\\\"}\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_mixed_order\",\"status\":\"completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
        let events = sse_data_values(&merged);
        let starts: Vec<(&str, u64)> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
            })
            .filter_map(|event| {
                Some((
                    event.pointer("/content_block/type")?.as_str()?,
                    event.get("index")?.as_u64()?,
                ))
            })
            .collect();

        assert_eq!(
            starts,
            vec![
                ("server_tool_use", 0),
                ("web_search_tool_result", 1),
                ("text", 2),
                ("tool_use", 3),
            ]
        );
        assert!(events
            .iter()
            .any(|event| event.get("type").and_then(Value::as_str) == Some("ping")));
        assert!(events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("tool_use")
        }));
    }

    #[tokio::test]
    async fn test_streaming_web_search_done_with_late_id_reuses_output_index_identity() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_late_id\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_late_id\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_late_id\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
        assert_eq!(result_blocks.len(), 1);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_stream_0");
        assert_eq!(
            result_blocks[0]["content"][0]["url"],
            "https://doc.rust-lang.org/"
        );
        assert!(!merged.contains("max_uses_exceeded"));
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_web_search_limit_closes_parallel_in_flight_blocks() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_limit\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_in_flight\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);
        let mut started_indices: Vec<u64> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
            })
            .filter_map(|event| event.get("index").and_then(Value::as_u64))
            .collect();
        let mut stopped_indices: Vec<u64> = events
            .iter()
            .filter(|event| event.get("type").and_then(Value::as_str) == Some("content_block_stop"))
            .filter_map(|event| event.get("index").and_then(Value::as_u64))
            .collect();
        started_indices.sort_unstable();
        stopped_indices.sort_unstable();

        assert_eq!(started_indices, stopped_indices);
        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
        assert!(merged.contains("\"error_code\":\"unavailable\""));
        assert!(merged.contains("\"error_code\":\"max_uses_exceeded\""));
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_web_search_limit_does_not_finalize_parallel_function() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_function_limit\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_open\",\"type\":\"function_call\",\"call_id\":\"call_open\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_open\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);
        let function_index = events
            .iter()
            .find(|event| {
                event.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use")
            })
            .and_then(|event| event.get("index"))
            .and_then(Value::as_u64)
            .unwrap();

        assert!(!events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("content_block_stop")
                && event.get("index").and_then(Value::as_u64) == Some(function_index)
        }));
        assert!(events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("error")
                && event.pointer("/error/type").and_then(Value::as_str) == Some("stream_truncated")
        }));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_web_search_limit_preserves_completed_function_stop_reason() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_completed_function_limit\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"call_id\":\"call_done\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_done\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"rust\\\"}\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);

        assert!(events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("tool_use")
        }));
        assert!(!events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("end_turn")
        }));
        assert!(merged.contains("event: message_stop"));
        assert!(!merged.contains("stream_truncated"));
    }

    #[tokio::test]
    async fn test_streaming_done_only_search_limit_does_not_finalize_parallel_reasoning() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_reasoning_limit\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_open\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
            "event: response.reasoning_summary_part.added\n",
            "data: {\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"rs_open\",\"output_index\":0,\"summary_index\":0,\"part\":{\"type\":\"summary_text\",\"text\":\"\"}}\n\n",
            "event: response.reasoning_summary_text.delta\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_open\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Still reasoning\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"over limit\"}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
        let events = sse_data_values(&merged);
        let reasoning_index = events
            .iter()
            .find(|event| {
                event.pointer("/content_block/type").and_then(Value::as_str) == Some("thinking")
            })
            .and_then(|event| event.get("index"))
            .and_then(Value::as_u64)
            .unwrap();

        assert!(!events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("content_block_stop")
                && event.get("index").and_then(Value::as_u64) == Some(reasoning_index)
        }));
        assert!(events.iter().any(|event| {
            event.get("type").and_then(Value::as_str) == Some("error")
                && event.pointer("/error/type").and_then(Value::as_str) == Some("stream_truncated")
        }));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_failed_search_sources_do_not_consume_successful_search_citations() {
        let input = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_shared_source\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"failed\",\"action\":{\"type\":\"search\",\"query\":\"failed query\",\"sources\":[{\"type\":\"url\",\"url\":\"https://example.com/shared\",\"title\":\"Failed source\"}]},\"error\":{\"code\":\"invalid_search_query\"}},{\"id\":\"ws_success\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"successful query\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Successful answer.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://example.com/shared\",\"title\":\"Successful citation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(result_blocks.len(), 2);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_failed");
        assert_eq!(
            result_blocks[0]["content"]["type"],
            "web_search_tool_result_error"
        );
        assert_eq!(result_blocks[1]["tool_use_id"], "ws_success");
        assert_eq!(
            result_blocks[1]["content"][0]["url"],
            "https://example.com/shared"
        );
        assert!(merged.contains("\"web_search_requests\":2"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_terminal_web_search_reuses_stream_synthesized_id() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_no_search_id\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_no_search_id\",\"status\":\"completed\",\"output\":[{\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
        assert_eq!(result_blocks.len(), 1);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_stream_0");
        assert!(merged.contains("\"web_search_requests\":1"));
    }

    #[tokio::test]
    async fn test_clean_eof_after_completed_web_search_is_truncated() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_truncated_search\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_truncated\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_truncated\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(merged.contains("stream_truncated"));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_hosted_web_search_pairs_every_call_with_its_sources() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_multi_search\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_rust\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_rust\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust language\",\"sources\":[{\"type\":\"url\",\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_cargo\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_cargo\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Cargo documentation\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":2,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":2,\"content_index\":0,\"delta\":\"Rust and Cargo have official documentation.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"msg_multi\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust and Cargo have official documentation.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"},{\"type\":\"url_citation\",\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":14}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
        assert_eq!(result_blocks.len(), 2);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_rust");
        assert_eq!(
            result_blocks[0]["content"][0]["url"],
            "https://www.rust-lang.org/"
        );
        assert_eq!(result_blocks[1]["tool_use_id"], "ws_cargo");
        assert_eq!(
            result_blocks[1]["content"][0]["url"],
            "https://doc.rust-lang.org/cargo/"
        );
        assert!(merged.contains("\"web_search_requests\":2"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_hosted_web_search_pairs_calls_without_sources() {
        let input = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi_fallback\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_first\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"first query\"}},{\"id\":\"ws_second\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"second query\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Combined answer.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":9,\"end_index\":15,\"url\":\"https://example.com/result\",\"title\":\"Combined result\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
        let events = sse_data_values(&merged);
        let result_blocks: Vec<&Value> = events
            .iter()
            .filter_map(|event| event.get("content_block"))
            .filter(|block| {
                block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
            })
            .collect();

        assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
        assert_eq!(result_blocks.len(), 2);
        assert_eq!(result_blocks[0]["tool_use_id"], "ws_first");
        assert_eq!(result_blocks[0]["content"], json!([]));
        assert_eq!(result_blocks[1]["tool_use_id"], "ws_second");
        assert_eq!(
            result_blocks[1]["content"][0]["url"],
            "https://example.com/result"
        );
        let text_deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
            .collect();
        assert_eq!(
            text_deltas,
            vec!["Combined [answer](https://example.com/result)."]
        );
        assert!(merged.contains("\"web_search_requests\":2"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_terminal_output_does_not_duplicate_streamed_text() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_text\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"delta\":\"Already streamed.\"}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"text\":\"Already streamed.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Already streamed.\",\"annotations\":[]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_text\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Already streamed.\",\"annotations\":[]}]}]}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(text_deltas, vec!["Already streamed."]);
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_output_item_done_emits_text_when_deltas_are_missing() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_done_text\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_done_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Recovered from the completed item.\",\"annotations\":[]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done_text\",\"model\":\"gpt-5.6\",\"status\":\"completed\"}}\n\n"
        );

        let merged = convert_stream_text(input).await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(text_deltas, vec!["Recovered from the completed item."]);
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_final_event_without_blank_line_is_processed() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("\"stop_reason\":\"end_turn\""));
        assert_eq!(merged.matches("event: message_stop").count(), 1);
        assert!(!merged.contains("stream_truncated"));
    }

    #[tokio::test]
    async fn test_clean_eof_after_partial_text_is_explicitly_incomplete() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
        assert!(merged.contains("event: content_block_stop"));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_hosted_web_search_option_does_not_buffer_without_search() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\"}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
        let text_deltas: Vec<String> = sse_data_values(&merged)
            .into_iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(text_deltas, vec!["partial"]);
        assert!(!merged.contains("event: ping"));
        assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_clean_eof_during_tool_arguments_is_error() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"exec\",\"delta\":\"{\\\"cmd\\\":\"}\n\n"
        );

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(merged.contains("stream_truncated"));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_stream_request_with_complete_json_response_is_converted() {
        let input = r#"{
            "id":"resp_json",
            "status":"completed",
            "model":"gpt-5",
            "output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]}],
            "usage":{"input_tokens":4,"output_tokens":1}
        }"#;

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: message_start"));
        assert!(merged.contains("\"text\":\"hello\""));
        assert!(merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_stream_request_with_failed_json_response_is_error() {
        let input = r#"{
            "id":"resp_json",
            "status":"failed",
            "error":{"type":"server_error","message":"json backend failed"},
            "output":[]
        }"#;

        let merged = convert_stream_text(input).await;
        assert!(merged.contains("event: error"));
        assert!(merged.contains("json backend failed"));
        assert!(!merged.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn test_streaming_conversion_with_wrapped_response_events() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-4o\",\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_weather\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":3}}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"type\":\"message_start\""));
        assert!(merged.contains("\"id\":\"resp_1\""));
        assert!(merged.contains("\"model\":\"gpt-4o\""));
        assert!(merged.contains("\"type\":\"tool_use\""));
        assert!(merged.contains("\"name\":\"get_weather\""));
        assert!(merged.contains("\"type\":\"input_json_delta\""));
        assert!(merged.contains("\"stop_reason\":\"tool_use\""));
        assert!(merged.contains("\"input_tokens\":12"));
        assert!(merged.contains("\"output_tokens\":3"));
        assert!(merged.contains("\"type\":\"message_stop\""));
    }

    #[tokio::test]
    async fn test_streaming_read_tool_drops_empty_pages() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_read\",\"model\":\"gpt-5.5\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_read\",\"delta\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_read\",\"arguments\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"name\":\"Read\""));
        assert!(merged.contains("\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0}"));
        assert!(!merged.contains("\\\"pages\\\":\\\"\\\""));
    }

    #[tokio::test]
    async fn test_streaming_read_tool_duplicate_start_preserves_buffered_args() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_read\",\"model\":\"gpt-5.5\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_read\",\"delta\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_read\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert_eq!(merged.matches("event: content_block_start").count(), 1);
        assert_eq!(merged.matches("event: content_block_stop").count(), 1);
        assert!(merged.contains("\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0}"));
        assert!(!merged.contains("\\\"pages\\\":\\\"\\\""));
    }

    #[tokio::test]
    async fn test_streaming_conversion_interleaved_tool_deltas_by_item_id() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_2\",\"model\":\"gpt-4o\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"first_tool\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_2\",\"name\":\"second_tool\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_2\",\"delta\":\"{\\\"b\\\":2}\"}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{\\\"a\\\":1}\"}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\"}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_2\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":4}}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let mut tool_index_by_call: HashMap<String, u64> = HashMap::new();
        for event in &events {
            if event.get("type").and_then(|v| v.as_str()) == Some("content_block_start") {
                let cb = event.get("content_block");
                if cb.and_then(|v| v.get("type")).and_then(|v| v.as_str()) == Some("tool_use") {
                    if let (Some(call_id), Some(index)) = (
                        cb.and_then(|v| v.get("id")).and_then(|v| v.as_str()),
                        event.get("index").and_then(|v| v.as_u64()),
                    ) {
                        tool_index_by_call.insert(call_id.to_string(), index);
                    }
                }
            }
        }

        let delta_indices: Vec<u64> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| event.get("index").and_then(|v| v.as_u64()))
            .collect();

        assert_eq!(delta_indices.len(), 2);
        assert_eq!(delta_indices[0], *tool_index_by_call.get("call_2").unwrap());
        assert_eq!(delta_indices[1], *tool_index_by_call.get("call_1").unwrap());
        assert_ne!(
            tool_index_by_call.get("call_1"),
            tool_index_by_call.get("call_2")
        );
    }

    #[tokio::test]
    async fn test_streaming_tool_done_arguments_fallback_without_deltas() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_done\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"call_id\":\"call_done\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.done\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_done\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"arguments\":\"{\\\"q\\\":\\\"rust\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input))]);
        let merged = create_anthropic_sse_stream_from_responses(upstream)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"partial_json\":\"{\\\"q\\\":\\\"rust\\\"}\""));
        assert_eq!(merged.matches("event: content_block_stop").count(), 1);
    }

    #[tokio::test]
    async fn test_official_reasoning_events_emit_signature_before_stop() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_reason\",\"model\":\"gpt-5.6\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
            "event: response.reasoning_summary_part.added\n",
            "data: {\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"part\":{\"type\":\"summary_text\",\"text\":\"\"}}\n\n",
            "event: response.reasoning_summary_text.delta\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Need a tool.\"}\n\n",
            "event: response.reasoning_summary_text.done\n",
            "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"text\":\"Need a tool.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Need a tool.\"}],\"encrypted_content\":\"opaque\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input))]);
        let merged = create_anthropic_sse_stream_from_responses(upstream)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"type\":\"thinking_delta\""));
        assert!(merged.contains("\"type\":\"signature_delta\""));
        let signature_position = merged.find("signature_delta").unwrap();
        let stop_position = merged.find("event: content_block_stop").unwrap();
        assert!(signature_position < stop_position);
        assert!(!merged[stop_position..].contains("content_block_delta"));
    }

    #[tokio::test]
    async fn test_streaming_reasoning_delta_emits_thinking_blocks() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_r\",\"model\":\"o3\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "event: response.reasoning.delta\n",
            "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"Let me \"}\n\n",
            "event: response.reasoning.delta\n",
            "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"think...\"}\n\n",
            "event: response.reasoning.done\n",
            "data: {\"type\":\"response.reasoning.done\"}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"42\",\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":10}}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        // Should contain thinking block start, thinking delta, and text content
        assert!(
            merged.contains("\"type\":\"thinking\""),
            "should emit thinking content_block_start"
        );
        assert!(
            merged.contains("\"type\":\"thinking_delta\""),
            "should emit thinking_delta"
        );
        assert!(
            merged.contains("\"thinking\":\"Let me \"")
                && merged.contains("\"thinking\":\"think...\""),
            "should contain both thinking deltas"
        );
        assert!(
            merged.contains("\"type\":\"text_delta\""),
            "should also emit text content"
        );
        assert!(
            merged.contains("\"text\":\"42\""),
            "should contain text delta"
        );
        assert!(merged.contains("\"stop_reason\":\"end_turn\""));

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                block
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .and_then(|data| serde_json::from_str(data).ok())
            })
            .collect();
        let thinking_starts: Vec<&Value> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
                    && event.pointer("/content_block/type").and_then(Value::as_str)
                        == Some("thinking")
            })
            .collect();
        assert_eq!(
            thinking_starts.len(),
            1,
            "keyless deltas must share one block"
        );
        let thinking_index = thinking_starts[0]
            .get("index")
            .and_then(Value::as_u64)
            .unwrap();
        let thinking_delta_indices: Vec<u64> = events
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("thinking_delta")
            })
            .filter_map(|event| event.get("index").and_then(Value::as_u64))
            .collect();
        assert_eq!(thinking_delta_indices, vec![thinking_index, thinking_index]);

        let stop_position = events
            .iter()
            .position(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_stop")
                    && event.get("index").and_then(Value::as_u64) == Some(thinking_index)
            })
            .expect("legacy reasoning done must close the thinking block");
        let text_start_position = events
            .iter()
            .position(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
                    && event.pointer("/content_block/type").and_then(Value::as_str) == Some("text")
            })
            .expect("text block must start");
        assert!(stop_position < text_start_position);
    }

    #[tokio::test]
    async fn test_streaming_text_parts_are_merged_into_one_text_block() {
        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_merge\",\"model\":\"gpt-5.4\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\",\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":0}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":1}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"好\",\"output_index\":0,\"content_index\":1}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":1}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"output_index\":0,\"content_index\":1}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let events: Vec<Value> = chunks
            .into_iter()
            .flat_map(|chunk| {
                let bytes = chunk.unwrap();
                let text = String::from_utf8_lossy(bytes.as_ref()).to_string();
                text.split("\n\n")
                    .filter_map(|block| {
                        block.lines().find_map(|line| {
                            strip_sse_field(line, "data")
                                .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let text_starts = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("text")
            })
            .count();
        let text_stops = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_stop")
            })
            .count();
        let text_deltas: Vec<String> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("text_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/text")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
            })
            .collect();

        assert_eq!(text_starts, 1);
        assert_eq!(text_stops, 1);
        assert_eq!(text_deltas, vec!["你".to_string(), "好".to_string()]);
    }

    #[tokio::test]
    async fn test_streaming_responses_chinese_split_across_chunks_no_replacement_chars() {
        // Chinese text delta split across two TCP chunks.
        let full = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_cn\",\"model\":\"gpt-4o\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好世界\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":4}}}\n\n"
        );
        let bytes = full.as_bytes();

        // Find "你" and split inside it
        let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let split_point = ni_start + 2; // split after second byte of "你"

        let chunk1 = Bytes::from(bytes[..split_point].to_vec());
        let chunk2 = Bytes::from(bytes[split_point..].to_vec());

        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(chunk1),
            Ok::<_, std::io::Error>(chunk2),
        ]);
        let converted = create_anthropic_sse_stream_from_responses(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(
            merged.contains("你好世界"),
            "expected '你好世界' in output, got replacement chars (U+FFFD)"
        );
        assert!(
            !merged.contains('\u{FFFD}'),
            "output must not contain U+FFFD replacement characters"
        );
    }
}
