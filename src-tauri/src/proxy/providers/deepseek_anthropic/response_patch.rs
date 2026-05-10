use serde_json::{json, Value};

pub fn patch_non_streaming_response(
    body: &mut Value,
    fake_model: &str,
    effective_thinking_enabled: bool,
) {
    let Some(obj) = body.as_object_mut() else { return; };

    if obj.contains_key("model") {
        obj.insert("model".into(), Value::String(fake_model.into()));
    }

    if let Some(content) = obj.get_mut("content").and_then(|v| v.as_array_mut()) {
        content.retain(|block| {
            let Some(t) = block.get("type").and_then(|s| s.as_str()) else {
                return true;
            };
            if t.starts_with("redacted_thinking") {
                return false;
            }
            if t == "thinking" && !effective_thinking_enabled {
                return false;
            }
            true
        });
        if content.is_empty() {
            content.push(json!({"type": "text", "text": "(empty)"}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_model_field_replaced() {
        let mut body = json!({"model": "deepseek-v4-pro", "content": []});
        patch_non_streaming_response(&mut body, "claude-opus-4-7", true);
        assert_eq!(body["model"], "claude-opus-4-7");
    }

    #[test]
    fn test_redacted_thinking_always_dropped() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "content": [
                {"type": "redacted_thinking", "data": "enc"},
                {"type": "text", "text": "hi"}
            ]
        });
        patch_non_streaming_response(&mut body, "claude-opus-4-7", true);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_thinking_dropped_when_disabled() {
        let mut body = json!({
            "model": "m",
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "answer"}
            ]
        });
        patch_non_streaming_response(&mut body, "fake", false);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_thinking_kept_when_enabled() {
        let mut body = json!({
            "model": "m",
            "content": [
                {"type": "thinking", "thinking": "chain"},
                {"type": "text", "text": "answer"}
            ]
        });
        patch_non_streaming_response(&mut body, "fake", true);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "thinking");
    }

    #[test]
    fn test_empty_content_fallback() {
        let mut body = json!({
            "model": "m",
            "content": [{"type": "thinking", "thinking": "chain"}]
        });
        patch_non_streaming_response(&mut body, "fake", false);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "(empty)");
    }

    #[test]
    fn test_no_model_field_no_panic() {
        let mut body = json!({"content": [{"type": "text", "text": "hi"}]});
        patch_non_streaming_response(&mut body, "fake", true);
        assert!(body.get("model").is_none());
    }

    #[test]
    fn test_non_object_body_no_panic() {
        let mut body = json!("not an object");
        patch_non_streaming_response(&mut body, "fake", true);
        // should not panic
    }
}
