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

fn normalize_legacy_message_item_id(id: &str) -> Option<String> {
    let suffix = id.strip_prefix("resp_")?;

    if let Some(response_suffix) = suffix.strip_suffix("_msg") {
        return (!response_suffix.is_empty()).then(|| format!("msg_{response_suffix}"));
    }

    let (response_suffix, output_index) = suffix.rsplit_once("_msg_")?;
    if response_suffix.is_empty()
        || output_index.is_empty()
        || !output_index.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    Some(format!("msg_{response_suffix}_{output_index}"))
}

/// Sanitize invalid IDs on replayed message inputs before native Responses
/// passthrough. IDs produced by older CC Switch conversions are rewritten to
/// the current `msg_` form so reasoning/message pairs remain intact. Unknown
/// invalid IDs are removed because an ID is optional for an input message.
pub(crate) fn sanitize_invalid_message_item_ids(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };

    let mut sanitized = 0;
    for item in input.iter_mut().filter_map(Value::as_object_mut) {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.starts_with("msg_") {
            continue;
        }

        if let Some(normalized) = normalize_legacy_message_item_id(id) {
            item.insert("id".to_string(), Value::String(normalized));
        } else {
            item.remove("id");
        }
        sanitized += 1;
    }

    sanitized
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
    fn sanitizer_preserves_reasoning_message_pairs_by_rewriting_legacy_ids() {
        let mut body = json!({
            "input": [
                {"type": "reasoning", "id": "rs_resp_chatcmpl_1", "summary": []},
                {"type": "message", "id": "resp_chatcmpl_1_msg", "role": "assistant", "content": []},
                {"type": "reasoning", "id": "rs_resp_anthropic_1_2", "summary": []},
                {"type": "message", "id": "resp_anthropic_1_msg_2", "role": "assistant", "content": []},
                {"type": "reasoning", "id": "rs_valid", "summary": []},
                {"type": "message", "id": "msg_valid", "role": "assistant", "content": []}
            ]
        });

        assert_eq!(sanitize_invalid_message_item_ids(&mut body), 2);
        assert_eq!(body["input"].as_array().unwrap().len(), 6);
        assert_eq!(body["input"][0]["id"], "rs_resp_chatcmpl_1");
        assert_eq!(body["input"][1]["id"], "msg_chatcmpl_1");
        assert_eq!(body["input"][2]["id"], "rs_resp_anthropic_1_2");
        assert_eq!(body["input"][3]["id"], "msg_anthropic_1_2");
        assert_eq!(body["input"][4]["id"], "rs_valid");
        assert_eq!(body["input"][5]["id"], "msg_valid");
    }

    #[test]
    fn sanitizer_removes_only_unknown_invalid_message_ids() {
        let mut body = json!({
            "input": [
                {"type": "message", "id": "legacy-uuid", "role": "assistant", "content": []},
                {"type": "message", "id": "resp_abc_msg_index", "role": "assistant", "content": []},
                {"type": "message", "role": "user", "content": []},
                {"type": "function_call", "id": "resp_abc_msg", "call_id": "call_1"}
            ]
        });

        assert_eq!(sanitize_invalid_message_item_ids(&mut body), 2);
        assert!(body["input"][0].get("id").is_none());
        assert!(body["input"][1].get("id").is_none());
        assert!(body["input"][2].get("id").is_none());
        assert_eq!(body["input"][3]["id"], "resp_abc_msg");
    }

    #[test]
    fn sanitizer_ignores_non_array_input() {
        let mut body = json!({"input": "hello"});
        assert_eq!(sanitize_invalid_message_item_ids(&mut body), 0);
        assert_eq!(body["input"], "hello");
    }
}
