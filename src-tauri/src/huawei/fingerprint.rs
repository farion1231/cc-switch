use serde_json::Value;

pub fn apply_fingerprint_replacements(body: &mut Value, replacements: &[(String, String)]) {
    if replacements.is_empty() {
        return;
    }
    replace_strings_in_value(body, replacements);
}

#[cfg(test)]
pub fn apply_fingerprint_replacements_text(
    text: &str,
    replacements: &[(String, String)],
) -> String {
    let mut result = text.to_string();
    for (from, to) in replacements {
        result = result.replace(from, to);
    }
    result
}

pub fn strip_billing_header_in_value(value: &mut Value) {
    match value {
        Value::String(s) => {
            *s = s
                .lines()
                .filter(|line| !line.trim_start().starts_with("x-anthropic-billing-header:"))
                .collect::<Vec<&str>>()
                .join("\n");
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_billing_header_in_value(item);
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                strip_billing_header_in_value(val);
            }
        }
        _ => {}
    }
}

fn replace_strings_in_value(value: &mut Value, replacements: &[(String, String)]) {
    match value {
        Value::String(s) => {
            for (from, to) in replacements {
                *s = s.replace(from, to);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                replace_strings_in_value(item, replacements);
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                replace_strings_in_value(val, replacements);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_fingerprint_replacements_basic() {
        let mut body = serde_json::json!({
            "content": [{"type": "text", "text": "Hello from Claude Code"}],
            "model": "claude-3-opus"
        });
        let replacements = vec![
            ("Claude Code".to_string(), "CodeAgent".to_string()),
            ("Claude".to_string(), "CodeAgent".to_string()),
        ];
        apply_fingerprint_replacements(&mut body, &replacements);
        assert_eq!(body["content"][0]["text"], "Hello from CodeAgent");
        assert_eq!(body["model"], "claude-3-opus");
    }

    #[test]
    fn test_apply_fingerprint_replacements_order() {
        let mut body = serde_json::json!({"text": "Claude Code and Claude"});
        let replacements = vec![
            ("Claude Code".to_string(), "CodeAgent".to_string()),
            ("Claude".to_string(), "CodeAgent".to_string()),
        ];
        apply_fingerprint_replacements(&mut body, &replacements);
        assert_eq!(body["text"], "CodeAgent and CodeAgent");
    }

    #[test]
    fn test_apply_fingerprint_replacements_text() {
        let text = "Hello from Claude Code!";
        let replacements = vec![
            ("Claude Code".to_string(), "CodeAgent".to_string()),
            ("Claude".to_string(), "CodeAgent".to_string()),
        ];
        let result = apply_fingerprint_replacements_text(text, &replacements);
        assert_eq!(result, "Hello from CodeAgent!");
    }

    #[test]
    fn test_strip_billing_header() {
        let mut body = serde_json::json!({
            "text": "line1\nx-anthropic-billing-header: some-value\nline3"
        });
        strip_billing_header_in_value(&mut body);
        let text = body["text"].as_str().unwrap();
        assert!(!text.contains("x-anthropic-billing-header"));
        assert!(text.contains("line1"));
        assert!(text.contains("line3"));
    }

    #[test]
    fn test_empty_replacements() {
        let mut body = serde_json::json!({"text": "Hello Claude"});
        apply_fingerprint_replacements(&mut body, &[]);
        assert_eq!(body["text"], "Hello Claude");
    }
}
