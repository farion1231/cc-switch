//! GLM reasoning capability normalization.
//!
//! GLM-5.3 and GLM-5.3-Flash are thinking-only models. Zhipu's API accepts
//! only `thinking.type = "enabled"` and the `low | high | max` effort domain.
//! Claude/Codex clients expose wider, provider-neutral controls, so direct
//! Zhipu routes need a model-scoped semantic translation before forwarding.
//!
//! Vendor references:
//! - <https://z.ai/blog/glm-5.3>
//! - <https://docs.z.ai/api-reference/llm/chat-completion>
//! - <https://docs.bigmodel.cn/cn/guide/models/vlm/glm-5.3-flash>
//!
//! The same `enabled + reasoning_effort` shape is accepted by Zhipu's
//! Anthropic-compatible endpoint (verified against the direct endpoint).

use crate::claude_desktop_config::ONE_M_CONTEXT_MARKER;
use serde_json::{json, Value};
use url::Url;

const THINKING_ONLY_GLM_5_3_MODEL_TAILS: &[&str] = &["glm-5.3", "glm-5.3-flash"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glm53Effort {
    Low,
    High,
    Max,
}

impl Glm53Effort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

/// Provider-neutral reasoning intent captured before a format converter can
/// collapse a tiered request into a plain on/off switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Glm53ReasoningIntent {
    effort: Option<Glm53Effort>,
}

impl Glm53ReasoningIntent {
    fn new(effort: Option<Glm53Effort>) -> Self {
        Self { effort }
    }
}

/// Exact GLM-5.3 forced-thinking-family detection after the model-name forms
/// CC Switch accepts are normalized. Deliberately fail open for unverified
/// variants such as 5.30, 5.3v, or flashx instead of inheriting capabilities
/// by prefix.
pub(crate) fn is_thinking_only_glm_5_3_family_model(model: &str) -> bool {
    let mut normalized = model.trim().to_ascii_lowercase();
    if normalized
        .as_bytes()
        .ends_with(ONE_M_CONTEXT_MARKER.as_bytes())
    {
        let keep = normalized.len() - ONE_M_CONTEXT_MARKER.len();
        normalized.truncate(keep);
        normalized = normalized.trim_end().to_string();
    }
    let normalized = normalized.strip_prefix("models/").unwrap_or(&normalized);
    normalized
        .rsplit('/')
        .next()
        .is_some_and(|tail| THINKING_ONLY_GLM_5_3_MODEL_TAILS.contains(&tail))
}

/// Only Zhipu's own endpoints are known to use the `thinking` plus top-level
/// `reasoning_effort` dialect documented for GLM-5.3. Hosted platforms are
/// intentionally excluded because their reasoning interface is platform-owned.
pub(crate) fn is_direct_zhipu_gateway(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return false;
    };
    matches!(
        url.host_str().map(str::to_ascii_lowercase).as_deref(),
        Some("api.z.ai" | "open.bigmodel.cn")
    )
}

fn map_effort(value: &str) -> Option<Glm53Effort> {
    match value.trim().to_ascii_lowercase().as_str() {
        // GLM-5.3 cannot honor an off/minimal tier. Zhipu's migration guide
        // explicitly maps disabled thinking to the lowest supported tier.
        "none" | "off" | "disabled" | "minimal" | "low" => Some(Glm53Effort::Low),
        "medium" | "high" => Some(Glm53Effort::High),
        "xhigh" | "max" | "ultra" => Some(Glm53Effort::Max),
        _ => None,
    }
}

fn budget_to_effort(budget: u64) -> Glm53Effort {
    // Preserve the ordering used by CC Switch's existing Codex→Anthropic
    // converter, then collapse the provider-neutral middle/high buckets into
    // GLM-5.3's documented three-level domain.
    match budget {
        0..=2_048 => Glm53Effort::Low,
        2_049..=16_384 => Glm53Effort::High,
        _ => Glm53Effort::Max,
    }
}

