//! Native web-search handles are scoped to their issuing backend. Preserve the
//! visible activity as messages when sharing history across upstreams; keep the
//! original in migration backups. Never inspect or rewrite reasoning ciphertext.
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

pub(crate) fn project_item(item: &Value) -> Option<Value> {
    if item.get("type").and_then(Value::as_str) != Some("web_search_call") {
        return None;
    }
    let completed = item.get("status").and_then(Value::as_str) == Some("completed");
    let mut message = json!({
        "type": "message", "role": "assistant", "phase": "commentary",
        "status": if completed { "completed" } else { "in_progress" },
        "content": if completed { json!([{
            "type": "output_text", "annotations": [],
            "text": format!("Historical web activity completed: {}. This records prior activity, not a new search result.",
                item.get("action").unwrap_or(&Value::Null))
        }]) } else { json!([]) }
    });
    if let Some(id) = item.get("id").and_then(Value::as_str) {
        message["id"] = json!(format!("msg_ccswitch_{id}"));
    }
    Some(message)
}

pub(crate) fn project_request(body: &mut Value) -> usize {
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };
    project_items(items)
}

fn project_items(items: &mut [Value]) -> usize {
    let mut changed = 0;
    for item in items {
        if let Some(message) = project_item(item) {
            *item = message;
            changed += 1;
        }
    }
    changed
}

pub(crate) fn project_response(body: &mut Value) -> usize {
    body.get_mut("output")
        .and_then(Value::as_array_mut)
        .map(|items| project_items(items))
        .unwrap_or(0)
}

fn project_block(block: &str) -> Bytes {
    let data: String = block
        .lines()
        .filter_map(|line| super::sse::strip_sse_field(line, "data"))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(mut event) = serde_json::from_str::<Value>(&data) else {
        return Bytes::from(format!("{block}\n\n"));
    };
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    if kind.starts_with("response.web_search_call.") {
        return Bytes::new();
    }
    let mut changed = false;
    if let Some(item) = event.get_mut("item") {
        if let Some(message) = project_item(item) {
            *item = message;
            changed = true;
        }
    }
    if let Some(response) = event.get_mut("response") {
        changed |= project_response(response) > 0;
    }
    if changed {
        rewrite_sse_json_data(block, &event.to_string())
    } else {
        Bytes::from(format!("{block}\n\n"))
    }
}

fn rewrite_sse_json_data(block: &str, data: &str) -> Bytes {
    let mut out = String::new();
    let mut wrote_data = false;
    for line in block.lines() {
        if super::sse::strip_sse_field(line, "data").is_some() {
            if !wrote_data {
                out.push_str("data: ");
                out.push_str(data);
                out.push('\n');
                wrote_data = true;
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('\n');
    Bytes::from(out)
}

pub(crate) fn project_stream<S, E>(
    stream: S,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    async_stream::stream! {
        futures::pin_mut!(stream);
        let mut buffer = String::new();
        let mut remainder = Vec::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    super::sse::append_utf8_safe(&mut buffer, &mut remainder, &bytes);
                    while let Some(block) = super::sse::take_sse_block(&mut buffer) {
                        yield Ok(project_block(&block));
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            }
        }
        if !remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&remainder));
        }
        if !buffer.trim().is_empty() { yield Ok(project_block(&buffer)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_backend_scoped_search_handles_are_projected() {
        let reasoning = json!({"type":"reasoning","encrypted_content":"opaque"});
        let tool = json!({"type":"function_call_output","call_id":"call_1","output":"evidence"});
        let mut body = json!({"input":[
            {"type":"web_search_call","id":"ws_1","status":"completed",
             "action":{"type":"search","query":"evidence"}}, reasoning, tool
        ]});
        assert_eq!(project_request(&mut body), 1);
        assert_eq!(body["input"][0]["type"], "message");
        assert!(body["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("evidence"));
        assert_eq!(body["input"][1], reasoning);
        assert_eq!(body["input"][2], tool);
        assert_eq!(project_request(&mut body), 0);
    }

    #[tokio::test]
    async fn fragmented_sse_preserves_completion_usage_and_other_items() {
        let search = json!({"type":"web_search_call","id":"ws_1","status":"completed",
                            "action":{"type":"search","query":"evidence"}});
        let wire = format!(
            "id: event-1\nevent: response.output_item.done\ndata: {}\n\n\
                            event: response.completed\ndata: {}\n\n",
            json!({"type":"response.output_item.done","item":search}),
            json!({"type":"response.completed","response":{"output":[search],"usage":{"total_tokens":42}}})
        );
        let chunks: Vec<_> = wire
            .as_bytes()
            .chunks(7)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect();
        let out = project_stream(futures::stream::iter(chunks));
        futures::pin_mut!(out);
        let mut bytes = Vec::new();
        while let Some(chunk) = out.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("id: event-1"));
        assert!(text.contains("response.completed"));
        assert!(text.contains("\"total_tokens\":42"));
        assert!(!text.contains("\"type\":\"web_search_call\""));
        assert_eq!(text.matches("msg_ccswitch_ws_1").count(), 2);
    }

    #[test]
    fn nonstream_projection_preserves_usage_reasoning_and_tool_results() {
        let reasoning = json!({"type":"reasoning","encrypted_content":"opaque"});
        let tool = json!({"type":"function_call_output","call_id":"call_1","output":"evidence"});
        let mut response = json!({
            "id":"resp_1","status":"completed","usage":{"total_tokens":42},
            "output":[{"type":"web_search_call","status":"completed",
                       "action":{"type":"open_page","url":"https://example.com"}}, reasoning, tool]
        });
        assert_eq!(project_response(&mut response), 1);
        assert_eq!(response["id"], "resp_1");
        assert_eq!(response["usage"]["total_tokens"], 42);
        assert_eq!(response["output"][1], reasoning);
        assert_eq!(response["output"][2], tool);
        assert!(response["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("https://example.com"));
        let once = response.clone();
        assert_eq!(project_response(&mut response), 0);
        assert_eq!(response, once);
    }
}
