//! Read explicit reasoning settings while respecting Claude's thinking switch.

use serde_json::Value;

pub(crate) fn reasoning_effort(body: &Value) -> Option<String> {
    match body.pointer("/thinking/type").and_then(Value::as_str) {
        Some("disabled") => return Some("none".into()),
        // Claude can send an effort preference even when thinking is off.
        None if body.get("output_config").is_some()
            && !body
                .get("model")
                .and_then(Value::as_str)
                .is_some_and(crate::proxy::thinking_optimizer::adaptive_thinking_is_default) =>
        {
            return Some("none".into());
        }
        _ => {}
    }
    [
        "/reasoning/effort",
        "/reasoning_effort",
        "/output_config/effort",
        "/generationConfig/thinkingConfig/thinkingLevel",
        "/generation_config/thinking_config/thinking_level",
    ]
    .iter()
    .filter_map(|path| body.pointer(path).and_then(Value::as_str))
    .map(str::trim)
    .find(|effort| !effort.is_empty())
    .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_explicit_reasoning_effort_across_request_formats() {
        for (body, expected) in [
            (json!({"reasoning": {"effort": "xhigh"}}), "xhigh"),
            (json!({"reasoning_effort": "none"}), "none"),
            (
                json!({"thinking": {"type": "adaptive"}, "output_config": {"effort": "max"}}),
                "max",
            ),
            (
                json!({"generationConfig": {"thinkingConfig": {"thinkingLevel": "HIGH"}}}),
                "high",
            ),
            (
                json!({"generation_config": {"thinking_config": {"thinking_level": "low"}}}),
                "low",
            ),
            (
                json!({"reasoning": {"effort": " "}, "reasoning_effort": " high "}),
                "high",
            ),
        ] {
            assert_eq!(reasoning_effort(&body).as_deref(), Some(expected));
        }
    }

    #[test]
    fn respects_claude_thinking_state_before_reading_effort() {
        for (body, expected) in [
            (
                json!({"thinking": {"type": "disabled"}, "output_config": {"effort": "high"}}),
                Some("none"),
            ),
            (json!({"output_config": {"effort": "medium"}}), Some("none")),
            (
                json!({"model": "claude-opus-4-6", "output_config": {"effort": "high"}}),
                Some("none"),
            ),
            (
                json!({"model": "claude-sonnet-5", "output_config": {"effort": "high"}}),
                Some("high"),
            ),
            (
                json!({"model": "claude-sonnet-5", "thinking": {"type": "disabled"}, "output_config": {"effort": "high"}}),
                Some("none"),
            ),
            (
                json!({"thinking": {"type": "enabled", "budget_tokens": 8192}, "output_config": {"effort": "medium"}}),
                Some("medium"),
            ),
            (
                json!({"thinking": {"type": "adaptive"}, "output_config": {"effort": "max"}}),
                Some("max"),
            ),
            (json!({"thinking": {"type": "disabled"}}), Some("none")),
            (json!({"model": "claude-sonnet-5"}), None),
        ] {
            assert_eq!(reasoning_effort(&body).as_deref(), expected, "{body}");
        }
    }

    #[test]
    fn does_not_infer_reasoning_effort_from_model_or_token_budget() {
        for body in [
            json!({"model": "gpt-5"}),
            json!({"thinking": {"type": "enabled", "budget_tokens": 8192}}),
            json!({"reasoning": {"effort": null}}),
            json!({"reasoning_effort": 123}),
        ] {
            assert_eq!(reasoning_effort(&body), None);
        }
    }
}