/// Capture reasoning intent from either Anthropic/Chat-shaped or
/// Responses-shaped input. `Some(intent-without-tier)` records an explicit but
/// unknown/on-with-default control so an illegal field can be removed without
/// inventing a tier; absence remains `None` and preserves the upstream default.
pub(crate) fn capture_glm_5_3_reasoning_intent(body: &Value) -> Option<Glm53ReasoningIntent> {
    let thinking_type = body
        .pointer("/thinking/type")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    if thinking_type.as_deref() == Some("disabled") {
        return Some(Glm53ReasoningIntent::new(Some(Glm53Effort::Low)));
    }

    if let Some(effort) = body.get("reasoning_effort").and_then(Value::as_str) {
        return Some(Glm53ReasoningIntent::new(map_effort(effort)));
    }
    if let Some(effort) = body.pointer("/reasoning/effort").and_then(Value::as_str) {
        return Some(Glm53ReasoningIntent::new(map_effort(effort)));
    }
    if body.get("reasoning").is_some_and(Value::is_null) {
        return Some(Glm53ReasoningIntent::new(Some(Glm53Effort::Low)));
    }
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
    {
        return Some(Glm53ReasoningIntent::new(map_effort(effort)));
    }

    match thinking_type.as_deref() {
        Some("adaptive") => Some(Glm53ReasoningIntent::new(Some(Glm53Effort::Max))),
        Some("enabled") => Some(Glm53ReasoningIntent::new(
            body.pointer("/thinking/budget_tokens")
                .and_then(Value::as_u64)
                .map(budget_to_effort),
        )),
        Some(_) => Some(Glm53ReasoningIntent::new(None)),
        None if body.get("reasoning").is_some() => Some(Glm53ReasoningIntent::new(None)),
        None => None,
    }
}

fn capture_glm_5_3_override_intent(body: &Value) -> Option<Glm53ReasoningIntent> {
    // An override object may contain neighboring fields such as
    // `reasoning.summary` or `thinking.clear_thinking` without overriding the
    // reasoning tier. Only the actual control fields establish a new intent.
    let has_explicit_control = body.pointer("/thinking/type").is_some()
        || body.get("reasoning_effort").is_some()
        || body.pointer("/reasoning/effort").is_some()
        || body.get("reasoning").is_some_and(Value::is_null)
        || body.pointer("/output_config/effort").is_some();
    if !has_explicit_control {
        return None;
    }

    // Preserve the ordinary capture precedence when the override itself
    // contains conflicting controls. A present but malformed control remains
    // authoritative as intent-without-tier so the illegal value is removed.
    Some(capture_glm_5_3_reasoning_intent(body).unwrap_or_else(|| Glm53ReasoningIntent::new(None)))
}

fn merge_intent(
    outbound: Option<Glm53ReasoningIntent>,
    original: Option<Glm53ReasoningIntent>,
) -> Option<Glm53ReasoningIntent> {
    match (outbound, original) {
        (Some(current), Some(fallback)) if current.effort.is_none() => {
            Some(Glm53ReasoningIntent::new(fallback.effort))
        }
        (Some(current), _) => Some(current),
        (None, fallback) => fallback,
    }
}

fn remove_output_config_effort(body: &mut Value) -> bool {
    let mut changed = false;
    let empty_after = body
        .get_mut("output_config")
        .and_then(Value::as_object_mut)
        .map(|config| {
            changed |= config.remove("effort").is_some();
            config.is_empty()
        })
        .unwrap_or(false);
    if empty_after {
        body.as_object_mut().unwrap().remove("output_config");
    }
    changed
}

fn apply_messages_intent(body: &mut Value, intent: Glm53ReasoningIntent) -> bool {
    let mut changed = false;

    match body.get_mut("thinking").and_then(Value::as_object_mut) {
        Some(thinking) => {
            if thinking.get("type").and_then(Value::as_str) != Some("enabled") {
                thinking.insert("type".to_string(), json!("enabled"));
                changed = true;
            }
            // GLM-5.3 exposes discrete effort tiers, not Anthropic token budgets.
            changed |= thinking.remove("budget_tokens").is_some();
        }
        None => {
            body["thinking"] = json!({ "type": "enabled" });
            changed = true;
        }
    }

    let desired_effort = intent.effort.map(Glm53Effort::as_str);
    let existing_effort = body.get("reasoning_effort").and_then(Value::as_str);
    match desired_effort {
        Some(effort) if existing_effort != Some(effort) => {
            body["reasoning_effort"] = json!(effort);
            changed = true;
        }
        None if body.get("reasoning_effort").is_some() => {
            body.as_object_mut().unwrap().remove("reasoning_effort");
            changed = true;
        }
        _ => {}
    }
    if let Some(object) = body.as_object_mut() {
        // Messages/Chat routes use Zhipu's top-level reasoning_effort dialect,
        // never the OpenAI Responses reasoning object.
        changed |= object.remove("reasoning").is_some();
    }
    changed |= remove_output_config_effort(body);

    changed
}

