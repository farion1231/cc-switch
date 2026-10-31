//! Keep the Claude Desktop protocol model identity stable across process resumes.
//!
//! Clients can persist assistant model IDs with their history. Keep the response
//! identity consistent with the requested Claude alias for an exact configured
//! Codex mapping, without relying on a process-local alias association. Apply
//! this only downstream of usage collection so accounting retains the real model.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct ClientModelAlias {
    requested: String,
    mapped: String,
}

impl ClientModelAlias {
    pub(crate) fn for_request(
        app_type: &str,
        codex_oauth: bool,
        api_format: &str,
        requested: &str,
        mapped: Option<&str>,
    ) -> Option<Self> {
        let mapped = mapped?;
        if app_type != "claude-desktop"
            || !codex_oauth
            || api_format != "openai_responses"
            || !requested.starts_with("claude-")
            || mapped.is_empty()
            || mapped == requested
        {
            return None;
        }
        Some(Self {
            requested: requested.to_string(),
            mapped: mapped.to_string(),
        })
    }

    pub(crate) fn rewrite_message(&self, message: &mut Value) -> bool {
        // A real upstream fallback must remain visible; do not label it as the
        // configured model or allow its thinking to masquerade as that model.
        if message.get("model").and_then(Value::as_str) != Some(self.mapped.as_str()) {
            return false;
        }
        message["model"] = Value::String(self.requested.clone());
        true
    }

    fn rewrite_converted_sse(&self, bytes: Bytes) -> Bytes {
        // The Anthropic converters emit one complete event per chunk. Avoid
        // touching content deltas, opaque reasoning signatures or usage events.
        let Some(data) = bytes.strip_prefix(b"event: message_start\ndata: ") else {
            return bytes;
        };
        let Some(data) = data.strip_suffix(b"\n\n") else {
            return bytes;
        };
        let Ok(mut event) = serde_json::from_slice::<Value>(data) else {
            return bytes;
        };
        if event.get("type").and_then(Value::as_str) != Some("message_start") {
            return bytes;
        }
        let Some(message) = event.get_mut("message") else {
            return bytes;
        };
        if !self.rewrite_message(message) {
            return bytes;
        }
        let Ok(data) = serde_json::to_string(&event) else {
            return bytes;
        };
        Bytes::from(format!("event: message_start\ndata: {data}\n\n"))
    }
}

