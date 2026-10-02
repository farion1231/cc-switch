//! GitHub Copilot native Responses SSE identity compatibility.
//!
//! Copilot can encrypt the same logical response or output-item ID differently
//! in every lifecycle event. Responses clients expect those IDs to remain
//! stable, so canonicalize them within one stream while leaving the event
//! lifecycle and payload content intact.

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

#[derive(Debug, Default)]
struct CopilotResponsesIdState {
    response_id: Option<String>,
    item_ids: HashMap<u64, String>,
}

impl CopilotResponsesIdState {
    fn stabilize_event(&mut self, event: &mut Value) -> bool {
        let mut changed = false;

        if let Some(response_id) = event.get_mut("response_id") {
            changed |= stabilize_id(&mut self.response_id, response_id);
        }

        if let Some(response) = event.get_mut("response") {
            if let Some(response_id) = response.get_mut("id") {
                changed |= stabilize_id(&mut self.response_id, response_id);
            }

            if let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) {
                for (output_index, item) in output.iter_mut().enumerate() {
                    if let Some(item_id) = item.get_mut("id") {
                        changed |= self.stabilize_item_id(output_index as u64, item_id);
                    }
                }
            }
        }

        let Some(output_index) = event.get("output_index").and_then(Value::as_u64) else {
            return changed;
        };

        if let Some(item_id) = event.get_mut("item_id") {
            changed |= self.stabilize_item_id(output_index, item_id);
        }
        if let Some(item_id) = event.get_mut("item").and_then(|item| item.get_mut("id")) {
            changed |= self.stabilize_item_id(output_index, item_id);
        }

        changed
    }

    fn stabilize_item_id(&mut self, output_index: u64, value: &mut Value) -> bool {
        let Some(current) = value.as_str().filter(|id| !id.is_empty()) else {
            return false;
        };

        match self.item_ids.get(&output_index) {
            Some(canonical) if canonical != current => {
                *value = Value::String(canonical.clone());
                true
            }
            Some(_) => false,
            None => {
                self.item_ids.insert(output_index, current.to_string());
                false
            }
        }
    }
}

fn stabilize_id(canonical: &mut Option<String>, value: &mut Value) -> bool {
    let Some(current) = value.as_str().filter(|id| !id.is_empty()) else {
        return false;
    };

    match canonical {
        Some(canonical) if canonical != current => {
            *value = Value::String(canonical.clone());
            true
        }
        Some(_) => false,
        None => {
            *canonical = Some(current.to_string());
            false
        }
    }
}

pub(crate) fn create_copilot_responses_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut state = CopilotResponsesIdState::default();

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        yield Ok(rewrite_sse_block(&block, &mut state));
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            yield Ok(rewrite_sse_block(&tail, &mut state));
        }
    }
}

fn rewrite_sse_block(block: &str, state: &mut CopilotResponsesIdState) -> Bytes {
    let data_parts: Vec<_> = block
        .lines()
        .filter_map(|line| strip_sse_field(line, "data"))
        .collect();
    if data_parts.is_empty() {
        return framed_block(block);
    }

    let data = data_parts.join("\n");
    if data.trim() == "[DONE]" {
        return framed_block(block);
    }

    let mut event: Value = match serde_json::from_str(&data) {
        Ok(event) => event,
        Err(_) => return framed_block(block),
    };
    if !state.stabilize_event(&mut event) {
        return framed_block(block);
    }

    let serialized = match serde_json::to_string(&event) {
        Ok(serialized) => serialized,
        Err(_) => return framed_block(block),
    };
    let mut rewritten = String::new();
    let mut emitted_data = false;
    for line in block.lines() {
        if strip_sse_field(line, "data").is_some() {
            if !emitted_data {
                rewritten.push_str("data: ");
                rewritten.push_str(&serialized);
                rewritten.push('\n');
                emitted_data = true;
            }
        } else {
            rewritten.push_str(line);
            rewritten.push('\n');
        }
    }
    rewritten.push('\n');
    Bytes::from(rewritten)
}

