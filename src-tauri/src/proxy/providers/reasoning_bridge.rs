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
    item.get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("summary_text" | "reasoning_text")
            )
            .then(|| part.get("text").and_then(Value::as_str))
            .flatten()
        })
        .collect::<Vec<_>>()
        .join("")
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
            return Some(json!({
                "type": "redacted_thinking",
                "data": envelope
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
    fn encrypted_item_without_summary_uses_redacted_thinking() {
        let item = json!({
            "id": "rs_2",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "opaque"
        });
        let block = anthropic_block_from_openai_reasoning_item(&item).unwrap();
        assert_eq!(block["type"], "redacted_thinking");
        assert_eq!(
            openai_reasoning_item_from_anthropic_block(&block),
            Some(item)
        );
    }
}
