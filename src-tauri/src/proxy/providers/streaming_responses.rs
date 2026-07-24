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
    responses_to_anthropic_with_web_search_name, sanitize_anthropic_tool_use_input_json,
    web_search_action_input, web_search_results_from_action, web_search_results_from_output_item,
};
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

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

/// Convert a compatible gateway's non-streaming Responses JSON into a complete
/// Anthropic SSE lifecycle. This is used when the client requested streaming but
/// the upstream ignored `stream:true` and returned `application/json`.
fn responses_json_to_anthropic_sse(
    body: Value,
    hosted_web_search_name: Option<&str>,
) -> Vec<Bytes> {
    let message = match responses_to_anthropic_with_web_search_name(body, hosted_web_search_name) {
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
struct StreamedTextState {
    by_output_part: HashMap<(u64, u64), String>,
    by_item_part: HashMap<(String, u64), String>,
    unkeyed: String,
}

impl StreamedTextState {
    fn record_delta(&mut self, data: &Value, delta: &str) {
        let content_index = data.get("content_index").and_then(Value::as_u64);
        if let (Some(output_index), Some(content_index)) = (
            data.get("output_index").and_then(Value::as_u64),
            content_index,
        ) {
            self.by_output_part
                .entry((output_index, content_index))
                .or_default()
                .push_str(delta);
            return;
        }
        if let (Some(item_id), Some(content_index)) =
            (data.get("item_id").and_then(Value::as_str), content_index)
        {
            self.by_item_part
                .entry((item_id.to_string(), content_index))
                .or_default()
                .push_str(delta);
            return;
        }
        self.unkeyed.push_str(delta);
    }

    fn missing_suffix(
        &mut self,
        full_text: &str,
        output_index: Option<u64>,
        item_id: Option<&str>,
        content_index: u64,
    ) -> String {
        let by_output =
            output_index.and_then(|index| self.by_output_part.get(&(index, content_index)));
        let by_item =
            item_id.and_then(|id| self.by_item_part.get(&(id.to_string(), content_index)));
        let emitted = match (by_output, by_item) {
            (Some(output), Some(item)) if item.len() > output.len() => Some(item.as_str()),
            (Some(output), _) => Some(output.as_str()),
            (None, Some(item)) => Some(item.as_str()),
            (None, None) => None,
        };

        let missing = if let Some(emitted) = emitted {
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
        } else if self.unkeyed.is_empty() {
            full_text.to_string()
        } else {
            let unkeyed = self.unkeyed.clone();
            if let Some(remaining) = unkeyed.strip_prefix(full_text) {
                self.unkeyed = remaining.to_string();
                String::new()
            } else if let Some(suffix) = full_text.strip_prefix(&unkeyed) {
                self.unkeyed.clear();
                suffix.to_string()
            } else {
                log::warn!(
                    "[Claude/Responses] Could not correlate terminal text with an unkeyed streamed delta; avoiding duplicate replay"
                );
                String::new()
            }
        };

        if let Some(output_index) = output_index {
            self.by_output_part
                .insert((output_index, content_index), full_text.to_string());
        }
        if let Some(item_id) = item_id {
            self.by_item_part
                .insert((item_id.to_string(), content_index), full_text.to_string());
        }
        missing
    }
}

fn missing_message_text_parts(
    item: &Value,
    output_index: Option<u64>,
    streamed_text: &mut StreamedTextState,
) -> Vec<String> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return Vec::new();
    }
    let item_id = item.get("id").and_then(Value::as_str);
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(content_index, part)| {
            let full_text = match part.get("type").and_then(Value::as_str) {
                Some("output_text") => part.get("text").and_then(Value::as_str),
                Some("refusal") => part.get("refusal").and_then(Value::as_str),
                _ => None,
            }
            .filter(|text| !text.is_empty())?;
            let missing = streamed_text.missing_suffix(
                full_text,
                output_index,
                item_id,
                content_index as u64,
            );
            (!missing.is_empty()).then_some(missing)
        })
        .collect()
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

#[inline]
fn web_search_item_key(data: &Value, item: Option<&Value>) -> Option<String> {
    if let Some(item_id) = item
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| data.get("item_id").and_then(Value::as_str))
    {
        return Some(format!("web-search:{item_id}"));
    }
    data.get("output_index")
        .and_then(Value::as_u64)
        .map(|index| format!("web-search:out:{index}"))
}

