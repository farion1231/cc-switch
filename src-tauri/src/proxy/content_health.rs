use http::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentHealth {
    Healthy,
    Inconclusive,
    Unhealthy(ContentHealthViolation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHealthViolation {
    pub reason: ContentHealthReason,
    pub body_hash: String,
    pub sample_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentHealthReason {
    KnownSignatureHit,
    AdOrPromoText,
    UnrelatedChatRoomOrInvite,
    HtmlLoginOrCaptcha,
    QuotaSalesOrProviderNotice,
    SchemaMismatch,
    MalformedToolCall,
    RepeatedUnrelatedStreamFragment,
    ZeroUsageZeroCostInvalidResponse,
}

impl ContentHealthReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnownSignatureHit => "known_signature_hit",
            Self::AdOrPromoText => "ad_or_promo_text",
            Self::UnrelatedChatRoomOrInvite => "unrelated_chat_room_or_invite",
            Self::HtmlLoginOrCaptcha => "html_login_or_captcha",
            Self::QuotaSalesOrProviderNotice => "quota_sales_or_provider_notice",
            Self::SchemaMismatch => "schema_mismatch",
            Self::MalformedToolCall => "malformed_tool_call",
            Self::RepeatedUnrelatedStreamFragment => "repeated_unrelated_stream_fragment",
            Self::ZeroUsageZeroCostInvalidResponse => "zero_usage_zero_cost_invalid_response",
        }
    }
}

impl ContentHealthViolation {
    pub fn provider_unhealthy_message(&self) -> String {
        format!(
            "content_unhealthy:{} hash={} sample_len={}",
            self.reason.as_str(),
            self.body_hash,
            self.sample_len
        )
    }
}

pub fn classify_non_stream_response(
    headers: &HeaderMap,
    body: &[u8],
    request_is_generation: bool,
) -> ContentHealth {
    let sample = String::from_utf8_lossy(body);
    let lower = sample.to_ascii_lowercase();

    if sample.trim().is_empty() {
        return ContentHealth::Healthy;
    }

    if looks_like_html_or_captcha(headers, &lower) {
        return unhealthy(ContentHealthReason::HtmlLoginOrCaptcha, body);
    }

    if let Some(reason) = synthetic_or_general_unhealthy_signal(&lower) {
        return unhealthy(reason, body);
    }

    if let Some(reason) = malformed_tool_call_reason(&sample) {
        return unhealthy(reason, body);
    }

    if request_is_generation && zero_usage_invalid_response(&sample) {
        return unhealthy(ContentHealthReason::ZeroUsageZeroCostInvalidResponse, body);
    }

    if request_is_generation && expects_json(headers) && !looks_like_valid_protocol_json(&sample) {
        return unhealthy(ContentHealthReason::SchemaMismatch, body);
    }

    ContentHealth::Healthy
}

pub fn classify_sse_prime_window(
    event_data: &[String],
    raw_sample: &[u8],
    request_is_generation: bool,
) -> ContentHealth {
    if event_data.is_empty() && raw_sample.is_empty() {
        return ContentHealth::Inconclusive;
    }

    let joined = event_data.join("\n");
    let lower = joined.to_ascii_lowercase();

    if let Some(reason) = synthetic_or_general_unhealthy_signal(&lower) {
        return unhealthy(reason, raw_sample);
    }

    if repeated_fragment(event_data) {
        return unhealthy(
            ContentHealthReason::RepeatedUnrelatedStreamFragment,
            raw_sample,
        );
    }

    if event_data
        .iter()
        .any(|data| malformed_tool_call_reason(data).is_some())
    {
        return unhealthy(ContentHealthReason::MalformedToolCall, raw_sample);
    }

    if request_is_generation
        && event_data
            .iter()
            .any(|data| zero_usage_invalid_response(data))
    {
        return unhealthy(
            ContentHealthReason::ZeroUsageZeroCostInvalidResponse,
            raw_sample,
        );
    }

    if request_is_generation
        && event_data
            .iter()
            .any(|data| !sse_event_data_has_protocol_shape(data))
    {
        return unhealthy(ContentHealthReason::SchemaMismatch, raw_sample);
    }

    ContentHealth::Healthy
}

fn unhealthy(reason: ContentHealthReason, sample: &[u8]) -> ContentHealth {
    ContentHealth::Unhealthy(ContentHealthViolation {
        reason,
        body_hash: super::json_canonical::short_sha256_hex(sample),
        sample_len: sample.len(),
    })
}

