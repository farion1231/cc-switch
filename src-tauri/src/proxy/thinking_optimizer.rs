//! Thinking 优化器

use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 根据模型类型自动优化 thinking 配置
///
/// 三路径分发：
/// - skip: haiku 模型直接跳过
/// - adaptive: current adaptive-thinking Claude models use adaptive thinking
/// - legacy: 其他模型注入 enabled thinking + budget_tokens
pub fn optimize(body: &mut Value, config: &OptimizerConfig) {
    if !config.thinking_optimizer {
        return;
    }

    let model = match body.get("model").and_then(|m| m.as_str()) {
        Some(m) => m.to_lowercase(),
        None => return,
    };

    if model.contains("haiku") {
        log::info!("[OPT] thinking: skip(haiku)");
        return;
    }

    if uses_adaptive_thinking(&model) {
        log::info!("[OPT] thinking: adaptive({model})");
        body["thinking"] = json!({"type": "adaptive"});
        body["output_config"] = json!({"effort": "max"});
        append_beta(body, "context-1m-2025-08-07");
        return;
    }

    // legacy path
    log::info!("[OPT] thinking: legacy({model})");

    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(16384);

    let budget_target = max_tokens.saturating_sub(1);

    let thinking_type = body
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    match thinking_type.as_deref() {
        None | Some("disabled") => {
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget_target
            });
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
        Some("enabled") => {
            let current_budget = body
                .get("thinking")
                .and_then(|t| t.get("budget_tokens"))
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            if current_budget < budget_target {
                body["thinking"]["budget_tokens"] = json!(budget_target);
            }
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
        _ => {
            append_beta(body, "interleaved-thinking-2025-05-14");
        }
    }
}

