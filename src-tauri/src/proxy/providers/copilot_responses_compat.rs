//! Copilot can change encrypted IDs between lifecycle events. Keep them stable
//! within a Responses stream so Codex does not append terminal snapshots as new items.

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::proxy::{
    hyper_client::ProxyResponse,
    response_processor::{is_sse_response, strip_entity_headers_for_rebuilt_body},
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
};

pub(crate) fn stabilize_response(
    response: ProxyResponse,
    request_is_stream: bool,
) -> ProxyResponse {
    if !response.status().is_success() || !is_sse_response(&response, request_is_stream) {
        return response;
    }
    let mut headers = response.headers().clone();
    strip_entity_headers_for_rebuilt_body(&mut headers);
    // Downstream compaction also uses this header to select its SSE path.
    headers
        .entry(http::header::CONTENT_TYPE)
        .or_insert(http::HeaderValue::from_static("text/event-stream"));
    ProxyResponse::streamed(
        response.status(),
        headers,
        create_copilot_responses_sse_stream(response.bytes_stream()),
    )
}

#[derive(Debug, Default)]
struct CopilotResponsesIdState {
    response_id: String,
    item_ids: HashMap<u64, String>,
}

impl CopilotResponsesIdState {
    fn stabilize_event(&mut self, event: &mut Value) -> bool {
        let mut changed = false;

        for path in ["/response_id", "/response/id"] {
            if let Some(id) = event.pointer_mut(path) {
                changed |= stabilize_id(&mut self.response_id, id);
            }
        }
        if let Some(output) = event
            .pointer_mut("/response/output")
            .and_then(Value::as_array_mut)
        {
            for (index, item) in output.iter_mut().enumerate() {
                if let Some(id) = item.get_mut("id") {
                    changed |= stabilize_id(self.item_ids.entry(index as u64).or_default(), id);
                }
            }
        }
        if let Some(index) = event.get("output_index").and_then(Value::as_u64) {
            for path in ["/item_id", "/item/id"] {
                if let Some(id) = event.pointer_mut(path) {
                    changed |= stabilize_id(self.item_ids.entry(index).or_default(), id);
                }
            }
        }
        changed
    }
}

fn stabilize_id(canonical: &mut String, value: &mut Value) -> bool {
    let Some(current) = value.as_str().filter(|id| !id.is_empty()) else {
        return false;
    };

    if canonical.is_empty() {
        *canonical = current.to_string();
        false
    } else if canonical.as_str() == current {
        false
    } else {
        *value = Value::String(canonical.clone());
        true
    }
}

fn create_copilot_responses_sse_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::try_stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut state = CopilotResponsesIdState::default();

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            append_utf8_safe(&mut buffer, &mut utf8_remainder, &chunk?);
            while let Some(block) = take_sse_block(&mut buffer) {
                if !block.trim().is_empty() {
                    yield rewrite_sse_block(&block, &mut state);
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        if !buffer.trim().is_empty() {
            yield rewrite_sse_block(&buffer, &mut state);
        }
    }
}

