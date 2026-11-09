use super::codex_chat_common::{is_empty_value, response_item_call_id};
use crate::proxy::sse::{strip_sse_field, SseBuffer};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_CACHED_RESPONSES: usize = 512;

#[derive(Debug, Clone, Default)]
struct CachedResponse {
    /// Session the response was recorded under; see [`CodexChatHistoryStore`].
    scope: String,
    calls_by_id: HashMap<String, Value>,
    call_order: Vec<String>,
}

/// `(scope, call_id)`: fallback lookups never cross sessions.
type ScopedCallId = (String, String);

#[derive(Debug, Default)]
struct CodexChatHistoryInner {
    responses: HashMap<String, CachedResponse>,
    response_order: VecDeque<String>,
    call_index: HashMap<ScopedCallId, VecDeque<String>>,
}

#[derive(Debug, Clone, Default)]
struct CachedLookup {
    previous: Option<CachedResponse>,
    fallback: CachedResponse,
}

/// Cross-request history needed when Codex Responses is bridged to Chat
/// Completions.
///
/// Chat providers such as DeepSeek require an assistant message with the
/// original tool call and its `reasoning_content` immediately before the tool
/// result. Codex often sends follow-up requests as
/// `previous_response_id + function_call_output`, so this store restores the
/// missing function call before the request is converted to Chat messages.
/// Some Codex flows such as subagents may omit or rewrite
/// `previous_response_id`, so the store can also fall back to a uniquely
/// cached `call_id`.
///
/// That fallback is scoped to the client session (Codex sends `session-id`).
/// Upstreams such as Kimi TaaS number tool calls per conversation
/// (`exec_command:0`, ...), so an unscoped index lets one session's history
/// pick up another session's call and reasoning once its own entry is evicted
/// or the proxy restarts. Rewriting old history that way breaks the upstream
/// prefix cache and leaks reasoning across sessions. Requests without a client
/// session share the empty scope.
#[derive(Debug, Default)]
pub struct CodexChatHistoryStore {
    inner: RwLock<CodexChatHistoryInner>,
}

impl CodexChatHistoryStore {
    pub async fn record_response(&self, response: &Value, scope: Option<&str>) -> usize {
        let Some(response_id) = response
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        else {
            return 0;
        };

        let calls = response
            .get("output")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(cached_call_item)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if calls.is_empty() {
            return 0;
        }

        let mut inner = self.inner.write().await;
        inner.insert_calls(scope.unwrap_or_default(), response_id, calls)
    }

    async fn record_call_item(
        &self,
        response_id: Option<&str>,
        item: &Value,
        scope: Option<&str>,
    ) -> bool {
        let Some(call) = cached_call_item(item) else {
            return false;
        };

        let mut inner = self.inner.write().await;
        if let Some(response_id) = response_id.filter(|value| !value.is_empty()) {
            inner.insert_calls(scope.unwrap_or_default(), response_id, vec![call]) > 0
        } else {
            false
        }
    }