fn looks_like_html_or_captcha(headers: &HeaderMap, lower: &str) -> bool {
    let content_type = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let trimmed = lower.trim_start();

    content_type.contains("html")
        || trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || lower.contains("<body")
        || lower.contains("<form")
        || lower.contains("</html>")
}

fn synthetic_or_general_unhealthy_signal(lower: &str) -> Option<ContentHealthReason> {
    if lower.contains("synthetic_unhealthy_marker") {
        return Some(ContentHealthReason::KnownSignatureHit);
    }
    if lower.contains("synthetic_ad_promo_marker") {
        return Some(ContentHealthReason::AdOrPromoText);
    }
    if lower.contains("provider_notice_unhealthy") {
        return Some(ContentHealthReason::QuotaSalesOrProviderNotice);
    }
    if lower.contains("unrelated_invite_marker") {
        return Some(ContentHealthReason::UnrelatedChatRoomOrInvite);
    }
    None
}

fn expects_json(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("json"))
        .unwrap_or(true)
}

fn looks_like_valid_protocol_json(sample: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(sample) else {
        return false;
    };

    let Some(object) = value.as_object() else {
        return false;
    };

    object.contains_key("content")
        || object.contains_key("output")
        || object.contains_key("choices")
        || object
            .get("message")
            .and_then(|message| message.as_object())
            .and_then(|message| message.get("content"))
            .is_some()
        || valid_tool_calls(object)
}

fn sse_event_data_has_protocol_shape(data: &str) -> bool {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return true;
    }

    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return false;
    };

    let Some(object) = value.as_object() else {
        return false;
    };

    object.contains_key("type")
        || object.contains_key("delta")
        || object.contains_key("choices")
        || object.contains_key("content")
        || object.contains_key("message")
        || object.contains_key("output")
        || object.contains_key("response")
        || object.contains_key("usage")
        || object.contains_key("candidate")
        || object.contains_key("candidates")
        || object.contains_key("error")
        || valid_tool_calls(object)
}

fn malformed_tool_call_reason(sample: &str) -> Option<ContentHealthReason> {
    let lower = sample.to_ascii_lowercase();
    if !lower.contains("tool_call") && !lower.contains("tool call") {
        return None;
    }

    let Ok(value) = serde_json::from_str::<Value>(sample) else {
        return Some(ContentHealthReason::MalformedToolCall);
    };

    let Some(object) = value.as_object() else {
        return Some(ContentHealthReason::MalformedToolCall);
    };

    if valid_tool_calls(object) {
        None
    } else {
        Some(ContentHealthReason::MalformedToolCall)
    }
}

fn valid_tool_calls(object: &serde_json::Map<String, Value>) -> bool {
    let Some(calls) = object.get("tool_calls").and_then(|value| value.as_array()) else {
        return false;
    };
    if calls.is_empty() {
        return false;
    }

    calls.iter().all(|call| {
        let Some(call) = call.as_object() else {
            return false;
        };

        if let Some(function) = call.get("function").and_then(|value| value.as_object()) {
            return function
                .get("name")
                .and_then(|value| value.as_str())
                .is_some_and(|name| !name.is_empty())
                && function
                    .get("arguments")
                    .and_then(|value| value.as_str())
                    .is_some();
        }

        call.get("name")
            .and_then(|value| value.as_str())
            .is_some_and(|name| !name.is_empty())
            && (call.contains_key("arguments") || call.contains_key("input"))
    })
}

fn zero_usage_invalid_response(sample: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(sample) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(usage) = object.get("usage").and_then(|value| value.as_object()) else {
        return false;
    };

    let input_zero = number_is_zero(usage.get("input_tokens").or_else(|| usage.get("input")));
    let output_zero = number_is_zero(usage.get("output_tokens").or_else(|| usage.get("output")));
    let cost_zero = number_is_zero(usage.get("total_cost").or_else(|| usage.get("cost")));

    input_zero && output_zero && cost_zero && !looks_like_valid_protocol_json(sample)
}

fn number_is_zero(value: Option<&Value>) -> bool {
    value
        .and_then(|value| value.as_f64())
        .is_some_and(|number| number.abs() < f64::EPSILON)
}

