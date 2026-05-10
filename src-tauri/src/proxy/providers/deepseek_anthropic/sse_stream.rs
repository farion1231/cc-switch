use bytes::Bytes;
use futures::{Stream, StreamExt};

use crate::proxy::providers::deepseek_anthropic::sse_state::{
    transform_native_sse_block_event, SseBlockPolicyState,
};

pub fn patch_sse_event(
    event: &str,
    state: &mut SseBlockPolicyState,
    fake_model: &str,
    thinking_enabled: bool,
) -> Vec<String> {
    transform_native_sse_block_event(event, state, fake_model, thinking_enabled)
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

pub fn wrap_sse_stream<S>(
    upstream: S,
    fake_model: String,
    thinking_enabled: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let mut buffer: Vec<u8> = Vec::new();
    let mut state = SseBlockPolicyState::default();

    upstream.flat_map(move |chunk_result| {
        let out: Vec<Result<Bytes, std::io::Error>> = match chunk_result {
            Err(e) => vec![Err(e)],
            Ok(chunk) => {
                buffer.extend_from_slice(&chunk);
                let mut events_out: Vec<Result<Bytes, std::io::Error>> = Vec::new();

                loop {
                    if let Some(pos) = find_double_newline(&buffer) {
                        let event_bytes = buffer[..pos].to_vec();
                        buffer.drain(..pos + 2);

                        let event_str = match std::str::from_utf8(&event_bytes) {
                            Ok(s) => s.trim_end_matches('\n').to_string(),
                            Err(_) => {
                                continue;
                            }
                        };

                        let patched =
                            patch_sse_event(&event_str, &mut state, &fake_model, thinking_enabled);
                        for e in patched {
                            events_out.push(Ok(Bytes::from(format!("{}\n\n", e))));
                        }
                    } else {
                        break;
                    }
                }
                events_out
            }
        };
        futures::stream::iter(out)
    })
}

#[cfg(test)]
mod tests_sse_stream {
    use super::*;
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;

    fn make_state() -> SseBlockPolicyState {
        SseBlockPolicyState::default()
    }

    fn event(event_type: &str, data: serde_json::Value) -> String {
        format!("event: {}\ndata: {}", event_type, data)
    }

    #[test]
    fn test_patch_event_no_trailing_newline() {
        let mut state = make_state();
        let e = event("ping", json!({}));
        let result = patch_sse_event(&e, &mut state, "fake", false);
        assert_eq!(result.len(), 1);
        assert!(
            !result[0].ends_with('\n'),
            "patch_sse_event elements must not end with \\n"
        );
    }

    #[test]
    fn test_patch_event_message_start_rewrites_model() {
        let mut state = make_state();
        let e = event(
            "message_start",
            json!({"type": "message_start", "message": {"model": "deepseek-v4-pro"}}),
        );
        let result = patch_sse_event(&e, &mut state, "claude-opus-4-7", true);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("claude-opus-4-7"));
    }

    #[test]
    fn test_patch_event_dropped_returns_empty() {
        let mut state = make_state();
        let e = event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "thinking"}
            }),
        );
        let result = patch_sse_event(&e, &mut state, "fake", false);
        assert!(result.is_empty());
    }

    fn bytes_from_events(events: Vec<&str>) -> Vec<Bytes> {
        events
            .into_iter()
            .map(|e| Bytes::from(format!("{}\n\n", e)))
            .collect()
    }

    async fn collect_stream_output(
        chunks: Vec<Bytes>,
        fake_model: &str,
        thinking_enabled: bool,
    ) -> Vec<String> {
        let upstream =
            futures::stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>));
        let stream = wrap_sse_stream(upstream, fake_model.to_string(), thinking_enabled);
        stream
            .map(|r| String::from_utf8(r.unwrap().to_vec()).unwrap())
            .collect::<Vec<_>>()
            .await
    }

    #[tokio::test]
    async fn test_wrap_stream_each_event_ends_with_double_newline() {
        let raw = "event: ping\ndata: {}\n\n".to_string();
        let chunks = vec![Bytes::from(raw)];
        let output = collect_stream_output(chunks, "fake", false).await;
        for item in &output {
            assert!(
                item.ends_with("\n\n"),
                "each yielded item must end with \\n\\n: {:?}",
                item
            );
        }
    }

    #[tokio::test]
    async fn test_wrap_stream_chunk_split_across_boundary() {
        let full = "event: ping\ndata: {}\n\n".to_string();
        let mid = full.len() / 2;
        let chunks = vec![
            Bytes::from(full[..mid].to_string()),
            Bytes::from(full[mid..].to_string()),
        ];
        let output = collect_stream_output(chunks, "fake", false).await;
        assert!(
            !output.is_empty(),
            "event split across chunks should still be emitted"
        );
        let full_output = output.join("");
        assert!(full_output.contains("ping"));
    }

    #[tokio::test]
    async fn test_wrap_stream_drops_thinking_events_when_disabled() {
        let start_event = format!(
            "event: content_block_start\ndata: {}\n\n",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking"}})
        );
        let chunks = vec![Bytes::from(start_event)];
        let output = collect_stream_output(chunks, "fake", false).await;
        assert!(
            output.is_empty() || !output.iter().any(|s| s.contains("thinking")),
            "thinking block should be dropped when disabled"
        );
    }

    #[tokio::test]
    async fn test_wrap_stream_model_rewritten_in_message_start() {
        let evt = format!(
            "event: message_start\ndata: {}\n\n",
            json!({"type":"message_start","message":{"model":"deepseek-v4-pro"}})
        );
        let chunks = vec![Bytes::from(evt)];
        let output = collect_stream_output(chunks, "claude-sonnet-4-6", false).await;
        let joined = output.join("");
        assert!(
            joined.contains("claude-sonnet-4-6"),
            "model should be rewritten"
        );
        assert!(
            !joined.contains("deepseek-v4-pro"),
            "original model should not appear"
        );
    }

    #[tokio::test]
    async fn test_wrap_stream_multi_event_each_has_terminator() {
        let events = vec![
            "event: ping\ndata: {}\n\n".to_string(),
            format!(
                "event: message_start\ndata: {}\n\n",
                json!({"type":"message_start","message":{"model":"deepseek-v4-pro"}})
            ),
        ];
        let chunks: Vec<Bytes> = events.iter().map(|e| Bytes::from(e.clone())).collect();
        let output = collect_stream_output(chunks, "fake", false).await;
        for item in &output {
            assert!(
                item.ends_with("\n\n"),
                "every yielded event must have \\n\\n: {:?}",
                item
            );
        }
    }
}