    pub async fn enrich_request(&self, body: &mut Value, scope: Option<&str>) -> usize {
        let previous_response_id = body
            .get("previous_response_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let Some(input) = body.get_mut("input") else {
            return 0;
        };

        let original_input = std::mem::take(input);
        let original_was_object = matches!(&original_input, Value::Object(_));
        let items = match original_input {
            Value::Array(items) => items,
            Value::Object(object) => vec![Value::Object(object)],
            other => {
                *input = other;
                return 0;
            }
        };

        let output_call_ids = items
            .iter()
            .filter(|item| {
                item.get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_call_output_item_type)
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let existing_call_ids = items
            .iter()
            .filter(|item| {
                item.get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_call_item_type)
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let requested_call_ids = output_call_ids
            .union(&existing_call_ids)
            .cloned()
            .collect::<HashSet<_>>();
        let lookup = self
            .lookup(
                previous_response_id.as_deref(),
                scope.unwrap_or_default(),
                &requested_call_ids,
            )
            .await;

        let restore_group = lookup.restore_group(&output_call_ids, &existing_call_ids);

        let restore_group_ids = restore_group
            .iter()
            .map(|(call_id, _)| call_id.clone())
            .collect::<HashSet<_>>();
        let mut restore_group = Some(restore_group);
        let mut seen_call_ids = HashSet::new();
        let mut restored = 0usize;
        let mut enriched = 0usize;
        let mut new_items = Vec::new();

        for mut item in items {
            match item.get("type").and_then(|value| value.as_str()) {
                Some(item_type) if is_call_item_type(item_type) => {
                    if let Some(call_id) = response_item_call_id(&item) {
                        if let Some(cached) = lookup.call(&call_id) {
                            if enrich_call_item_from_cache(&mut item, cached) {
                                enriched += 1;
                            }
                        }
                        seen_call_ids.insert(call_id);
                    }
                    new_items.push(item);
                }
                Some(item_type) if is_call_output_item_type(item_type) => {
                    // Older results may precede this batch in a full-history request.
                    if response_item_call_id(&item)
                        .is_some_and(|id| restore_group_ids.contains(&id))
                    {
                        if let Some(group) = restore_group.take() {
                            for (call_id, cached_item) in group {
                                seen_call_ids.insert(call_id);
                                new_items.push(cached_item);
                                restored += 1;
                            }
                        }
                    }

                    if let Some(call_id) = response_item_call_id(&item) {
                        if !seen_call_ids.contains(&call_id)
                            && !restore_group_ids.contains(&call_id)
                        {
                            if let Some(cached) = lookup.call(&call_id).cloned() {
                                seen_call_ids.insert(call_id);
                                new_items.push(cached);
                                restored += 1;
                            }
                        }
                    }
                    new_items.push(item);
                }
                _ => new_items.push(item),
            }
        }

        let changed = restored + enriched;
        if changed == 0 && original_was_object && new_items.len() == 1 {
            *input = new_items.into_iter().next().unwrap_or(Value::Null);
        } else {
            *input = Value::Array(new_items);
        }
        changed
    }

    async fn lookup(
        &self,
        previous_response_id: Option<&str>,
        scope: &str,
        requested_call_ids: &HashSet<String>,
    ) -> CachedLookup {
        let inner = self.inner.read().await;
        let previous = previous_response_id.and_then(|id| inner.response(id, scope).cloned());
        let fallback = inner.unique_fallback_calls(scope, requested_call_ids, previous.as_ref());
        CachedLookup { previous, fallback }
    }
}

impl CodexChatHistoryInner {
    /// Cached response `response_id` as seen from `scope`.
    fn response(&self, response_id: &str, scope: &str) -> Option<&CachedResponse> {
        self.responses
            .get(response_id)
            .filter(|response| response.scope == scope)
            .or_else(|| self.responses.get(&scoped_response_key(response_id, scope)))
    }

    fn insert_calls(
        &mut self,
        scope: &str,
        response_id: &str,
        calls: Vec<(String, Value)>,
    ) -> usize {
        // Upstreams that omit the chat id all map to the same response id
        // (`resp_ccswitch`); keep each session's calls under its own key.
        let response_key = if self
            .responses
            .get(response_id)
            .is_some_and(|response| response.scope != scope)
        {
            scoped_response_key(response_id, scope)
        } else {
            response_id.to_string()
        };
        let response_id = response_key.as_str();
        if !self.responses.contains_key(response_id) {
            self.response_order.push_back(response_id.to_string());
        }

        let cached_response = self
            .responses
            .entry(response_id.to_string())
            .or_insert_with(|| CachedResponse {
                scope: scope.to_string(),
                ..CachedResponse::default()
            });
        let mut inserted_or_updated = 0usize;
        let mut indexed_call_ids = Vec::new();
        for (call_id, item) in calls {
            if !cached_response.calls_by_id.contains_key(&call_id) {
                cached_response.call_order.push(call_id.clone());
            }
            cached_response.calls_by_id.insert(call_id.clone(), item);
            indexed_call_ids.push(call_id);
            inserted_or_updated += 1;
        }
        for call_id in indexed_call_ids {
            self.index_call(scope, &call_id, response_id);
        }

        self.prune();
        inserted_or_updated
    }

    fn prune(&mut self) {
        while self.response_order.len() > MAX_CACHED_RESPONSES {
            let Some(response_id) = self.response_order.pop_front() else {
                break;
            };
            self.responses.remove(&response_id);
            self.remove_response_from_call_index(&response_id);
        }
    }

    fn index_call(&mut self, scope: &str, call_id: &str, response_id: &str) {
        let response_ids = self
            .call_index
            .entry((scope.to_string(), call_id.to_string()))
            .or_default();
        if !response_ids
            .iter()
            .any(|cached_id| cached_id == response_id)
        {
            response_ids.push_back(response_id.to_string());
        }
    }

    fn remove_response_from_call_index(&mut self, response_id: &str) {
        for response_ids in self.call_index.values_mut() {
            response_ids.retain(|cached_id| cached_id != response_id);
        }
        self.call_index
            .retain(|_, response_ids| !response_ids.is_empty());
    }

    fn unique_fallback_calls(
        &self,
        scope: &str,
        requested_call_ids: &HashSet<String>,
        previous: Option<&CachedResponse>,
    ) -> CachedResponse {
        let mut selected = HashMap::new();
        for call_id in requested_call_ids {
            if previous.is_some_and(|response| response.calls_by_id.contains_key(call_id)) {
                continue;
            }
            if let Some(item) = self.unique_call(scope, call_id) {
                selected.insert(call_id.clone(), item.clone());
            }
        }

        let mut fallback = CachedResponse::default();
        for response_id in &self.response_order {
            let Some(response) = self
                .responses
                .get(response_id)
                .filter(|response| response.scope == scope)
            else {
                continue;
            };
            for call_id in &response.call_order {
                if let Some(item) = selected.remove(call_id) {
                    fallback.call_order.push(call_id.clone());
                    fallback.calls_by_id.insert(call_id.clone(), item);
                }
            }
        }
        fallback
    }

    fn unique_call(&self, scope: &str, call_id: &str) -> Option<&Value> {
        let response_ids = self
            .call_index
            .get(&(scope.to_string(), call_id.to_string()))?;
        let mut found = None;
        for response_id in response_ids {
            let Some(item) = self
                .responses
                .get(response_id)
                .and_then(|response| response.calls_by_id.get(call_id))
            else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            found = Some(item);
        }
        found
    }
}

impl CachedLookup {
    fn call(&self, call_id: &str) -> Option<&Value> {
        self.previous
            .as_ref()
            .and_then(|previous| previous.calls_by_id.get(call_id))
            .or_else(|| self.fallback.calls_by_id.get(call_id))
    }

    fn restore_group(
        &self,
        output_call_ids: &HashSet<String>,
        existing_call_ids: &HashSet<String>,
    ) -> Vec<(String, Value)> {
        let mut group = Vec::new();
        let mut grouped_call_ids = HashSet::new();
        if let Some(previous) = &self.previous {
            append_restore_group(
                previous,
                output_call_ids,
                existing_call_ids,
                &mut grouped_call_ids,
                &mut group,
            );
        }
        append_restore_group(
            &self.fallback,
            output_call_ids,
            existing_call_ids,
            &mut grouped_call_ids,
            &mut group,
        );
        group
    }
}

fn append_restore_group(
    response: &CachedResponse,
    output_call_ids: &HashSet<String>,
    existing_call_ids: &HashSet<String>,
    grouped_call_ids: &mut HashSet<String>,
    group: &mut Vec<(String, Value)>,
) {
    for call_id in &response.call_order {
        if !output_call_ids.contains(call_id)
            || existing_call_ids.contains(call_id)
            || grouped_call_ids.contains(call_id)
        {
            continue;
        }
        if let Some(item) = response.calls_by_id.get(call_id).cloned() {
            grouped_call_ids.insert(call_id.clone());
            group.push((call_id.clone(), item));
        }
    }
}

pub fn record_responses_sse_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    history: Arc<CodexChatHistoryStore>,
    scope: Option<String>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut sse = SseBuffer::new();
        let mut current_response_id: Option<String> = None;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    sse.push_bytes(&bytes);
                    while let Some(block) = sse.next_block() {
                        inspect_sse_block(
                            block,
                            &mut current_response_id,
                            history.as_ref(),
                            scope.as_deref(),
                        )
                        .await;
                    }
                    yield Ok(bytes);
                }
                Err(err) => yield Err(err),
            }
        }
    }
}