fn framed_block(block: &str) -> Bytes {
    Bytes::from(format!("{block}\n\n"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use futures::{stream, StreamExt};
    use serde_json::Value;

    use super::*;

    const UNSTABLE_TEXT_STREAM: &str = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_created\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        "event: response.in_progress\n",
        "data: {\"type\":\"response.in_progress\",\"sequence_number\":1,\"response\":{\"id\":\"resp_progress\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"sequence_number\":2,\"output_index\":0,\"item\":{\"id\":\"msg_added\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"sequence_number\":3,\"item_id\":\"msg_part\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":4,\"item_id\":\"msg_delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"hello\"}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"sequence_number\":5,\"item_id\":\"msg_text_done\",\"output_index\":0,\"content_index\":0,\"text\":\"hello\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"sequence_number\":6,\"output_index\":0,\"item\":{\"id\":\"msg_item_done\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"sequence_number\":7,\"response\":{\"id\":\"resp_completed\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_completed\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n",
    );

    async fn convert(input: &str) -> String {
        convert_chunks(vec![Bytes::copy_from_slice(input.as_bytes())]).await
    }

    async fn convert_chunks(chunks: Vec<Bytes>) -> String {
        let upstream = stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>));
        let chunks: Vec<_> = create_copilot_responses_sse_stream(upstream)
            .collect()
            .await;
        String::from_utf8(
            chunks
                .into_iter()
                .map(Result::unwrap)
                .flat_map(|bytes| bytes.to_vec())
                .collect(),
        )
        .unwrap()
    }

    fn events(input: &str) -> Vec<Value> {
        input
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .collect::<Vec<_>>()
                    .join("\n");
                (!data.is_empty()).then(|| serde_json::from_str(&data).unwrap())
            })
            .collect()
    }

    #[tokio::test]
    async fn stabilizes_response_and_item_ids_without_changing_payload_semantics() {
        let output = convert(UNSTABLE_TEXT_STREAM).await;
        let events = events(&output);

        let response_ids: HashSet<_> = events
            .iter()
            .filter_map(|event| event.pointer("/response/id").and_then(Value::as_str))
            .collect();
        let mut item_ids = HashSet::new();
        for event in &events {
            if let Some(id) = event.get("item_id").and_then(Value::as_str) {
                item_ids.insert(id);
            }
            if let Some(id) = event.pointer("/item/id").and_then(Value::as_str) {
                item_ids.insert(id);
            }
            if let Some(items) = event.pointer("/response/output").and_then(Value::as_array) {
                item_ids.extend(
                    items
                        .iter()
                        .filter_map(|item| item.get("id").and_then(Value::as_str)),
                );
            }
        }

        assert_eq!(response_ids.len(), 1);
        assert_eq!(item_ids.len(), 1);
        assert_eq!(events.len(), 8);
        assert_eq!(
            events
                .iter()
                .filter_map(|event| event.get("sequence_number").and_then(Value::as_u64))
                .collect::<Vec<_>>(),
            (0..8).collect::<Vec<_>>()
        );
        assert_eq!(events[4]["delta"], "hello");
        assert_eq!(events[5]["text"], "hello");
        assert_eq!(
            events[7]["response"]["output"][0]["content"][0]["text"],
            "hello"
        );
        assert_eq!(
            events[7]["response"]["usage"],
            serde_json::json!({
                "input_tokens": 4,
                "output_tokens": 1,
                "total_tokens": 5
            })
        );
    }

    #[tokio::test]
    async fn keeps_reasoning_message_and_function_call_ids_distinct() {
        let input = concat!(
                    "event: response.created\n",
                    "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_1\",\"output\":[]}}\n\n",
                    "event: response.output_item.added\n",
                    "data: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"id\":\"reasoning_added\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
                    "event: response.reasoning_summary_text.delta\n",
                    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"sequence_number\":2,\"item_id\":\"reasoning_delta\",\"output_index\":0,\"summary_index\":0,\"delta\":\"think\"}\n\n",
                    "event: response.output_item.done\n",
                    "data: {\"type\":\"response.output_item.done\",\"sequence_number\":3,\"output_index\":0,\"item\":{\"id\":\"reasoning_done\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"think\"}]}}\n\n",
                    "event: response.output_item.added\n",
                    "data: {\"type\":\"response.output_item.added\",\"sequence_number\":4,\"output_index\":1,\"item\":{\"id\":\"message_added\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
                    "event: response.output_text.delta\n",
                    "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":5,\"item_id\":\"message_delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"answer\"}\n\n",
                    "event: response.output_item.done\n",
                    "data: {\"type\":\"response.output_item.done\",\"sequence_number\":6,\"output_index\":1,\"item\":{\"id\":\"message_done\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"answer\"}]}}\n\n",
                    "event: response.output_item.added\n",
                    "data: {\"type\":\"response.output_item.added\",\"sequence_number\":7,\"output_index\":2,\"item\":{\"id\":\"call_added\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
                    "event: response.function_call_arguments.delta\n",
                    "data: {\"type\":\"response.function_call_arguments.delta\",\"sequence_number\":8,\"item_id\":\"call_delta\",\"output_index\":2,\"delta\":\"{}\"}\n\n",
                    "event: response.output_item.done\n",
                    "data: {\"type\":\"response.output_item.done\",\"sequence_number\":9,\"output_index\":2,\"item\":{\"id\":\"call_done\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"{}\"}}\n\n",
                    "event: response.completed\n",
                    "data: {\"type\":\"response.completed\",\"sequence_number\":10,\"response\":{\"id\":\"resp_2\",\"output\":[{\"id\":\"reasoning_completed\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"think\"}]},{\"id\":\"message_completed\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"answer\"}]},{\"id\":\"call_completed\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"{}\"}]}}\n\n",
                );

        let output = convert(input).await;
        let events = events(&output);
        let completed = events.last().unwrap();

        assert_eq!(completed["response"]["id"], "resp_1");
        assert_eq!(completed["response"]["output"][0]["id"], "reasoning_added");
        assert_eq!(completed["response"]["output"][1]["id"], "message_added");
        assert_eq!(completed["response"]["output"][2]["id"], "call_added");
        assert_ne!(
            completed["response"]["output"][0]["id"],
            completed["response"]["output"][1]["id"]
        );
        assert_ne!(
            completed["response"]["output"][1]["id"],
            completed["response"]["output"][2]["id"]
        );
        assert_eq!(events[2]["item_id"], "reasoning_added");
        assert_eq!(events[5]["item_id"], "message_added");
        assert_eq!(events[8]["item_id"], "call_added");
        assert_eq!(completed["response"]["output"][2]["call_id"], "call_1");
    }

    #[tokio::test]
    async fn handles_split_utf8_chunks_and_unterminated_tail() {
        let input = concat!(
                    "event: response.created\r\n",
                    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_first\",\"output\":[]}}\r\n\r\n",
                    "event: response.output_item.added\r\n",
                    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_first\",\"type\":\"message\",\"content\":[]}}\r\n\r\n",
                    "event: response.output_text.delta\r\n",
                    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"item_id\":\"msg_delta\",\"delta\":\"你好\"}\r\n\r\n",
                    "event: response.completed\r\n",
                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_last\",\"output\":[{\"id\":\"msg_last\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"你好\"}]}]}}"
                );
        let bytes = input.as_bytes();
        let chinese = bytes
            .windows("你".len())
            .position(|window| window == "你".as_bytes())
            .unwrap();
        let chunks = vec![
            Bytes::copy_from_slice(&bytes[..chinese + 1]),
            Bytes::copy_from_slice(&bytes[chinese + 1..chinese + 2]),
            Bytes::copy_from_slice(&bytes[chinese + 2..]),
        ];

        let output = convert_chunks(chunks).await;
        let events = events(&output);

        assert!(!output.contains('\u{fffd}'));
        assert_eq!(events[2]["delta"], "你好");
        assert_eq!(
            events[3]["response"]["output"][0]["content"][0]["text"],
            "你好"
        );
        assert_eq!(events[3]["response"]["id"], "resp_first");
        assert_eq!(events[2]["item_id"], "msg_first");
        assert_eq!(events[3]["response"]["output"][0]["id"], "msg_first");
    }

    #[tokio::test]
    async fn passes_comments_done_markers_and_malformed_events_through() {
        let input = concat!(
            ": keep-alive\n\n",
            "event: vendor.extension\n",
            "data: not-json\n\n",
            "data: [DONE]\n\n",
        );

        assert_eq!(convert(input).await, input);
    }

    #[tokio::test]
    async fn propagates_upstream_stream_errors_after_completed_blocks() {
        let upstream = stream::iter(vec![
                    Ok::<_, std::io::Error>(Bytes::from_static(
                        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
                    )),
                    Err(std::io::Error::other("boom")),
                ]);
        let results: Vec<_> = create_copilot_responses_sse_stream(upstream)
            .collect()
            .await;

        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert_eq!(results[1].as_ref().unwrap_err().to_string(), "boom");
    }
}
