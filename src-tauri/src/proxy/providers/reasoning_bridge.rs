//! Opaque reasoning transport helpers shared by the Messages ↔ Responses bridge.
//!
//! The Anthropic Messages protocol has no field for an OpenAI Responses
//! `reasoning` item. To keep stateless tool loops lossless, the complete item is
//! carried in a versioned thinking signature/redacted-thinking payload and
//! restored when the client replays the assistant message.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

pub(crate) const OPENAI_REASONING_ITEM_PREFIX: &str = "ccswitch-openai-reasoning-v1:";

pub(crate) fn reasoning_summary_text(item: &Value) -> String {
    if let Some(text) = reasoning_parts_text(item.get("summary")) {
        if !text.is_empty() {
            return text;
        }
    }

    // Responses-compatible gateways disagree about where visible reasoning lives:
    // the official shape uses summary[], while some gateways expose reasoning_text
    // in content[]. Prefer summary when both are present so mirrored payloads do
    // not make the same thought appear twice.
    reasoning_parts_text(item.get("content")).unwrap_or_default()
}

fn reasoning_parts_text(parts: Option<&Value>) -> Option<String> {
    let parts = parts?;
    match parts {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    Value::String(value) if !value.is_empty() => Some(value.as_str()),
                    Value::Object(object) => object
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some(text.join("\n"))
        }
        _ => None,
    }
}

pub(crate) fn encode_openai_reasoning_item(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return None;
    }
    let bytes = serde_json::to_vec(item).ok()?;
    Some(format!(
        "{OPENAI_REASONING_ITEM_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

pub(crate) fn decode_openai_reasoning_item(encoded: &str) -> Option<Value> {
    let payload = encoded.strip_prefix(OPENAI_REASONING_ITEM_PREFIX)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let item: Value = serde_json::from_slice(&bytes).ok()?;
    (item.get("type").and_then(Value::as_str) == Some("reasoning")).then_some(item)
}

pub(crate) fn anthropic_block_from_openai_reasoning_item(item: &Value) -> Option<Value> {
    anthropic_block_from_openai_reasoning_item_for_client(item)
}

/// Convert an opaque Responses reasoning item into a replayable Anthropic thinking block.
pub(crate) fn anthropic_block_from_openai_reasoning_item_for_client(
    item: &Value,
) -> Option<Value> {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return None;
    }

    let text = reasoning_summary_text(item);
    let has_encrypted_content = item
        .get("encrypted_content")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());

    if has_encrypted_content {
        let envelope = encode_openai_reasoning_item(item)?;
        if text.is_empty() {
            // Responses reasoning may be encrypted without a visible summary. Keep
            // the replay envelope in a valid thinking block instead of exposing the
            // unsupported redacted_thinking content type or a fake placeholder.
            return Some(json!({
                "type": "thinking",
                "thinking": "",
                "signature": envelope
            }));
        }
        return Some(json!({
            "type": "thinking",
            "thinking": text,
            "signature": envelope
        }));
    }

    (!text.is_empty()).then(|| {
        json!({
            "type": "thinking",
            "thinking": text
        })
    })
}

pub(crate) fn openai_reasoning_item_from_anthropic_block(block: &Value) -> Option<Value> {
    match block.get("type").and_then(Value::as_str) {
        Some("thinking") => block
            .get("signature")
            .and_then(Value::as_str)
            .and_then(decode_openai_reasoning_item),
        Some("redacted_thinking") => block
            .get("data")
            .and_then(Value::as_str)
            .and_then(decode_openai_reasoning_item),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_reasoning_item_round_trips_through_thinking_signature() {
        let item = json!({
            "id": "rs_1",
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "Need a tool."}],
            "encrypted_content": "opaque"
        });
        let block = anthropic_block_from_openai_reasoning_item(&item).unwrap();
        assert_eq!(block["type"], "thinking");
        assert_eq!(
            openai_reasoning_item_from_anthropic_block(&block),
            Some(item)
        );
    }

    #[test]
    fn encrypted_item_without_summary_uses_empty_thinking_signature() {
        let item = json!({
            "id": "rs_2",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "opaque"
        });
        let block = anthropic_block_from_openai_reasoning_item(&item).unwrap();
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "");
        assert!(block.get("data").is_none());
        assert_eq!(
            openai_reasoning_item_from_anthropic_block(&block),
            Some(item)
        );
    }

    #[test]
    fn encrypted_item_without_summary_uses_thinking_for_compatible_clients() {
        let item = json!({
            "id": "rs_compat",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "opaque"
        });
        let block = anthropic_block_from_openai_reasoning_item_for_client(&item).unwrap();
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "");
        assert_eq!(
            openai_reasoning_item_from_anthropic_block(&block),
            Some(item)
        );
    }

    #[test]
    fn summary_text_falls_back_to_reasoning_content() {
        let item = json!({
            "type": "reasoning",
            "summary": [],
            "content": [
                {"type": "reasoning_text", "text": "Restored from content."}
            ]
        });

        assert_eq!(reasoning_summary_text(&item), "Restored from content.");
    }

    #[test]
    fn summary_text_prefers_summary_when_content_mirrors_it() {
        let item = json!({
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "Visible summary."}
            ],
            "content": [
                {"type": "reasoning_text", "text": "Visible summary."}
            ]
        });

        assert_eq!(reasoning_summary_text(&item), "Visible summary.");
    }

    #[test]
    fn summary_text_accepts_string_parts_and_unknown_part_types() {
        let item = json!({
            "type": "reasoning",
            "summary": [
                "First ",
                {"type": "future_summary_part", "text": "second"}
            ]
        });

        assert_eq!(reasoning_summary_text(&item), "First \nsecond");
    }
}
