//! Compatibility fixes for native Responses API payloads.
//!
//! Native upstreams are normally passed through unchanged. Some compatible
//! gateways omit optional-looking fields that strict clients deserialize as
//! required, though. Keep the fixes here narrow and protocol-preserving.

use serde_json::{json, Value};

/// Add the Responses API's required `annotations` array to output-text content
/// parts when an upstream omits it. Existing values are never overwritten.
pub(crate) fn ensure_output_text_annotations(value: &mut Value) -> bool {
    let mut changed = false;
    match value {
        Value::Array(items) => {
            for item in items {
                changed |= ensure_output_text_annotations(item);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("output_text")
                && !object.contains_key("annotations")
            {
                object.insert("annotations".to_string(), json!([]));
                changed = true;
            }
            for child in object.values_mut() {
                changed |= ensure_output_text_annotations(child);
            }
        }
        _ => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_annotations_to_nested_output_text_parts() {
        let mut response = json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "type": "message",
                    "content": [{"type": "output_text", "text": "hello"}]
                }]
            }
        });

        assert!(ensure_output_text_annotations(&mut response));
        assert_eq!(
            response["response"]["output"][0]["content"][0]["annotations"],
            json!([])
        );
    }

    #[test]
    fn preserves_existing_annotations_and_ignores_delta_events() {
        let citations = json!([{"type": "url_citation", "url": "https://example.com"}]);
        let mut value = json!({
            "parts": [
                {"type": "output_text", "text": "cited", "annotations": citations},
                {"type": "response.output_text.delta", "delta": "x"}
            ]
        });

        assert!(!ensure_output_text_annotations(&mut value));
        assert_eq!(value["parts"][0]["annotations"], citations);
        assert!(value["parts"][1].get("annotations").is_none());
    }
}