fn repeated_fragment(event_data: &[String]) -> bool {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for data in event_data {
        let trimmed = data.trim();
        if trimmed.len() < 20 {
            continue;
        }
        let count = counts.entry(trimmed).or_insert(0);
        *count += 1;
        if *count >= 3 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::CONTENT_TYPE;
    use http::{HeaderMap, HeaderValue};

    #[test]
    fn allows_clean_openai_response_json() {
        let body =
            br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#;

        assert_eq!(
            classify_non_stream_response(&HeaderMap::new(), body, true),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn allows_clean_anthropic_message_json() {
        let body = br#"{"content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":1,"output_tokens":1}}"#;

        assert_eq!(
            classify_non_stream_response(&HeaderMap::new(), body, true),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn blocks_synthetic_unhealthy_non_stream_json_with_redacted_message() {
        let body = b"SYNTHETIC_UNHEALTHY_MARKER should not be copied";
        let ContentHealth::Unhealthy(violation) =
            classify_non_stream_response(&HeaderMap::new(), body, true)
        else {
            panic!("expected unhealthy");
        };

        let message = violation.provider_unhealthy_message();
        assert!(message.contains("content_unhealthy:known_signature_hit"));
        assert!(message.contains("hash="));
        assert!(message.contains("sample_len="));
        assert!(!message.contains("SYNTHETIC_UNHEALTHY_MARKER"));
        assert!(!message.contains("should not be copied"));
    }

    #[test]
    fn blocks_html_login_or_captcha_200() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
        let body = b"<html><body><form>login captcha</form></body></html>";

        let result = classify_non_stream_response(&headers, body, true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::HtmlLoginOrCaptcha,
                ..
            })
        ));
    }

    #[test]
    fn allows_clean_json_text_that_mentions_login() {
        let body = br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"Please login with the documented command."}]}]}"#;

        assert_eq!(
            classify_non_stream_response(&HeaderMap::new(), body, true),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn blocks_schema_mismatch_for_generation_json_response() {
        let body = br#"{"ok":true}"#;

        let result = classify_non_stream_response(&HeaderMap::new(), body, true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::SchemaMismatch,
                ..
            })
        ));
    }

    #[test]
    fn allows_non_generation_json_without_protocol_shape() {
        let body = br#"{"ok":true}"#;

        assert_eq!(
            classify_non_stream_response(&HeaderMap::new(), body, false),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn blocks_malformed_tool_call_delta() {
        let body = br#"{"tool_calls":[{"function":{"arguments":"{}"}}]}"#;

        let result = classify_non_stream_response(&HeaderMap::new(), body, true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::MalformedToolCall,
                ..
            })
        ));
    }

    #[test]
    fn zero_usage_valid_response_is_not_unhealthy() {
        let body = br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":0,"output_tokens":0,"total_cost":0}}"#;

        assert_eq!(
            classify_non_stream_response(&HeaderMap::new(), body, true),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn zero_usage_invalid_response_is_unhealthy() {
        let body =
            br#"{"usage":{"input_tokens":0,"output_tokens":0,"total_cost":0},"note":"invalid"}"#;

        let result = classify_non_stream_response(&HeaderMap::new(), body, true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::ZeroUsageZeroCostInvalidResponse,
                ..
            })
        ));
    }

    #[test]
    fn blocks_repeated_unrelated_stream_fragments() {
        let repeated = "repeated synthetic unrelated fragment";
        let event_data = vec![
            repeated.to_string(),
            repeated.to_string(),
            repeated.to_string(),
        ];

        let result = classify_sse_prime_window(&event_data, repeated.as_bytes(), true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::RepeatedUnrelatedStreamFragment,
                ..
            })
        ));
    }

    #[test]
    fn allows_clean_openai_sse_json_event() {
        let event_data = vec![r#"{"type":"response.output_text.delta","delta":"ok"}"#.to_string()];

        assert_eq!(
            classify_sse_prime_window(&event_data, event_data[0].as_bytes(), true),
            ContentHealth::Healthy
        );
    }

    #[test]
    fn blocks_non_protocol_sse_data_for_generation_request() {
        let event_data = vec!["plain provider notice instead of protocol JSON".to_string()];

        let result = classify_sse_prime_window(&event_data, event_data[0].as_bytes(), true);

        assert!(matches!(
            result,
            ContentHealth::Unhealthy(ContentHealthViolation {
                reason: ContentHealthReason::SchemaMismatch,
                ..
            })
        ));
    }
}
