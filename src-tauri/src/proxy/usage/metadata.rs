//! Request settings are observations, not a tariff or proof of server selection.
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageMetadata {
    pub service_tier: Option<String>,
    pub service_tier_source: Option<String>,
    pub reasoning_effort: Option<String>,
}

fn known(value: Option<&Value>, choices: &[&str]) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|v| choices.contains(v))
        .map(str::to_owned)
}

impl UsageMetadata {
    pub fn from_request(value: &Value) -> Self {
        let service_tier = match value.get("speed").and_then(Value::as_str) {
            Some("fast") => Some("fast".to_string()),
            Some("standard") => Some("default".to_string()),
            _ => None,
        }
        .or_else(|| {
            known(
                value.get("service_tier"),
                &[
                    "auto",
                    "default",
                    "flex",
                    "priority",
                    "fast",
                    "ultrafast",
                    "scale",
                ],
            )
        });
        Self {
            service_tier_source: service_tier.as_ref().map(|_| "request".to_string()),
            service_tier,
            reasoning_effort: known(
                value
                    .pointer("/reasoning/effort")
                    .or_else(|| value.pointer("/output_config/effort"))
                    .or_else(|| value.get("effort"))
                    .or_else(|| value.get("reasoning_effort"))
                    .or_else(|| value.pointer("/collaboration_mode/settings/reasoning_effort")),
                &[
                    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
                ],
            ),
        }
    }

    pub fn with_stream_events(self, events: &[Value]) -> Self {
        events
            .iter()
            .fold(self, |metadata, event| metadata.with_response(event))
    }

    pub fn with_response(mut self, value: &Value) -> Self {
        let response = value
            .get("response")
            .or_else(|| value.get("message"))
            .unwrap_or(value);
        let speed = response.pointer("/usage/speed").and_then(Value::as_str);
        let tier = match speed {
            Some("fast") => Some("fast".to_string()),
            Some("standard") => Some("default".to_string()),
            _ => Self::from_request(response).service_tier,
        };
        if let Some(tier) = tier {
            self.service_tier = Some(tier);
            self.service_tier_source = Some("response".to_string());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_prefers_response_tier_without_changing_requested_effort() {
        let request = UsageMetadata::from_request(&json!({
            "service_tier": "priority", "reasoning": {"effort": "xhigh"}
        }));
        assert_eq!(request.service_tier_source.as_deref(), Some("request"));
        let actual = request.with_stream_events(&[
            json!({"type": "response.created", "response": {"service_tier": "priority"}}),
            json!({"type": "response.completed", "response": {"service_tier": "default"}}),
            json!({"usage": {"input_tokens": 10}}),
        ]);
        assert_eq!(actual.service_tier.as_deref(), Some("default"));
        assert_eq!(actual.service_tier_source.as_deref(), Some("response"));
        assert_eq!(actual.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn metadata_accepts_explicit_effort_and_keeps_missing_or_invalid_unknown() {
        for value in [
            json!({}),
            json!({"service_tier": null}),
            json!({"service_tier": "invalid", "effort": 100}),
        ] {
            assert_eq!(
                UsageMetadata::from_request(&value),
                UsageMetadata::default()
            );
        }
        for value in [
            json!({"effort": "max"}),
            json!({"reasoning_effort": "max"}),
            json!({"output_config": {"effort": "max"}}),
            json!({"collaboration_mode": {"settings": {"reasoning_effort": "max"}}}),
        ] {
            assert_eq!(
                UsageMetadata::from_request(&value)
                    .reasoning_effort
                    .as_deref(),
                Some("max")
            );
        }
        assert_eq!(
            UsageMetadata::from_request(&json!({"effort": null, "reasoning_effort": "high"}))
                .reasoning_effort,
            None
        );
    }

    #[test]
    fn claude_speed_reports_fast_and_standard_fallback() {
        let requested = UsageMetadata::from_request(&json!({"speed":"fast"}));
        assert_eq!(requested.service_tier.as_deref(), Some("fast"));
        let actual = requested.with_stream_events(&[
            json!({"type":"message_start","message":{"usage":{"speed":"fast"}}}),
            json!({"type":"message_delta","usage":{"speed":"standard"}}),
        ]);
        assert_eq!(actual.service_tier.as_deref(), Some("default"));
        assert_eq!(actual.service_tier_source.as_deref(), Some("response"));
    }
}
