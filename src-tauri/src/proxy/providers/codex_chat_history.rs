use super::codex_chat_common::{is_empty_value, response_item_call_id};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_CACHED_RESPONSES: usize = 512;

#[derive(Debug, Clone, Default)]
struct CachedResponse {
    calls_by_id: HashMap<String, Value>,
    call_order: Vec<String>,
}

#[derive(Debug, Default)]
struct CodexChatHistoryInner {
    responses: HashMap<String, CachedResponse>,
    response_order: VecDeque<String>,
    call_index: HashMap<String, VecDeque<String>>,
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
#[derive(Debug, Default)]
pub struct CodexChatHistoryStore {
    inner: RwLock<CodexChatHistoryInner>,
}

impl CodexChatHistoryStore {
    pub async fn record_response(&self, response: &Value) -> usize {
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
                    .filter_map(cached_function_call)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if calls.is_empty() {
            return 0;
        }

        let mut inner = self.inner.write().await;
        inner.insert_calls(response_id, calls)
    }

    async fn record_function_call(&self, response_id: Option<&str>, item: &Value) -> bool {
        let Some(call) = cached_function_call(item) else {
            return false;
        };

        let mut inner = self.inner.write().await;
        if let Some(response_id) = response_id.filter(|value| !value.is_empty()) {
            inner.insert_calls(response_id, vec![call]) > 0
        } else {
            false
        }
    }

    pub async fn enrich_request(&self, body: &mut Value) -> usize {
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
                item.get("type").and_then(|value| value.as_str()) == Some("function_call_output")
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let existing_call_ids = items
            .iter()
            .filter(|item| {
                item.get("type").and_then(|value| value.as_str()) == Some("function_call")
            })
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let requested_call_ids = output_call_ids
            .union(&existing_call_ids)
            .cloned()
            .collect::<HashSet<_>>();
        let lookup = self
            .lookup(previous_response_id.as_deref(), &requested_call_ids)
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
                Some("function_call") => {
                    if let Some(call_id) = response_item_call_id(&item) {
                        if let Some(cached) = lookup.call(&call_id) {
                            if enrich_function_call_reasoning(&mut item, cached) {
                                enriched += 1;
                            }
                        }
                        seen_call_ids.insert(call_id);
                    }
                    new_items.push(item);
                }
                Some("function_call_output") => {
                    if let Some(group) = restore_group.take().filter(|group| !group.is_empty()) {
                        for (call_id, cached_item) in group {
                            seen_call_ids.insert(call_id);
                            new_items.push(cached_item);
                            restored += 1;
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
        requested_call_ids: &HashSet<String>,
    ) -> CachedLookup {
        let inner = self.inner.read().await;
        let previous = previous_response_id.and_then(|id| inner.responses.get(id).cloned());
        let fallback = inner.unique_fallback_calls(requested_call_ids, previous.as_ref());
        CachedLookup { previous, fallback }
    }
}

impl CodexChatHistoryInner {
    fn insert_calls(&mut self, response_id: &str, calls: Vec<(String, Value)>) -> usize {
        if !self.responses.contains_key(response_id) {
            self.response_order.push_back(response_id.to_string());
        }

        let cached_response = self.responses.entry(response_id.to_string()).or_default();
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
            self.index_call(&call_id, response_id);
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

    fn index_call(&mut self, call_id: &str, response_id: &str) {
        let response_ids = self.call_index.entry(call_id.to_string()).or_default();
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
        requested_call_ids: &HashSet<String>,
        previous: Option<&CachedResponse>,
    ) -> CachedResponse {
        let mut selected = HashMap::new();
        for call_id in requested_call_ids {
            if previous.is_some_and(|response| response.calls_by_id.contains_key(call_id)) {
                continue;
            }
            if let Some(item) = self.unique_call(call_id) {
                selected.insert(call_id.clone(), item.clone());
            }
        }

        let mut fallback = CachedResponse::default();
        for response_id in &self.response_order {
            let Some(response) = self.responses.get(response_id) else {
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

    fn unique_call(&self, call_id: &str) -> Option<&Value> {
        let response_ids = self.call_index.get(call_id)?;
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
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut current_response_id: Option<String> = None;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        inspect_sse_block(&block, &mut current_response_id, history.as_ref()).await;
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
                    .record_function_call(current_response_id.as_deref(), item)
                    .await;
            }
        }
        Some("response.completed") => {
            if let Some(response) = value.get("response") {
                history.record_response(response).await;
            }
        }
        _ => {}
    }
}

fn cached_function_call(item: &Value) -> Option<(String, Value)> {
    if item.get("type").and_then(|value| value.as_str()) != Some("function_call") {
        return None;
    }
    let call_id = response_item_call_id(item)?;
    Some((call_id, item.clone()))
}

fn enrich_function_call_reasoning(item: &mut Value, cached: &Value) -> bool {
    let mut changed = false;
    for key in ["reasoning_content", "reasoning"] {
        if item.get(key).is_some_and(|value| !is_empty_value(value)) {
            continue;
        }
        let Some(reasoning) = cached.get(key).filter(|value| !is_empty_value(value)) else {
            continue;
        };
        if let Some(object) = item.as_object_mut() {
            object.insert(key.to_string(), reasoning.clone());
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
            .record_response(&json!({
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
            }))
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

        assert_eq!(history.enrich_request(&mut request).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[tokio::test]
    async fn restores_unique_call_id_without_matching_previous_response() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
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
            }))
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
        assert_eq!(history.enrich_request(&mut missing_previous).await, 1);
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
        assert_eq!(history.enrich_request(&mut different_previous).await, 1);
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
                .record_response(&json!({
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
                }))
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
        assert_eq!(history.enrich_request(&mut missing_previous).await, 0);
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
        assert_eq!(history.enrich_request(&mut different_previous).await, 0);
        assert_eq!(
            different_previous["input"][0]["type"],
            "function_call_output"
        );
    }

    #[tokio::test]
    async fn enriches_existing_function_call_missing_reasoning() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
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
            }))
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

        assert_eq!(history.enrich_request(&mut request).await, 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input.len(), 2);
    }

    #[tokio::test]
    async fn restores_parallel_tool_calls_as_one_assistant_group() {
        let history = CodexChatHistoryStore::default();
        history
            .record_response(&json!({
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
            }))
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

        assert_eq!(history.enrich_request(&mut request).await, 2);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_2");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[3]["type"], "function_call_output");
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

        let output = record_responses_sse_stream(stream, history.clone())
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

        assert_eq!(history.enrich_request(&mut request).await, 1);
        assert_eq!(request["input"][0]["reasoning_content"], "Need a file.");
    }
}