pub(crate) fn alias_stream<S, E>(
    stream: S,
    alias: Option<ClientModelAlias>,
) -> impl Stream<Item = Result<Bytes, E>>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    stream.map(move |chunk| {
        chunk.map(|bytes| match &alias {
            Some(alias) => alias.rewrite_converted_sse(bytes),
            None => bytes,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn alias() -> ClientModelAlias {
        ClientModelAlias::for_request(
            "claude-desktop",
            true,
            "openai_responses",
            "claude-opus-4-6",
            Some("gpt-5.4"),
        )
        .unwrap()
    }

    #[test]
    fn only_exact_desktop_codex_mapping_is_eligible() {
        for (app, oauth, format, requested, mapped) in [
            (
                "claude",
                true,
                "openai_responses",
                "claude-opus-4-6",
                Some("gpt-5.4"),
            ),
            (
                "claude-desktop",
                false,
                "openai_responses",
                "claude-opus-4-6",
                Some("gpt-5.4"),
            ),
            (
                "claude-desktop",
                true,
                "openai_chat",
                "claude-opus-4-6",
                Some("gpt-5.4"),
            ),
            (
                "claude-desktop",
                true,
                "openai_responses",
                "gpt-5.4",
                Some("gpt-5.4"),
            ),
            (
                "claude-desktop",
                true,
                "openai_responses",
                "claude-opus-4-6",
                None,
            ),
        ] {
            assert!(ClientModelAlias::for_request(app, oauth, format, requested, mapped).is_none());
        }
    }

    #[test]
    fn message_alias_preserves_usage_id_and_reasoning() {
        let original = json!({"id":"resp_local","model":"gpt-5.4","usage":{"input_tokens":123,"cache_read_input_tokens":100},"content":[{"type":"redacted_thinking","data":"opaque-synthetic"},{"type":"text","text":"OK"}]});
        let mut actual = original.clone();
        assert!(alias().rewrite_message(&mut actual));
        let mut expected = original;
        expected["model"] = json!("claude-opus-4-6");
        assert_eq!(actual, expected);
    }

    #[test]
    fn real_fallback_model_is_not_aliased() {
        let mut message = json!({"model":"gpt-5.4-mini"});
        assert!(!alias().rewrite_message(&mut message));
        assert_eq!(message["model"], "gpt-5.4-mini");
    }

    #[test]
    fn sse_changes_only_message_start_model() {
        let original = Bytes::from_static(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"resp_local\",\"model\":\"gpt-5.4\",\"usage\":{\"input_tokens\":123}}}\n\n");
        let result = alias().rewrite_converted_sse(original);
        let mut event: Value = serde_json::from_slice(
            result
                .strip_prefix(b"event: message_start\ndata: ")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["message"]["model"], "claude-opus-4-6");
        event["message"]["model"] = json!("gpt-5.4");
        assert_eq!(
            event,
            json!({"type":"message_start","message":{"id":"resp_local","model":"gpt-5.4","usage":{"input_tokens":123}}})
        );
        for bytes in [
            Bytes::from_static(b"event: content_block_delta\ndata: {\"delta\":{\"type\":\"signature_delta\",\"signature\":\"gpt-5.4\"}}\n\n"),
            Bytes::from_static(b"event: message_start\ndata: broken\n\n"),
            Bytes::from_static(b": keep-alive\n\n"),
        ] {
            assert_eq!(alias().rewrite_converted_sse(bytes.clone()), bytes);
        }
    }

    #[tokio::test]
    async fn responses_converter_retains_reasoning_and_usage_through_aliasing() {
        use super::super::providers::streaming_responses::create_anthropic_sse_stream_from_responses;

        let input = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_local\",\"model\":\"gpt-5.4\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_local\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
            "event: response.reasoning_summary_part.added\n",
            "data: {\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"rs_local\",\"output_index\":0,\"summary_index\":0,\"part\":{\"type\":\"summary_text\",\"text\":\"\"}}\n\n",
            "event: response.reasoning_summary_text.delta\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_local\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Need a tool.\"}\n\n",
            "event: response.reasoning_summary_text.done\n",
            "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"rs_local\",\"output_index\":0,\"summary_index\":0,\"text\":\"Need a tool.\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_local\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Need a tool.\"}],\"encrypted_content\":\"opaque-synthetic\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":123,\"output_tokens\":7,\"input_tokens_details\":{\"cached_tokens\":100}}}}\n\n"
        );
        let upstream = futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input))]);
        let converted = create_anthropic_sse_stream_from_responses(upstream)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let source = futures::stream::iter(converted.iter().cloned().map(Ok::<_, std::io::Error>));
        let aliased = alias_stream(source, Some(alias()))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let starts = converted
            .iter()
            .filter(|chunk| chunk.starts_with(b"event: message_start\n"))
            .count();
        assert_eq!(starts, 1);
        for (before, after) in converted.iter().zip(&aliased) {
            if before.starts_with(b"event: message_start\n") {
                assert!(String::from_utf8_lossy(after).contains("\"model\":\"claude-opus-4-6\""));
                let data = before
                    .strip_prefix(b"event: message_start\ndata: ")
                    .unwrap();
                let original: Value = serde_json::from_slice(data).unwrap();
                assert_eq!(original["message"]["model"], "gpt-5.4");
            } else {
                assert_eq!(before, after);
            }
        }
        let merged = aliased
            .iter()
            .map(|chunk| String::from_utf8_lossy(chunk))
            .collect::<String>();
        assert!(merged.contains("signature_delta"));
        assert!(merged.contains("cache_read_input_tokens\":100"));
        assert!(merged.contains("output_tokens\":7"));
    }

    #[tokio::test]
    async fn stream_errors_and_disabled_alias_are_passed_through() {
        let bytes = Bytes::from_static(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"gpt-5.4\"}}\n\n");
        let disabled = alias_stream(
            futures::stream::iter([Ok::<_, &'static str>(bytes.clone())]),
            None,
        )
        .collect::<Vec<_>>()
        .await;
        assert_eq!(disabled, vec![Ok(bytes)]);
        let errors = alias_stream(
            futures::stream::iter([Err::<Bytes, _>("upstream error")]),
            Some(alias()),
        )
        .collect::<Vec<_>>()
        .await;
        assert_eq!(errors, vec![Err("upstream error")]);
    }
}
