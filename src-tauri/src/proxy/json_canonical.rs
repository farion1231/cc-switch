//! Stable JSON helpers for cache-sensitive request bodies.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize_value(value));
            }
            Value::Object(sorted)
        }
        other => other,
    }
}

pub(crate) fn canonical_json_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value)
            .expect("serializing a JSON string for canonical output should not fail"),
        Value::Array(values) => {
            let parts = values.iter().map(canonical_json_string).collect::<Vec<_>>();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let parts = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).expect(
                        "serializing a JSON object key for canonical output should not fail",
                    );
                    format!("{key}:{}", canonical_json_string(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(","))
        }
    }
}

pub(crate) fn canonicalize_json_string_if_parseable(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value.to_string();
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return canonical_json_string(&parsed);
    }

    // Third-party tool-call arguments sometimes contain invalid `\x` escapes
    // inside string values (e.g. GLM emits `\'`, which JSON does not permit).
    // Passing such text through verbatim poisons the session: the arguments
    // land in the client's history and 400 the next request when the strict
    // request path tries to parse them. As a best-effort repair, double every
    // backslash that does not start a valid escape so it becomes a literal
    // backslash; structural JSON outside string values is left untouched.
    if let Ok(parsed) = serde_json::from_str::<Value>(&repair_invalid_escapes(trimmed)) {
        return canonical_json_string(&parsed);
    }

    value.to_string()
}

/// Best-effort repair of invalid `\x` escape sequences inside JSON string
/// values. Walks the text with a string-state machine and doubles every
/// backslash that is not followed by a valid escape character (one of
/// `\" \\ \/ \b \f \n \r \t \uXXXX`), turning the likes of `\'` or `\d` into a
/// literal backslash plus the following character. Bytes outside string
/// values (structural JSON) are copied unchanged.
fn repair_invalid_escapes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 8);
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if !in_string {
            out.push(b);
            in_string = b == b'"';
            i += 1;
        } else if b == b'\\' {
            let valid_next = i + 1 < bytes.len()
                && matches!(bytes[i + 1], b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u');
            if valid_next {
                out.push(b);
                out.push(bytes[i + 1]);
                i += 2;
            } else {
                // Illegal escape: double the backslash so it parses as a literal '\'.
                out.push(b'\\');
                out.push(b'\\');
                i += 1;
            }
        } else if b == b'"' {
            out.push(b);
            in_string = false;
            i += 1;
        } else {
            // UTF-8 lead/continuation bytes inside a string — copy verbatim.
            out.push(b);
            i += 1;
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Normalize a tool-call `arguments` string into a valid JSON payload.
///
/// Identical to [`canonicalize_json_string_if_parseable`] except that an empty
/// (or whitespace-only) value is coerced to `"{}"` instead of being passed
/// through verbatim. A no-argument tool call must serialize as `"{}"`; strict
/// upstreams such as Minimax reject `arguments: ""` with a 400
/// `invalid function arguments json string` error, whereas lenient ones
/// (OpenAI, Kimi) silently treat it as an empty object.
pub(crate) fn canonicalize_tool_arguments_str(value: &str) -> String {
    if value.trim().is_empty() {
        return "{}".to_string();
    }
    canonicalize_json_string_if_parseable(value)
}

/// Normalize a tool-call `arguments` field from a Responses/Chat item.
///
/// Mirrors the inline `match` that several transform paths used to duplicate:
/// a string is canonicalized (with empty coerced to `"{}"`), a structured
/// value is serialized canonically, and a missing field defaults to `"{}"`.
pub(crate) fn canonicalize_tool_arguments(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => canonicalize_tool_arguments_str(s),
        Some(v) => canonical_json_string(v),
        None => "{}".to_string(),
    }
}

pub(crate) fn short_value_hash(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return "absent".to_string();
    };
    short_sha256_hex(canonical_json_string(value).as_bytes())
}

pub(crate) fn short_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_string_sorts_nested_object_keys() {
        let left = json!({
            "b": 2,
            "a": {
                "d": true,
                "c": [3, {"z": 1, "y": 2}]
            }
        });
        let right = json!({
            "a": {
                "c": [3, {"y": 2, "z": 1}],
                "d": true
            },
            "b": 2
        });

        assert_eq!(canonical_json_string(&left), canonical_json_string(&right));
        assert_eq!(
            short_value_hash(Some(&left)),
            short_value_hash(Some(&right))
        );
    }

    #[test]
    fn canonicalize_value_sorts_map_storage_order() {
        let value = canonicalize_value(json!({"b": 2, "a": 1}));

        assert_eq!(serde_json::to_string(&value).unwrap(), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn canonicalize_json_string_if_parseable_sorts_keys_and_removes_whitespace() {
        assert_eq!(
            canonicalize_json_string_if_parseable(r#"{ "b": 2, "a": 1 }"#),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn canonicalize_json_string_if_parseable_preserves_plain_text() {
        assert_eq!(
            canonicalize_json_string_if_parseable("plain text"),
            "plain text"
        );
    }

    #[test]
    fn canonicalize_tool_arguments_str_coerces_empty_to_object() {
        assert_eq!(canonicalize_tool_arguments_str(""), "{}");
        assert_eq!(canonicalize_tool_arguments_str("   "), "{}");
        assert_eq!(canonicalize_tool_arguments_str("\n\t"), "{}");
    }

    #[test]
    fn canonicalize_tool_arguments_str_canonicalizes_valid_json() {
        assert_eq!(
            canonicalize_tool_arguments_str(r#"{ "b": 2, "a": 1 }"#),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn canonicalize_tool_arguments_handles_field_variants() {
        // Missing field -> empty object.
        assert_eq!(canonicalize_tool_arguments(None), "{}");
        // Empty string field -> empty object.
        assert_eq!(canonicalize_tool_arguments(Some(&json!(""))), "{}");
        // String field with JSON -> canonicalized.
        assert_eq!(
            canonicalize_tool_arguments(Some(&json!(r#"{"b":2,"a":1}"#))),
            r#"{"a":1,"b":2}"#
        );
        // Structured (non-string) field -> canonical serialization.
        assert_eq!(
            canonicalize_tool_arguments(Some(&json!({"b": 2, "a": 1}))),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn canonicalize_tool_arguments_str_repairs_invalid_escape() {
        // Third-party models (e.g. GLM) sometimes emit string values with
        // invalid `\x` escapes such as `\'`. Canonicalization must yield a
        // parseable document so the poisoned arguments cannot later fail the
        // strict parse on the request path and 400 the whole request.
        let repaired = canonicalize_tool_arguments_str(r#"{"plan":[{"step":"a\'b\'c"}]}"#);
        let parsed: Value = serde_json::from_str(&repaired)
            .expect("repaired arguments must parse as JSON");
        assert_eq!(parsed["plan"][0]["step"], "a\\'b\\'c");
    }

    #[test]
    fn canonicalize_tool_arguments_str_passthrough_when_unrepairable() {
        // A structural error (not an invalid escape) cannot be repaired —
        // pass through verbatim rather than silently dropping content.
        assert_eq!(canonicalize_tool_arguments_str(r#"{"plan":[}"#), r#"{"plan":[}"#);
    }
}