fn apply_responses_intent(body: &mut Value, intent: Glm53ReasoningIntent) -> bool {
    let mut changed = false;

    if let Some(object) = body.as_object_mut() {
        changed |= object.remove("thinking").is_some();
        changed |= object.remove("reasoning_effort").is_some();
    }
    changed |= remove_output_config_effort(body);

    if let Some(effort) = intent.effort {
        match body.get_mut("reasoning").and_then(Value::as_object_mut) {
            Some(reasoning) => {
                if reasoning.get("effort").and_then(Value::as_str) != Some(effort.as_str()) {
                    reasoning.insert("effort".to_string(), json!(effort.as_str()));
                    changed = true;
                }
            }
            None => {
                body["reasoning"] = json!({ "effort": effort.as_str() });
                changed = true;
            }
        }
    } else {
        let remove_reasoning = match body.get_mut("reasoning") {
            Some(Value::Object(reasoning)) => {
                changed |= reasoning.remove("effort").is_some();
                reasoning.is_empty()
            }
            Some(Value::Null) => true,
            _ => false,
        };
        if remove_reasoning {
            body.as_object_mut().unwrap().remove("reasoning");
            changed = true;
        }
    }

    changed
}

/// Normalize the final outbound request for a direct Zhipu GLM-5.3-family route.
///
/// `original_intent` is captured before protocol conversion. It restores tier
/// information that an on/off-only generic converter may otherwise discard.
pub(crate) fn normalize_direct_zhipu_glm_5_3_request(
    base_url: &str,
    body: &mut Value,
    original_intent: Option<Glm53ReasoningIntent>,
) -> bool {
    normalize_direct_zhipu_glm_5_3_request_with_override(base_url, body, original_intent, None)
}