pub(crate) fn uses_adaptive_thinking(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    [
        "fable-5",
        "mythos-5",
        "mythos-preview",
        "sonnet-5",
        "opus-5",
        "opus-4-8",
        "opus-4-7",
        "opus-4-6",
        "sonnet-4-6",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

/// Models where omitting `thinking` still leaves adaptive thinking enabled
/// (Claude thinking docs, per-model table: the 5.x Fable / Mythos / Opus /
/// Sonnet generations and Mythos Preview; Opus 4.x and Sonnet 4.x are off).
pub(crate) fn adaptive_thinking_is_default(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    [
        "fable-5",
        "mythos-5",
        "mythos-preview",
        "opus-5",
        "sonnet-5",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

/// Models that reject `thinking: {"type":"disabled"}` at every effort level
/// (Fable / Mythos 5.x, Opus 5.5 and Sonnet 5.5).
pub(crate) fn thinking_cannot_be_disabled(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    ["fable-5", "mythos-5", "opus-5-5", "sonnet-5-5"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

/// Adaptive-only models that accept a replayed tool turn without a thinking
/// block (adaptive thinking may skip thinking before a tool call), so unsigned
/// tool history keeps adaptive thinking instead of having to disable it.
pub(crate) fn accepts_unsigned_tool_history(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    ["opus-5-5", "sonnet-5-5"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn normalize_model_name(model: &str) -> String {
    model.trim().to_ascii_lowercase().replace(['.', '_'], "-")
}

/// 追加 beta 标识到 anthropic_beta 数组（去重）
fn append_beta(body: &mut Value, beta: &str) {
    match body.get_mut("anthropic_beta") {
        Some(Value::Array(arr)) => {
            if arr.iter().any(|v| v.as_str() == Some(beta)) {
                return;
            }
            arr.push(json!(beta));
        }
        Some(Value::Null) | None => {
            body["anthropic_beta"] = json!([beta]);
        }
        _ => {
            body["anthropic_beta"] = json!([beta]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: true,
            cache_injection: true,
        }
    }

    #[test]
    fn current_generation_models_use_adaptive_thinking() {
        for model in [
            "claude-sonnet-5",
            "anthropic/claude-fable-5",
            "claude-mythos-5",
            "claude-opus-5",
            "claude-opus-4.8",
        ] {
            assert!(uses_adaptive_thinking(model), "model={model}");
        }
        for model in [
            "claude-sonnet-5",
            "claude-sonnet-5-5",
            "claude-opus-5",
            "claude-opus-5-5",
            "claude-fable-5-1",
            "claude-mythos-5-1",
        ] {
            assert!(adaptive_thinking_is_default(model), "model={model}");
        }
        for model in ["claude-opus-4-8", "claude-opus-4-5", "claude-sonnet-4-6"] {
            assert!(!adaptive_thinking_is_default(model), "model={model}");
        }
        assert!(thinking_cannot_be_disabled("claude-fable-5"));
        assert!(!thinking_cannot_be_disabled("claude-sonnet-5"));
        assert!(!thinking_cannot_be_disabled("claude-opus-5"));
        for model in [
            "claude-opus-5-5",
            "claude-opus-5.5",
            "claude-opus-5-5[1m]",
            "claude-opus-5-5@20261101",
            "claude-sonnet-5-5",
        ] {
            assert!(thinking_cannot_be_disabled(model), "model={model}");
            assert!(accepts_unsigned_tool_history(model), "model={model}");
        }
        for model in ["claude-fable-5-1", "claude-opus-5", "claude-sonnet-5"] {
            assert!(!accepts_unsigned_tool_history(model), "model={model}");
        }
    }

    #[test]
    fn optimize_adaptive_and_beta_handling() {
        let cfg = enabled_config();
        for (case, mut body) in [
            (
                "opus_4_8_replaces_enabled_thinking",
                json!({
                    "model": "anthropic/claude-opus-4.8",
                    "max_tokens": 16384,
                    "thinking": {"type": "enabled", "budget_tokens": 8000},
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            (
                "opus_4_6_replaces_enabled_thinking",
                json!({
                    "model": "anthropic.claude-opus-4-6-20250514-v1:0",
                    "max_tokens": 16384,
                    "thinking": {"type": "enabled", "budget_tokens": 8000},
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            (
                "sonnet_4_6_without_thinking",
                json!({
                    "model": "anthropic.claude-sonnet-4-6-20250514-v1:0",
                    "max_tokens": 16384,
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            (
                "dedups_existing_beta",
                json!({
                    "model": "anthropic.claude-opus-4-6-20250514-v1:0",
                    "max_tokens": 16384,
                    "anthropic_beta": ["context-1m-2025-08-07"],
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            (
                "replaces_null_beta_field",
                json!({
                    "model": "anthropic.claude-opus-4-6-20250514-v1:0",
                    "anthropic_beta": null,
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
        ] {
            optimize(&mut body, &cfg);
            assert_eq!(body["thinking"]["type"], "adaptive", "{case}");
            assert!(body["thinking"].get("budget_tokens").is_none(), "{case}");
            assert_eq!(body["output_config"]["effort"], "max", "{case}");
            let betas = body["anthropic_beta"].as_array().unwrap();
            let count = betas
                .iter()
                .filter(|v| *v == &json!("context-1m-2025-08-07"))
                .count();
            assert_eq!(count, 1, "{case}");
        }
    }

    #[test]
    fn optimize_legacy_budget_and_skip_paths() {
        let cfg = enabled_config();
        for (case, mut body, expected_budget) in [
            (
                "missing_thinking",
                json!({
                    "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
                    "max_tokens": 16384,
                    "messages": [{"role": "user", "content": "hello"}]
                }),
                16383,
            ),
            (
                "small_enabled_budget_upgraded",
                json!({
                    "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
                    "max_tokens": 16384,
                    "thinking": {"type": "enabled", "budget_tokens": 1024},
                    "messages": [{"role": "user", "content": "hello"}]
                }),
                16383,
            ),
            (
                "disabled_thinking_injected",
                json!({
                    "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
                    "max_tokens": 8192,
                    "thinking": {"type": "disabled"},
                    "messages": [{"role": "user", "content": "hello"}]
                }),
                8191,
            ),
            (
                "default_max_tokens_when_omitted",
                json!({
                    "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
                    "messages": [{"role": "user", "content": "hello"}]
                }),
                16383,
            ),
        ] {
            optimize(&mut body, &cfg);
            assert_eq!(body["thinking"]["type"], "enabled", "{case}");
            assert_eq!(body["thinking"]["budget_tokens"], expected_budget, "{case}");
            let betas = body["anthropic_beta"].as_array().unwrap();
            assert!(
                betas.iter().any(|v| v == "interleaved-thinking-2025-05-14"),
                "{case}"
            );
        }

        // haiku 模型与开关关闭均保持原样
        let mut haiku = json!({
            "model": "anthropic.claude-haiku-4-5-20250514-v1:0",
            "max_tokens": 8192,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let haiku_orig = haiku.clone();
        optimize(&mut haiku, &cfg);
        assert_eq!(haiku, haiku_orig);

        let mut disabled_body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let disabled_orig = disabled_body.clone();
        optimize(
            &mut disabled_body,
            &OptimizerConfig {
                thinking_optimizer: false,
                ..cfg
            },
        );
        assert_eq!(disabled_body, disabled_orig);
    }
}