fn web_search_result_events(index: u32, tool_use_id: &str, content: Vec<Value>) -> [Bytes; 2] {
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

fn append_unique_web_search_results(target: &mut Vec<Value>, results: Vec<Value>) {
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

fn record_web_search_call(
    search_id: &str,
    item: &Value,
    ids_seen: &mut HashSet<String>,
    id_order: &mut Vec<String>,
    results_by_id: &mut HashMap<String, Vec<Value>>,
    request_count: &mut u64,
) {
    if ids_seen.insert(search_id.to_string()) {
        *request_count += 1;
        id_order.push(search_id.to_string());
    }
    append_unique_web_search_results(
        results_by_id.entry(search_id.to_string()).or_default(),
        web_search_results_from_action(item),
    );
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

/// 创建从 Responses API SSE 到 Anthropic SSE 的转换流
///
/// 状态机跟踪: message_id, current_model, has_sent_message_start, item/content index map
/// SSE 解析支持 named events (event: + data: 行)
pub fn create_anthropic_sse_stream_from_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_anthropic_sse_stream_from_responses_with_web_search_name(stream, None)
}

pub(crate) fn create_anthropic_sse_stream_from_responses_with_web_search_name<
    E: std::error::Error + Send + 'static,
>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    hosted_web_search_name: Option<String>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let hosted_web_search_name = hosted_web_search_name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "web_search".to_string());
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
        let mut tool_index_by_item_id: HashMap<String, u32> = HashMap::new();
        let mut tool_name_by_index: HashMap<u32, String> = HashMap::new();
        let mut tool_args_by_index: HashMap<u32, String> = HashMap::new();
        let mut tool_had_delta: HashSet<u32> = HashSet::new();
        let mut last_tool_index: Option<u32> = None;
        let mut web_search_index_by_item_id: HashMap<String, u32> = HashMap::new();
        let mut web_search_ids_seen: HashSet<String> = HashSet::new();
        let mut web_search_ids_completed: HashSet<String> = HashSet::new();
        let mut web_search_id_order: Vec<String> = Vec::new();
        let mut web_search_results_by_id: HashMap<String, Vec<Value>> = HashMap::new();
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

        while let Some((chunk, is_eof)) = stream.next().await {
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
                            "response.content_part.done" => {}

                            // ================================================
                            // response.output_item.added (function_call) → content_block_start (tool_use)
                            // ================================================
                            "response.output_item.added" => {
                                if let Some(item) = data.get("item") {
                                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
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

                                        let index = if let Some(key) = web_search_item_key(&data, Some(item)) {
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
                                        let search_id = item
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .or_else(|| data.get("item_id").and_then(Value::as_str))
                                            .filter(|id| !id.is_empty())
                                            .map(ToString::to_string)
                                            .unwrap_or_else(|| format!("ws_stream_{index}"));
                                        record_web_search_call(
                                            &search_id,
                                            item,
                                            &mut web_search_ids_seen,
                                            &mut web_search_id_order,
                                            &mut web_search_results_by_id,
                                            &mut web_search_count,
                                        );
                                        web_search_index_by_item_id
                                            .insert(search_id.clone(), index);

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
                                if let Some(output) =
                                    response_obj.get("output").and_then(Value::as_array)
                                {
                                    let has_web_search_output = output.iter().any(|item| {
                                        item.get("type").and_then(Value::as_str)
                                            == Some("web_search_call")
                                    });
                                    if has_web_search_output {
                                        if let Some(text_index) = current_text_index.take() {
                                            if open_indices.remove(&text_index) {
                                                yield Ok(anthropic_sse(
                                                    "content_block_stop",
                                                    &json!({"type":"content_block_stop","index":text_index}),
                                                ));
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
                                            let search_id = item
                                                .get("id")
                                                .and_then(Value::as_str)
                                                .filter(|id| !id.is_empty())
                                                .map(ToString::to_string)
                                                .unwrap_or_else(|| {
                                                    format!("ws_terminal_{output_index}")
                                                });
                                            record_web_search_call(
                                                &search_id,
                                                item,
                                                &mut web_search_ids_seen,
                                                &mut web_search_id_order,
                                                &mut web_search_results_by_id,
                                                &mut web_search_count,
                                            );

                                            if !web_search_ids_completed.contains(&search_id) {
                                                let index = web_search_index_by_item_id
                                                    .get(&search_id)
                                                    .copied()
                                                    .unwrap_or_else(|| {
                                                        let assigned = next_content_index;
                                                        next_content_index += 1;
                                                        assigned
                                                    });
                                                web_search_index_by_item_id
                                                    .insert(search_id.clone(), index);
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

                                let attributed_web_search_urls: HashSet<String> =
                                    web_search_results_by_id
                                        .values()
                                        .flatten()
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
                                if let Some(last_search_id) = web_search_id_order.last().cloned() {
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
                                        if open_indices.remove(&text_index) {
                                            yield Ok(anthropic_sse(
                                                "content_block_stop",
                                                &json!({"type":"content_block_stop","index":text_index}),
                                            ));
                                        }
                                    }
                                    for search_id in web_search_id_order.clone() {
                                        let index = next_content_index;
                                        next_content_index += 1;
                                        let results = web_search_results_by_id
                                            .remove(&search_id)
                                            .unwrap_or_default();
                                        for event in
                                            web_search_result_events(index, &search_id, results)
                                        {
                                            yield Ok(event);
                                        }
                                    }
                                }

                                for (output_index, item) in terminal_message_items {
                                    let missing_text = missing_message_text_parts(
                                        &item,
                                        Some(output_index),
                                        &mut streamed_text,
                                    );
                                    if missing_text.is_empty() {
                                        continue;
                                    }
                                    has_substantive_output = true;
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
                                    for text in missing_text {
                                        let index = next_content_index;
                                        next_content_index += 1;
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
                                match item.get("type").and_then(Value::as_str) {
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

                                        let provisional_index = web_search_item_key(&data, Some(item))
                                            .and_then(|key| index_by_key.get(&key).copied());
                                        let search_id = item
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .or_else(|| data.get("item_id").and_then(Value::as_str))
                                            .filter(|id| !id.is_empty())
                                            .map(ToString::to_string)
                                            .or_else(|| {
                                                provisional_index
                                                    .map(|index| format!("ws_stream_{index}"))
                                            })
                                            .unwrap_or_else(|| {
                                                format!("ws_stream_{next_content_index}")
                                            });
                                        record_web_search_call(
                                            &search_id,
                                            item,
                                            &mut web_search_ids_seen,
                                            &mut web_search_id_order,
                                            &mut web_search_results_by_id,
                                            &mut web_search_count,
                                        );
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
                                                if let Some(key) = web_search_item_key(&data, Some(item)) {
                                                    index_by_key.insert(key, assigned);
                                                }
                                                assigned
                                            });
                                        web_search_index_by_item_id
                                            .insert(search_id.clone(), index);

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
                                        let missing_text = missing_message_text_parts(
                                            item,
                                            data.get("output_index").and_then(Value::as_u64),
                                            &mut streamed_text,
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
                                            for text in missing_text {
                                                let index = next_content_index;
                                                next_content_index += 1;
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
            let has_open_server_tool = open_indices.iter().any(|index| {
                web_search_index_by_item_id
                    .values()
                    .any(|server_index| server_index == index)
            });
            let has_open_reasoning = open_indices.iter().any(|index| {
                reasoning_item_by_index.contains_key(index)
                    || reasoning_text_by_index.contains_key(index)
                    || legacy_reasoning_index == Some(*index)
            });

            if has_substantive_output
                && !has_open_tool
                && !has_open_server_tool
                && !has_open_reasoning
            {
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
        create_anthropic_sse_stream_from_responses_with_web_search_name(
            upstream,
            Some(hosted_web_search_name.to_string()),
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
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_123\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust official documentation\"}}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"output_index\":1,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":12}}}\n\n"
        );

        let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
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
        assert!(merged.contains("\"stop_reason\":\"end_turn\""));
        assert!(!merged.contains("\"stop_reason\":\"tool_use\""));
        assert!(merged.contains("\"web_search_requests\":1"));
        assert!(merged.contains("event: message_stop"));
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
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi_fallback\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_first\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"first query\"}},{\"id\":\"ws_second\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"second query\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Combined answer.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://example.com/result\",\"title\":\"Combined result\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
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
        assert_eq!(text_deltas, vec!["Combined answer."]);
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
