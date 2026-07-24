//! Helpers for Responses API message item IDs.
//!
//! A Responses `message` item ID, when present, must use the `msg_` prefix.
//! Converted Chat/Anthropic responses are later replayed by Codex as input, so
//! generating response-scoped IDs such as `resp_*_msg` makes old tasks fail
//! against strict Responses upstreams.

use serde_json::Value;
use std::fmt::Display;

pub(crate) fn response_message_item_id(response_id: &str) -> String {
    let suffix = response_id.strip_prefix("resp_").unwrap_or(response_id);
    format!("msg_{suffix}")
}

pub(crate) fn indexed_response_message_item_id(
    response_id: &str,
    output_index: impl Display,
) -> String {
    format!("{}_{output_index}", response_message_item_id(response_id))
}

/// Remove invalid IDs from replayed message inputs before native Responses
/// passthrough. The ID is optional for an input message, while forwarding a
/// non-`msg_` value is rejected by strict upstreams.
pub(crate) fn sanitize_invalid_message_item_ids(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };

    let mut removed = 0;
    for item in input.iter_mut().filter_map(Value::as_object_mut) {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        let invalid = item
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.starts_with("msg_"));
        if invalid {
            item.remove("id");
            removed += 1;
        }
    }

    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn generated_message_ids_use_the_responses_prefix() {
        assert_eq!(
            response_message_item_id("resp_chatcmpl_1"),
            "msg_chatcmpl_1"
        );
        assert_eq!(
            indexed_response_message_item_id("resp_anthropic_1", 2),
            "msg_anthropic_1_2"
        );
    }

    #[test]
    fn sanitizer_removes_only_invalid_message_ids() {
        let mut body = json!({
            "input": [
                {"type": "message", "id": "resp_abc_msg", "role": "assistant", "content": []},
                {"type": "message", "id": "legacy-uuid", "role": "assistant", "content": []},
                {"type": "message", "id": "msg_valid", "role": "assistant", "content": []},
                {"type": "message", "role": "user", "content": []},
                {"type": "function_call", "id": "resp_abc_msg", "call_id": "call_1"}
            ]
        });

        assert_eq!(sanitize_invalid_message_item_ids(&mut body), 2);
        assert!(body["input"][0].get("id").is_none());
        assert!(body["input"][1].get("id").is_none());
        assert_eq!(body["input"][2]["id"], "msg_valid");
        assert!(body["input"][3].get("id").is_none());
        assert_eq!(body["input"][4]["id"], "resp_abc_msg");
    }

    #[test]
    fn sanitizer_ignores_non_array_input() {
        let mut body = json!({"input": "hello"});
        assert_eq!(sanitize_invalid_message_item_ids(&mut body), 0);
        assert_eq!(body["input"], "hello");
    }
}
