//! Read explicit reasoning settings without guessing model defaults or mapping budgets to levels.

use serde_json::Value;

pub(crate) fn reasoning_effort(body: &Value) -> Option<String> {
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
    .map(str::to_owned)
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
            (json!({"output_config": {"effort": "max"}}), "max"),
            (
                json!({"generationConfig": {"thinkingConfig": {"thinkingLevel": "HIGH"}}}),
                "HIGH",
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
