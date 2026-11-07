//! Stable JSON helpers for cache-sensitive request bodies.

use serde::{ser::SerializeMap, ser::SerializeSeq, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn canonicalize_value(mut value: Value) -> Value {
    value.sort_all_objects();
    value
}

/// Serialize sorted keys directly into the output, without a string per JSON node.
struct CanonicalValue<'a>(&'a Value);

impl Serialize for CanonicalValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(&CanonicalValue(value))?;
                }
                seq.end()
            }
            Value::Object(map) => {
                let mut output = serializer.serialize_map(Some(map.len()))?;
                if map.keys().is_sorted() {
                    for (key, value) in map {
                        output.serialize_entry(key, &CanonicalValue(value))?;
                    }
                } else {
                    let mut entries = map.iter().collect::<Vec<_>>();
                    entries.sort_unstable_by_key(|(key, _)| *key);
                    for (key, value) in entries {
                        output.serialize_entry(key, &CanonicalValue(value))?;
                    }
                }
                output.end()
            }
            value => value.serialize(serializer),
        }
    }
}

pub(crate) fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(&CanonicalValue(value))
        .expect("serializing a JSON value for canonical output should not fail")
}

pub(crate) fn canonicalize_json_string_if_parseable(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value.to_string();
    }

    serde_json::from_str::<Value>(trimmed)
        .map(|parsed| canonical_json_string(&parsed))
        .unwrap_or_else(|_| value.to_string())
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
    let mut writer = HashWriter(Sha256::new());
    {
        use std::io::Write;

        // Coalesce the serializer's small writes without keeping a full JSON copy.
        let mut buffer = std::io::BufWriter::new(&mut writer);
        serde_json::to_writer(&mut buffer, &CanonicalValue(value))
            .expect("hashing a JSON value for canonical output should not fail");
        buffer.flush().expect("writing to a hash cannot fail");
    }
    short_digest_hex(&writer.0.finalize())
}

pub(crate) fn short_sha256_hex(bytes: &[u8]) -> String {
    short_digest_hex(&Sha256::digest(bytes))
}

struct HashWriter(Sha256);

impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn short_digest_hex(digest: &[u8]) -> String {
    use std::fmt::Write;

    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
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
    fn canonical_serialization_preserves_wire_bytes_and_hashes() {
        let value = json!({
            "z": [null, true, false, -7, 1.25, "\u{0000}\n\"\\中"],
            "a": {"z": [], "a": {}}
        });
        let expected = r#"{"a":{"a":{},"z":[]},"z":[null,true,false,-7,1.25,"\u0000\n\"\\中"]}"#;

        assert_eq!(canonical_json_string(&value), expected);
        assert_eq!(
            short_value_hash(Some(&value)),
            short_sha256_hex(expected.as_bytes())
        );
        assert_eq!(short_value_hash(None), "absent");
    }

    #[test]
    fn canonical_hash_handles_values_larger_than_the_write_buffer() {
        let value = json!({"z": "中\n\"\\".repeat(4096), "a": 1});
        let canonical = canonical_json_string(&value);

        assert_eq!(
            short_value_hash(Some(&value)),
            short_sha256_hex(canonical.as_bytes())
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
}