async fn inspect_sse_block(
    block: &str,
    current_response_id: &mut Option<String>,
    history: &CodexChatHistoryStore,
    scope: Option<&str>,
) {
    if block.trim().is_empty() {
        return;
    }

    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data.to_string());
        }
    }

    let data = data_parts.join("\n");
    if data.trim().is_empty() || data.trim() == "[DONE]" {
        return;
    }

    let Ok(value) = serde_json::from_str::<Value>(&data) else {
        return;
    };

    if let Some(response_id) = value
        .pointer("/response/id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    {
        *current_response_id = Some(response_id.to_string());
    }

    match value.get("type").and_then(|value| value.as_str()) {
        Some("response.output_item.done") => {
            if let Some(item) = value.get("item") {
                history
                    .record_call_item(current_response_id.as_deref(), item, scope)
                    .await;
            }
        }
        Some("response.completed") => {
            if let Some(response) = value.get("response") {
                history.record_response(response, scope).await;
            }
        }
        _ => {}
    }
}

fn scoped_response_key(response_id: &str, scope: &str) -> String {
    format!("{response_id}\u{0}{scope}")
}

fn cached_call_item(item: &Value) -> Option<(String, Value)> {
    if !item
        .get("type")
        .and_then(|value| value.as_str())
        .is_some_and(is_call_item_type)
    {
        return None;
    }
    let call_id = response_item_call_id(item)?;
    Some((call_id, item.clone()))
}