fn rewrite_sse_block(block: &str, state: &mut CopilotResponsesIdState) -> Bytes {
    let data_parts: Vec<_> = block
        .lines()
        .filter_map(|line| strip_sse_field(line, "data"))
        .collect();
    let data = data_parts.join("\n");
    let mut event: Value = match serde_json::from_str(&data) {
        Ok(event) => event,
        Err(_) => return framed_block(block),
    };
    if !state.stabilize_event(&mut event) {
        return framed_block(block);
    }

    let mut rewritten = String::new();
    let mut emitted_data = false;
    for line in block.lines() {
        if strip_sse_field(line, "data").is_some() {
            if !emitted_data {
                rewritten.push_str("data: ");
                rewritten.push_str(&event.to_string());
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
    use super::super::codex_compaction::{
        create_native_compaction_sse_stream, decode_compaction_summary,
    };
    use futures::{stream, StreamExt};
    use http::{header, HeaderMap, HeaderValue, StatusCode};
    use serde_json::{json, Value};

    use super::*;

    fn response(
        stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    ) -> ProxyResponse {
        let headers = HeaderMap::from_iter([
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            ),
            (header::CONTENT_LENGTH, HeaderValue::from_static("999")),
        ]);
        stabilize_response(
            ProxyResponse::streamed(StatusCode::OK, headers, stream),
            true,
        )
    }

    async fn convert(input: &str) -> String {
        let chunks: Vec<_> = input
            .as_bytes()
            .chunks(1)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect();
        let response = response(stream::iter(chunks));
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        let chunks: Vec<_> = response.bytes_stream().collect().await;
        String::from_utf8(
            chunks
                .into_iter()
                .map(Result::unwrap)
                .flat_map(|bytes| bytes.to_vec())
                .collect(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn codex_receives_one_coherent_reply_with_reasoning_text_tools_and_usage() {
        let input = r#"event: response.created
data: {"type":"response.created","response":{"id":"resp_first","output":[]}}

data: {"type":"response.output_item.added","output_index":0,"item":{"id":"reasoning_first","type":"reasoning","summary":[]}}

data: {"type":"response.reasoning_summary_text.delta","item_id":"reasoning_delta","output_index":0,"summary_index":0,"delta":"think"}

data: {"type":"response.output_item.added","output_index":1,"item":{"id":"message_first","type":"message","role":"assistant","content":[]}}

data: {"type":"response.output_text.delta","item_id":"message_delta","output_index":1,"content_index":0,"delta":"你好"}

data: {"type":"response.output_item.done","output_index":1,"item":{"id":"message_done","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]}}

data: {"type":"response.output_item.added","output_index":2,"item":{"id":"tool_first","type":"function_call","call_id":"call_1","name":"lookup","arguments":""}}

data: {"type":"response.function_call_arguments.delta","item_id":"tool_delta","output_index":2,"delta":"{}"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_completed","status":"completed","output":[{"id":"reasoning_completed","type":"reasoning","summary":[{"type":"summary_text","text":"think"}]},{"id":"message_completed","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]},{"id":"tool_completed","type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}],"usage":{"input_tokens":4,"output_tokens":2,"total_tokens":6}}}"#;
        let output = convert(&input.replace('\n', "\r\n")).await;
        let received: Vec<Value> = output
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str(data).unwrap())
            .collect();
        assert!(output.contains("event: response.completed\ndata: "));
        assert_eq!(
            Value::Array(received),
            json!([
                {"type":"response.created","response":{"id":"resp_first","output":[]}},
                {"type":"response.output_item.added","output_index":0,"item":{"id":"reasoning_first","type":"reasoning","summary":[]}},
                {"type":"response.reasoning_summary_text.delta","item_id":"reasoning_first","output_index":0,"summary_index":0,"delta":"think"},
                {"type":"response.output_item.added","output_index":1,"item":{"id":"message_first","type":"message","role":"assistant","content":[]}},
                {"type":"response.output_text.delta","item_id":"message_first","output_index":1,"content_index":0,"delta":"你好"},
                {"type":"response.output_item.done","output_index":1,"item":{"id":"message_first","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]}},
                {"type":"response.output_item.added","output_index":2,"item":{"id":"tool_first","type":"function_call","call_id":"call_1","name":"lookup","arguments":""}},
                {"type":"response.function_call_arguments.delta","item_id":"tool_first","output_index":2,"delta":"{}"},
                {"type":"response.completed","response":{"id":"resp_first","status":"completed","output":[
                    {"id":"reasoning_first","type":"reasoning","summary":[{"type":"summary_text","text":"think"}]},
                    {"id":"message_first","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]},
                    {"id":"tool_first","type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}
                ],"usage":{"input_tokens":4,"output_tokens":2,"total_tokens":6}}}
            ])
        );
    }

    #[tokio::test]
    async fn already_compatible_streams_pass_through_unchanged() {
        let input = concat!(
            ": keep-alive\n\n",
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"output\":[]}}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"output\":[]}}\n\n",
            "event: vendor.extension\n",
            "data: not-json\n\n",
            "data: [DONE]\n\n",
        );

        assert_eq!(convert(input).await, input);
    }

    #[tokio::test]
    async fn compaction_preserves_message_identity_and_adds_one_summary() {
        let input = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_first\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_first\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_done\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"summary\"}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done\",\"output\":[{\"id\":\"msg_completed\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"summary\"}]}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n",
        );
        for content_type in [Some("text/event-stream"), None] {
            let mut headers = HeaderMap::new();
            if let Some(content_type) = content_type {
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            }
            let upstream = ProxyResponse::buffered(StatusCode::OK, headers, Bytes::from(input));
            let stabilized = stabilize_response(upstream, true);
            assert!(stabilized.is_sse());
            let chunks: Vec<_> = create_native_compaction_sse_stream(stabilized.bytes_stream())
                .collect()
                .await;
            let output: Vec<u8> = chunks
                .into_iter()
                .flat_map(|chunk| chunk.unwrap())
                .collect();
            let output = String::from_utf8(output).unwrap();
            let events: Vec<Value> = output
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .map(|data| serde_json::from_str(data).unwrap())
                .collect();
            assert_eq!(events[2]["item"]["id"], "msg_first", "{content_type:?}");
            assert_eq!(events[3]["item"]["type"], "compaction");
            assert_eq!(events[3]["output_index"], 1);
            let completed = &events[4]["response"];
            assert_eq!(completed["id"], "resp_first");
            assert_eq!(completed["output"][0]["id"], "msg_first");
            assert_eq!(completed["output"].as_array().unwrap().len(), 2);
            assert_eq!(completed["output"][1], events[3]["item"]);
            let encrypted = completed["output"][1]["encrypted_content"]
                .as_str()
                .unwrap();
            assert_eq!(
                decode_compaction_summary(encrypted).as_deref(),
                Some("summary")
            );
            assert_eq!(
                completed["usage"],
                json!({"input_tokens":4,"output_tokens":1})
            );
        }
    }

    #[tokio::test]
    async fn upstream_failure_reaches_the_client_after_delivered_events() {
        let upstream = stream::iter(vec![
                    Ok::<_, std::io::Error>(Bytes::from_static(
                        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
                    )),
                    Err(std::io::Error::other("boom")),
                ]);
        let results: Vec<_> = response(upstream).bytes_stream().collect().await;

        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert_eq!(results[1].as_ref().unwrap_err().to_string(), "boom");
    }
}