/// Normalize with an optional local body-override layer. Explicit reasoning
/// controls in that layer outrank both the merged final body and the original
/// pre-conversion intent, regardless of which stale sibling fields remain.
pub(crate) fn normalize_direct_zhipu_glm_5_3_request_with_override(
    base_url: &str,
    body: &mut Value,
    original_intent: Option<Glm53ReasoningIntent>,
    final_override_body: Option<&Value>,
) -> bool {
    if !is_direct_zhipu_gateway(base_url)
        || !body
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(is_thinking_only_glm_5_3_family_model)
    {
        return false;
    }

    let override_intent = final_override_body.and_then(capture_glm_5_3_override_intent);
    let merged_intent = merge_intent(capture_glm_5_3_reasoning_intent(body), original_intent);
    let Some(intent) = override_intent.or(merged_intent) else {
        // No client reasoning control: preserve the upstream-enabled default
        // and its provider-selected effort rather than inventing a tier.
        return false;
    };

    if body.get("messages").is_some() {
        apply_messages_intent(body, intent)
    } else if body.get("input").is_some()
        || body.get("instructions").is_some()
        || body.get("reasoning").is_some()
    {
        apply_responses_intent(body, intent)
    } else {
        // A future or misconfigured wire format (for example Gemini native)
        // must fail open rather than receive OpenAI Responses-only fields.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::CodexChatReasoningConfig;
    use crate::proxy::providers::transform_responses;
    use crate::proxy::providers::{transform, transform_codex_anthropic, transform_codex_chat};

    #[test]
    fn model_detection_is_exact_and_normalizes_supported_wrappers() {
        for model in [
            "glm-5.3",
            "GLM-5.3",
            "glm-5.3[1M]",
            "zhipu/glm-5.3",
            "models/zai-org/GLM-5.3[1m]",
            "glm-5.3-flash",
            "GLM-5.3-FLASH[1M]",
            "zhipu/glm-5.3-flash",
            "models/zai-org/GLM-5.3-Flash[1m]",
        ] {
            assert!(is_thinking_only_glm_5_3_family_model(model), "{model}");
        }
        for model in [
            "glm-5.2",
            "glm-5.30",
            "glm-5.3v",
            "glm-5.3-air",
            "glm-5.3-flashx",
            "glm-5.3-flash-vision",
        ] {
            assert!(!is_thinking_only_glm_5_3_family_model(model), "{model}");
        }
    }

    #[test]
    fn direct_gateway_detection_rejects_host_lookalikes_and_aggregators() {
        for url in [
            "https://api.z.ai/api/anthropic",
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ] {
            assert!(is_direct_zhipu_gateway(url), "{url}");
        }
        for url in [
            "https://api.z.ai.evil.example/v1",
            "https://openrouter.ai/api/v1",
            "https://api.siliconflow.cn/v1",
            "not-a-url",
        ] {
            assert!(!is_direct_zhipu_gateway(url), "{url}");
        }
    }

    #[test]
    fn effort_mapping_covers_the_client_superset() {
        for (input, expected) in [
            ("none", Glm53Effort::Low),
            ("off", Glm53Effort::Low),
            ("disabled", Glm53Effort::Low),
            ("minimal", Glm53Effort::Low),
            ("low", Glm53Effort::Low),
            ("medium", Glm53Effort::High),
            ("high", Glm53Effort::High),
            ("xhigh", Glm53Effort::Max),
            ("max", Glm53Effort::Max),
            ("ultra", Glm53Effort::Max),
        ] {
            assert_eq!(map_effort(input), Some(expected), "{input}");
        }
        assert_eq!(map_effort("unknown"), None);
    }

    #[test]
    fn messages_disabled_becomes_enabled_low_and_preserves_siblings() {
        let mut body = json!({
            "model": "glm-5.3-flash",
            "messages": [{ "role": "user", "content": "hi" }],
            "thinking": {
                "type": "disabled",
                "budget_tokens": 8192,
                "clear_thinking": false
            },
            "output_config": { "effort": "max", "verbosity": "low" }
        });

        assert!(normalize_direct_zhipu_glm_5_3_request(
            "https://api.z.ai/api/anthropic",
            &mut body,
            None,
        ));
        assert_eq!(
            body["thinking"],
            json!({ "type": "enabled", "clear_thinking": false })
        );
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["output_config"], json!({ "verbosity": "low" }));
    }

    #[test]
    fn final_override_layer_resolves_stale_control_conflicts_symmetrically() {
        for sibling_only in [
            json!({ "thinking": { "clear_thinking": false } }),
            json!({ "reasoning": { "summary": "auto" } }),
        ] {
            assert_eq!(capture_glm_5_3_override_intent(&sibling_only), None);
        }
        assert_eq!(
            capture_glm_5_3_override_intent(&json!({ "reasoning_effort": null })),
            Some(Glm53ReasoningIntent::new(None))
        );

        for (override_body, expected_effort) in [
            (json!({ "reasoning_effort": "max" }), "max"),
            (json!({ "thinking": { "type": "disabled" } }), "low"),
        ] {
            // The merged body is deliberately identical in both cases. The
            // explicit override layer determines which of the stale siblings
            // is authoritative.
            let mut body = json!({
                "model": "glm-5.3-flash",
                "messages": [{ "role": "user", "content": "hi" }],
                "thinking": { "type": "disabled", "clear_thinking": false },
                "reasoning_effort": "max"
            });

            assert!(normalize_direct_zhipu_glm_5_3_request_with_override(
                "https://open.bigmodel.cn/api/anthropic",
                &mut body,
                None,
                Some(&override_body),
            ));
            assert_eq!(
                body["thinking"],
                json!({ "type": "enabled", "clear_thinking": false })
            );
            assert_eq!(body["reasoning_effort"], expected_effort);
        }

        for (override_body, expected_effort) in [
            (json!({ "reasoning": { "effort": "max" } }), "max"),
            (json!({ "thinking": { "type": "disabled" } }), "low"),
        ] {
            let mut body = json!({
                "model": "glm-5.3-flash",
                "input": "hi",
                "thinking": { "type": "disabled" },
                "reasoning": { "effort": "max", "summary": "auto" }
            });

            assert!(normalize_direct_zhipu_glm_5_3_request_with_override(
                "https://api.z.ai/api/v1",
                &mut body,
                None,
                Some(&override_body),
            ));
            assert!(body.get("thinking").is_none());
            assert_eq!(body["reasoning"]["effort"], expected_effort);
            assert_eq!(body["reasoning"]["summary"], "auto");
        }
    }

    #[test]
    fn adaptive_claude_controls_become_enabled_max() {
        let mut body = json!({
            "model": "zhipu/GLM-5.3[1M]",
            "messages": [{ "role": "user", "content": "hi" }],
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "max" }
        });

        normalize_direct_zhipu_glm_5_3_request(
            "https://open.bigmodel.cn/api/anthropic",
            &mut body,
            None,
        );
        assert_eq!(body["thinking"], json!({ "type": "enabled" }));
        assert_eq!(body["reasoning_effort"], "max");
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn legacy_budget_controls_collapse_to_legal_effort_tiers() {
        for (budget, expected) in [(2_048, "low"), (8_192, "high"), (24_576, "max")] {
            let mut body = json!({
                "model": "glm-5.3",
                "messages": [{ "role": "user", "content": "hi" }],
                "thinking": { "type": "enabled", "budget_tokens": budget }
            });

            normalize_direct_zhipu_glm_5_3_request(
                "https://api.z.ai/api/anthropic",
                &mut body,
                None,
            );
            assert_eq!(body["thinking"], json!({ "type": "enabled" }));
            assert_eq!(body["reasoning_effort"], expected, "budget={budget}");
        }
    }

    #[test]
    fn converted_chat_restores_original_effort_when_converter_kept_only_on_off() {
        let original = json!({
            "reasoning": { "effort": "medium" },
            "input": "hi"
        });
        let intent = capture_glm_5_3_reasoning_intent(&original);
        let mut converted = json!({
            "model": "glm-5.3-flash",
            "messages": [{ "role": "user", "content": "hi" }],
            "thinking": { "type": "enabled" }
        });

        assert!(normalize_direct_zhipu_glm_5_3_request(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            &mut converted,
            intent,
        ));
        assert_eq!(converted["thinking"]["type"], "enabled");
        assert_eq!(converted["reasoning_effort"], "high");
    }

    #[test]
    fn converted_messages_disable_wins_over_a_stronger_original_tier() {
        let original = capture_glm_5_3_reasoning_intent(&json!({
            "reasoning": { "effort": "max" }
        }));
        let mut converted = json!({
            "model": "glm-5.3",
            "messages": [{ "role": "user", "content": "hi" }],
            "thinking": { "type": "disabled" }
        });

        normalize_direct_zhipu_glm_5_3_request(
            "https://api.z.ai/api/anthropic",
            &mut converted,
            original,
        );
        assert_eq!(converted["thinking"]["type"], "enabled");
        assert_eq!(converted["reasoning_effort"], "low");
    }

    #[test]
    fn responses_none_becomes_low_and_preserves_reasoning_siblings() {
        let mut body = json!({
            "model": "glm-5.3-flash",
            "input": "hi",
            "reasoning": { "effort": "none", "summary": "auto" }
        });

        normalize_direct_zhipu_glm_5_3_request("https://api.z.ai/api/v1", &mut body, None);
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["reasoning"]["summary"], "auto");
    }

    #[test]
    fn responses_unknown_effort_is_dropped_without_losing_siblings() {
        let mut body = json!({
            "model": "glm-5.3",
            "input": "hi",
            "reasoning": { "effort": "bogus", "summary": "auto" }
        });

        normalize_direct_zhipu_glm_5_3_request("https://api.z.ai/api/v1", &mut body, None);
        assert!(body["reasoning"].get("effort").is_none());
        assert_eq!(body["reasoning"]["summary"], "auto");
    }

    #[test]
    fn responses_conversion_restores_disabled_intent_after_field_loss() {
        let original = capture_glm_5_3_reasoning_intent(&json!({
            "thinking": { "type": "disabled" }
        }));
        let mut converted = json!({ "model": "glm-5.3", "input": "hi" });

        normalize_direct_zhipu_glm_5_3_request("https://api.z.ai/api/v1", &mut converted, original);
        assert_eq!(converted["reasoning"], json!({ "effort": "low" }));
    }

    #[test]
    fn claude_to_chat_and_responses_restore_disabled_as_low() {
        let input = json!({
            "model": "glm-5.3",
            "max_tokens": 128,
            "thinking": { "type": "disabled" },
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let intent = capture_glm_5_3_reasoning_intent(&input);

        let mut chat = transform::anthropic_to_openai_with_reasoning_content(input.clone(), false)
            .expect("Claude→Chat conversion");
        assert!(chat.get("thinking").is_none());
        normalize_direct_zhipu_glm_5_3_request(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            &mut chat,
            intent,
        );
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert_eq!(chat["reasoning_effort"], "low");

        let mut responses = transform_responses::anthropic_to_responses(input, None, false, false)
            .expect("Claude→Responses conversion");
        assert!(responses.get("reasoning").is_none());
        normalize_direct_zhipu_glm_5_3_request("https://api.z.ai/api/v1", &mut responses, intent);
        assert_eq!(responses["reasoning"], json!({ "effort": "low" }));
    }

    #[test]
    fn codex_to_chat_and_anthropic_never_forward_disabled() {
        let input = json!({
            "model": "glm-5.3",
            "max_output_tokens": 128,
            "reasoning": null,
            "input": [{ "role": "user", "content": "hi" }]
        });
        let intent = capture_glm_5_3_reasoning_intent(&input);
        let toggle_only_zhipu = CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
            effort_levels: None,
        };

        let mut chat = transform_codex_chat::responses_to_chat_completions_with_reasoning(
            input.clone(),
            Some(&toggle_only_zhipu),
        )
        .expect("Codex→Chat conversion");
        assert_eq!(chat["thinking"]["type"], "disabled");
        normalize_direct_zhipu_glm_5_3_request(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            &mut chat,
            intent,
        );
        assert_eq!(chat["thinking"]["type"], "enabled");
        assert_eq!(chat["reasoning_effort"], "low");

        let mut anthropic = transform_codex_anthropic::responses_request_to_anthropic(input, 128)
            .expect("Codex→Anthropic conversion");
        assert!(anthropic.get("thinking").is_none());
        normalize_direct_zhipu_glm_5_3_request(
            "https://api.z.ai/api/anthropic",
            &mut anthropic,
            intent,
        );
        assert_eq!(anthropic["thinking"]["type"], "enabled");
        assert_eq!(anthropic["reasoning_effort"], "low");
    }

    #[test]
    fn absent_controls_and_neighboring_models_remain_byte_identical() {
        let cases = [
            json!({ "model": "glm-5.3", "messages": [{ "role": "user", "content": "hi" }] }),
            json!({ "model": "glm-5.3-flash", "messages": [{ "role": "user", "content": "hi" }] }),
            json!({ "model": "glm-5.2", "messages": [], "thinking": { "type": "disabled" } }),
            json!({ "model": "glm-5.3", "messages": [], "thinking": { "type": "disabled" } }),
        ];
        let urls = [
            "https://api.z.ai/api/anthropic",
            "https://api.z.ai/api/anthropic",
            "https://api.z.ai/api/anthropic",
            "https://openrouter.ai/api/v1",
        ];

        for (mut body, url) in cases.into_iter().zip(urls) {
            let original = body.clone();
            assert!(!normalize_direct_zhipu_glm_5_3_request(
                url, &mut body, None
            ));
            assert_eq!(body, original);
        }
    }

    #[test]
    fn unknown_wire_shapes_fail_open_even_with_a_captured_intent() {
        let intent = capture_glm_5_3_reasoning_intent(&json!({
            "thinking": { "type": "disabled" }
        }));
        let mut gemini = json!({
            "model": "glm-5.3",
            "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
            "generationConfig": {}
        });
        let original = gemini.clone();

        assert!(!normalize_direct_zhipu_glm_5_3_request(
            "https://api.z.ai/api/anthropic",
            &mut gemini,
            intent,
        ));
        assert_eq!(gemini, original);
    }

    #[test]
    fn final_override_tier_wins_on_the_second_normalization_pass() {
        let original = capture_glm_5_3_reasoning_intent(&json!({
            "thinking": { "type": "disabled" }
        }));
        let mut body = json!({
            "model": "glm-5.3",
            "messages": [{ "role": "user", "content": "hi" }]
        });
        normalize_direct_zhipu_glm_5_3_request(
            "https://api.z.ai/api/anthropic",
            &mut body,
            original,
        );
        assert_eq!(body["reasoning_effort"], "low");

        body["reasoning_effort"] = json!("max");
        normalize_direct_zhipu_glm_5_3_request("https://api.z.ai/api/anthropic", &mut body, None);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "max");
    }
}