fn is_call_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call" | "custom_tool_call" | "tool_search_call"
    )
}

pub(crate) fn is_call_output_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call_output" | "custom_tool_call_output" | "tool_search_output"
    )
}

fn enrich_call_item_from_cache(item: &mut Value, cached: &Value) -> bool {
    let mut changed = false;
    for key in [
        "name",
        "namespace",
        "arguments",
        "input",
        "status",
        "execution",
        "reasoning_content",
        "reasoning",
        "extra_content",
    ] {
        if item.get(key).is_some_and(|value| !is_empty_value(value)) {
            continue;
        }
        let Some(value) = cached.get(key).filter(|value| !is_empty_value(value)) else {
            continue;
        };
        if let Some(object) = item.as_object_mut() {
            object.insert(key.to_string(), value.clone());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn enriches_tool_output_with_cached_function_call_from_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "read_file",
                            "arguments": "{\"path\":\"README.md\"}",
                            "reasoning_content": "Need to inspect the file."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_unique_call_id_without_matching_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "read_file",
                            "arguments": "{}",
                            "reasoning_content": "This is the only cached call."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut missing_previous = json!({
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut missing_previous, None).await, 1);
        assert_eq!(missing_previous["input"][0]["type"], "function_call");
        assert_eq!(
            missing_previous["input"][0]["reasoning_content"],
            "This is the only cached call."
        );
        assert_eq!(missing_previous["input"][1]["type"], "function_call_output");

        let mut different_previous = json!({
            "previous_response_id": "resp_2",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(
            history.enrich_request(&mut different_previous, None).await,
            1
        );
        assert_eq!(different_previous["input"][0]["type"], "function_call");
        assert_eq!(
            different_previous["input"][0]["reasoning_content"],
            "This is the only cached call."
        );
        assert_eq!(
            different_previous["input"][1]["type"],
            "function_call_output"
        );
    }

    #[tokio::test]
    async fn does_not_restore_ambiguous_call_id_without_previous_response() {
        let history = CodexChatHistoryStore::default();
        for (response_id, reasoning) in [
            ("resp_1", "This belongs to the first response."),
            ("resp_2", "This belongs to the second response."),
        ] {
            history
                .record_response(
                    &json!({
                        "id": response_id,
                        "output": [
                            {
                                "type": "function_call",
                                "call_id": "call_1",
                                "name": "read_file",
                                "arguments": "{}",
                                "reasoning_content": reasoning
                            }
                        ]
                    }),
                    None,
                )
                .await;
        }

        let mut missing_previous = json!({
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(history.enrich_request(&mut missing_previous, None).await, 0);
        assert_eq!(missing_previous["input"][0]["type"], "function_call_output");

        let mut different_previous = json!({
            "previous_response_id": "resp_missing",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });
        assert_eq!(
            history.enrich_request(&mut different_previous, None).await,
            0
        );
        assert_eq!(
            different_previous["input"][0]["type"],
            "function_call_output"
        );
    }

    #[tokio::test]
    async fn enriches_existing_function_call_missing_reasoning() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "read_file",
                            "arguments": "{}",
                            "reasoning_content": "Need to inspect the file."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input.len(), 2);
    }

    #[tokio::test]
    async fn round_trips_gemini_thought_signature_extra_content() {
        // Gemini OpenAI 兼容接口要求下一轮原样带回 tool call 的 thought signature（#8336）。
        let signature = json!({"google": {"thought_signature": "sig-abc"}});
        let response = super::super::transform_codex_chat::chat_completion_to_response(json!({
            "id": "chatcmpl_gemini",
            "model": "gemini-4-argon",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "exec_command", "arguments": "{\"cmd\":\"ls\"}"},
                        "extra_content": signature
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }))
        .unwrap();
        let output = response["output"].as_array().unwrap();
        let call = output
            .iter()
            .find(|item| item["type"] == "function_call")
            .unwrap();
        assert_eq!(call["extra_content"], signature);

        let history = CodexChatHistoryStore::default();
        history.record_response(&response, None).await;

        // Codex 回传历史时不保留未知字段。
        let mut request = json!({
            "model": "gemini-4-argon",
            "input": [
                {"type": "message", "role": "user", "content": "list files"},
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"ls\"}"
                },
                {"type": "function_call_output", "call_id": "call_1", "output": "a.txt"}
            ]
        });
        assert_eq!(history.enrich_request(&mut request, None).await, 1);

        let chat =
            super::super::transform_codex_chat::responses_to_chat_completions(request).unwrap();
        let assistant = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["tool_calls"][0]["extra_content"], signature);
    }

    #[tokio::test]
    async fn enriches_existing_function_call_missing_name_and_arguments() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "read_file",
                            "arguments": "{\"path\":\"README.md\"}",
                            "reasoning_content": "Need to inspect the file."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["name"], "read_file");
        assert_eq!(input[0]["arguments"], "{\"path\":\"README.md\"}");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_parallel_tool_calls_as_one_assistant_group() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "first",
                            "arguments": "{}",
                            "reasoning_content": "Need both tools."
                        },
                        {
                            "type": "function_call",
                            "call_id": "call_2",
                            "name": "second",
                            "arguments": "{}",
                            "reasoning_content": "Need both tools."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "one"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": "two"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 2);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_2");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[3]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_missing_parallel_group_after_existing_older_call_results() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_old",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_old",
                            "name": "exec_command",
                            "arguments": "{}",
                            "reasoning_content": "First step."
                        }
                    ]
                }),
                None,
            )
            .await;
        history
            .record_response(
                &json!({
                    "id": "resp_new",
                    "output": [
                        {
                            "type": "function_call",
                            "call_id": "call_new_1",
                            "name": "exec_command",
                            "arguments": "{}",
                            "reasoning_content": "Second step."
                        },
                        {
                            "type": "function_call",
                            "call_id": "call_new_2",
                            "name": "exec_command",
                            "arguments": "{}",
                            "reasoning_content": "Second step."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_new",
            "input": [
                { "role": "user", "content": "test" },
                { "type": "function_call", "call_id": "call_old", "name": "exec_command", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "call_old", "output": "old" },
                { "type": "function_call_output", "call_id": "call_new_1", "output": "one" },
                { "type": "function_call_output", "call_id": "call_new_2", "output": "two" }
            ]
        });

        history.enrich_request(&mut request, None).await;
        let chat =
            super::super::transform_codex_chat::responses_to_chat_completions(request).unwrap();
        let assistants: Vec<_> = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "assistant")
            .collect();
        assert_eq!(assistants.len(), 2);
        assert_eq!(assistants[0]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(assistants[1]["tool_calls"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn restores_custom_and_tool_search_calls_from_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({
                    "id": "resp_1",
                    "output": [
                        {
                            "type": "custom_tool_call",
                            "call_id": "call_patch",
                            "name": "apply_patch",
                            "input": "*** Begin Patch\n*** End Patch",
                            "reasoning_content": "Need to patch the file."
                        },
                        {
                            "type": "tool_search_call",
                            "call_id": "call_search",
                            "status": "completed",
                            "execution": "client",
                            "arguments": {"query": "Gmail tools"},
                            "reasoning_content": "Need to discover tools."
                        }
                    ]
                }),
                None,
            )
            .await;

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_patch",
                    "output": "patched"
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "tools": []
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 2);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "custom_tool_call");
        assert_eq!(input[0]["call_id"], "call_patch");
        assert_eq!(input[1]["type"], "tool_search_call");
        assert_eq!(input[1]["call_id"], "call_search");
        assert_eq!(input[2]["type"], "custom_tool_call_output");
        assert_eq!(input[3]["type"], "tool_search_output");
    }

    #[tokio::test]
    async fn records_streamed_function_call_done_items() {
        let history = Arc::new(CodexChatHistoryStore::default());
        let stream = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream\"}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"{}\",\"reasoning_content\":\"Need a file.\"}}\n\n",
            )),
        ]);

        let output = record_responses_sse_stream(stream, history.clone(), None)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(output.len(), 2);

        let mut request = json!({
            "previous_response_id": "resp_stream",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                }
            ]
        });

        assert_eq!(history.enrich_request(&mut request, None).await, 1);
        assert_eq!(request["input"][0]["reasoning_content"], "Need a file.");
    }

    fn numbered_call(call_id: &str, reasoning: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": call_id,
            "name": "exec_command",
            "arguments": "{}",
            "reasoning_content": reasoning
        })
    }

    fn replayed_history(call_id: &str) -> Value {
        json!({
            "input": [
                {"type": "function_call", "call_id": call_id, "name": "exec_command", "arguments": "{}"},
                {"type": "function_call_output", "call_id": call_id, "output": "ok"}
            ]
        })
    }

    #[tokio::test]
    async fn fallback_never_borrows_a_call_from_another_session() {
        // Kimi TaaS numbers tool calls per conversation, so two sessions share
        // `exec_command:0`. With session A's own entry gone (eviction or proxy
        // restart), session B's entry must not be copied into A's history.
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({"id": "resp_b", "output": [numbered_call("exec_command:0", "B reasoning")]}),
                Some("session_b"),
            )
            .await;

        let mut request = replayed_history("exec_command:0");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_a"))
                .await,
            0
        );
        assert!(request["input"][0].get("reasoning_content").is_none());

        let mut request = replayed_history("exec_command:0");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_b"))
                .await,
            1
        );
        assert_eq!(request["input"][0]["reasoning_content"], "B reasoning");
    }

    #[tokio::test]
    async fn same_call_id_in_another_session_does_not_make_own_call_ambiguous() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({"id": "resp_a", "output": [numbered_call("exec_command:0", "A reasoning")]}),
                Some("session_a"),
            )
            .await;
        history
            .record_response(
                &json!({"id": "resp_b", "output": [numbered_call("exec_command:0", "B reasoning")]}),
                Some("session_b"),
            )
            .await;

        let mut request = replayed_history("exec_command:0");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_a"))
                .await,
            1
        );
        assert_eq!(request["input"][0]["reasoning_content"], "A reasoning");
    }

    #[tokio::test]
    async fn requests_without_a_session_keep_sharing_the_unscoped_index() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(
                &json!({"id": "resp_1", "output": [numbered_call("call_1", "Unscoped reasoning")]}),
                None,
            )
            .await;

        let mut request = replayed_history("call_1");
        assert_eq!(history.enrich_request(&mut request, None).await, 1);
        assert_eq!(
            request["input"][0]["reasoning_content"],
            "Unscoped reasoning"
        );

        let mut request = replayed_history("call_1");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_a"))
                .await,
            0
        );
    }

    #[tokio::test]
    async fn shared_fallback_response_id_stays_per_session() {
        // Non-streaming chat responses without an id all become `resp_ccswitch`.
        let history = CodexChatHistoryStore::default();
        for (scope, call_id, reasoning) in [
            ("session_a", "call_a", "A reasoning"),
            ("session_b", "call_b", "B reasoning"),
        ] {
            history
                .record_response(
                    &json!({"id": "resp_ccswitch", "output": [numbered_call(call_id, reasoning)]}),
                    Some(scope),
                )
                .await;
        }

        let mut request = replayed_history("call_b");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_b"))
                .await,
            1
        );
        assert_eq!(request["input"][0]["reasoning_content"], "B reasoning");

        // previous_response_id must not reach the other session's calls either.
        let mut request = replayed_history("call_a");
        request["previous_response_id"] = json!("resp_ccswitch");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_b"))
                .await,
            0
        );
        let mut request = replayed_history("call_a");
        request["previous_response_id"] = json!("resp_ccswitch");
        assert_eq!(
            history
                .enrich_request(&mut request, Some("session_a"))
                .await,
            1
        );
    }
}
