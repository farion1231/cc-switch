//! Chat Completions message `content` field sanitization for OpenAI-compatible
//! upstream APIs.
//!
//! Some third-party OpenAI-compatible gateways (e.g. reverse-proxy aggregators)
//! enforce strict serde validation: every message's `content` must be either a
//! `String` or an `Array` of content parts. Codex's Responses→Chat conversion
//! (`transform_codex_chat`) can produce messages where `content` is a bare
//! JSON `Object`, `Number`, `Boolean`, or `null` — these are valid in
//! Responses but rejected by strict Chat Completions parsers with
//! "messages[N] 的 content 类型不合法，应为字符串或数组" (HTTP 400).
//!
//! This module normalizes every message's `content` field so it is always a
//! `String` or `Array` before the body leaves the proxy. It is gated on the
//! provider's `meta.chatContentSanitize` flag so it never touches providers
//! that already handle these edge cases natively.
//!
//! Run this *after* `responses_to_chat_completions_with_reasoning` — it is a
//! post-transform normalization pass, not a format conversion.

use serde_json::Value;

/// Walk every message in the Chat Completions `messages` array and ensure
/// each message's `content` field is a `String` or `Array`.
///
/// Returns `true` if any field was modified (idempotent: a second pass is
/// always a no-op).
pub(crate) fn sanitize_chat_message_content(body: &mut Value) -> bool {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return false;
    };

    let mut changed = false;
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            if let Some(content) = obj.get_mut("content") {
                changed |= normalize_message_content(content);
            }
        }
    }
    changed
}

/// Replace non-string, non-array `content` values with a deterministic string
/// representation.
///
/// - `String`, `Array`: already valid → no change.
/// - `null` → `""` (empty string). A missing `content` is the same as
///   `content: null` for most APIs, and an empty string is the safest
///   default.
/// - `Object` → JSON-serialized string. This preserves information while
///   making the strict parser happy.
/// - `Number`, `Boolean` → `to_string()`. Rare in practice; handled for
///   completeness.
fn normalize_message_content(content: &mut Value) -> bool {
    match content {
        Value::String(_) | Value::Array(_) => false,
        Value::Null => {
            *content = Value::String(String::new());
            true
        }
        other => {
            let text = serde_json::to_string(other).unwrap_or_default();
            *content = Value::String(text);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sanitize(mut body: Value) -> (Value, bool) {
        let changed = sanitize_chat_message_content(&mut body);
        (body, changed)
    }

    #[test]
    fn string_content_is_unchanged() {
        let body = json!({"messages": [{"role": "user", "content": "hello"}]});
        let (result, changed) = sanitize(body);
        assert!(!changed);
        assert_eq!(result["messages"][0]["content"], "hello");
    }

    #[test]
    fn array_content_is_unchanged() {
        let body = json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
        let (result, changed) = sanitize(body);
        assert!(!changed);
        assert_eq!(result["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn null_content_becomes_empty_string() {
        let body = json!({"messages": [{"role": "assistant", "content": null}]});
        let (result, changed) = sanitize(body);
        assert!(changed);
        assert_eq!(result["messages"][0]["content"], "");
    }

    #[test]
    fn object_content_is_json_serialized() {
        let body = json!({
            "messages": [{
                "role": "tool",
                "content": {"result": "ok", "count": 42}
            }]
        });
        let (result, changed) = sanitize(body);
        assert!(changed);
        let parsed: serde_json::Value =
            serde_json::from_str(result["messages"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["result"], "ok");
        assert_eq!(parsed["count"], 42);
    }

    #[test]
    fn number_content_becomes_string() {
        let body = json!({"messages": [{"role": "assistant", "content": 42}]});
        let (result, changed) = sanitize(body);
        assert!(changed);
        assert_eq!(result["messages"][0]["content"], "42");
    }

    #[test]
    fn second_pass_is_noop() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "tool", "content": {"x": 1}},
                {"role": "assistant", "content": null}
            ]
        });
        assert!(sanitize_chat_message_content(&mut body));
        assert!(!sanitize_chat_message_content(&mut body));
    }
}
