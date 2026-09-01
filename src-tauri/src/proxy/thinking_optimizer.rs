//! Thinking 优化器

use super::thinking_capability::ThinkingMode;
use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 仅测试用：用内置启发式判断 thinking 形状后优化。
///
/// 生产链路一律走 [`optimize_with_mode`] 传入 store 解析结果，这样才能吃到从上游报错
/// 学到的形状修正；这里用的启发式就是 store 未命中时的回落逻辑。
#[cfg(test)]
fn optimize(body: &mut Value, config: &OptimizerConfig) {
    let mode = body
        .get("model")
        .and_then(|m| m.as_str())
        .map(super::thinking_capability::resolve_mode)
        .unwrap_or(ThinkingMode::Adaptive);
    optimize_with_mode(body, config, mode);
}

/// 根据 thinking 形状自动优化 thinking 配置
///
/// 三路径分发：
/// - skip: haiku 模型直接跳过
/// - adaptive: `thinking: {"type":"adaptive"}` + `output_config.effort`
/// - legacy: 注入 enabled thinking + budget_tokens
pub fn optimize_with_mode(body: &mut Value, config: &OptimizerConfig, mode: ThinkingMode) {
    if !config.thinking_optimizer {
        return;
    }

    let model = match body.get("model").and_then(|m| m.as_str()) {
        Some(m) => m.to_lowercase(),
        None => return,
    };

    // Haiku 一律跳过，**有意**优先于 `mode`（包括从上游报错学到的形状）。
    //
    // 两个原因：Haiku 4.5 既不支持 adaptive 也不支持 effort；而 Haiku 是廉价快速档，
    // 本优化器的动作是激进注入（effort=max / budget=max_tokens-1），对这一档并不想要。
    // 所以即便 `adaptive_threshold(Family::Haiku)` 是 `Some((5, 0))`、未来的
    // claude-haiku-5 会被解析成 Adaptive，这里也不注入。
    //
    // 这不会阻断自愈：学到的形状修正是由 `normalize_thinking_shape` 落到请求体上的
    //（整流器重试路径 + Anthropic 直通的发送端归一化，两处都不含 haiku 门禁），
    // 本函数只负责「是否主动加 thinking」，不负责改写已有形状。
    if model.contains("haiku") {
        log::info!("[OPT] thinking: skip(haiku)");
        return;
    }

    if mode == ThinkingMode::Adaptive {
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

    fn disabled_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: false,
            cache_injection: true,
        }
    }

    #[test]
    fn test_adaptive_opus_4_8() {
        let mut body = json!({
            "model": "anthropic/claude-opus-4.8",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn test_adaptive_opus_5() {
        // 回归：opus-5 不在任何白名单里，旧实现会把它落到 legacy 分支并注入
        // budget_tokens，被 Anthropic 以 400 拒绝。
        let mut body = json!({
            "model": "claude-opus-5",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
    }

    #[test]
    fn test_adaptive_for_unknown_and_future_models() {
        // 「未知的 Claude 即新 Claude」：解析不出版本的 Claude 别名也走 adaptive，
        // 出新模型不必改代码
        for model in ["claude-opus-6", "claude-sonnet-6", "claude-gateway-alias"] {
            let mut body = json!({
                "model": model,
                "max_tokens": 16384,
                "messages": [{"role": "user", "content": "hello"}]
            });

            optimize(&mut body, &enabled_config());

            assert_eq!(body["thinking"]["type"], "adaptive", "model={model}");
            assert!(
                body["thinking"].get("budget_tokens").is_none(),
                "model={model}"
            );
        }
    }

    #[test]
    fn test_optimize_with_mode_overrides_the_heuristic() {
        // 学到的形状要能压过启发式：store 说 legacy 就走 legacy
        let mut body = json!({
            "model": "claude-opus-5",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize_with_mode(&mut body, &enabled_config(), ThinkingMode::Legacy);

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 16383);
    }

    #[test]
    fn test_adaptive_opus_4_6() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn test_adaptive_sonnet_4_6() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "max");
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }

    #[test]
    fn test_legacy_sonnet_4_5_thinking_null() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 16383);
        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "interleaved-thinking-2025-05-14"));
    }

    #[test]
    fn test_legacy_budget_too_small_upgraded() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 16384,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 16383);
    }

    #[test]
    fn test_skip_haiku() {
        let mut body = json!({
            "model": "anthropic.claude-haiku-4-5-20250514-v1:0",
            "max_tokens": 8192,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let original = body.clone();

        optimize(&mut body, &enabled_config());

        assert_eq!(body, original);
    }

    #[test]
    fn test_thinking_optimizer_disabled() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let original = body.clone();

        optimize(&mut body, &disabled_config());

        assert_eq!(body, original);
    }

    #[test]
    fn test_adaptive_dedup_beta() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 16384,
            "anthropic_beta": ["context-1m-2025-08-07"],
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        let betas = body["anthropic_beta"].as_array().unwrap();
        let count = betas
            .iter()
            .filter(|v| v == &&json!("context-1m-2025-08-07"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_legacy_disabled_thinking_injected() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "max_tokens": 8192,
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8191);
    }

    #[test]
    fn test_legacy_default_max_tokens() {
        let mut body = json!({
            "model": "anthropic.claude-sonnet-4-5-20250514-v1:0",
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 16383);
    }

    #[test]
    fn test_append_beta_null_field() {
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "anthropic_beta": null,
            "messages": [{"role": "user", "content": "hello"}]
        });

        optimize(&mut body, &enabled_config());

        let betas = body["anthropic_beta"].as_array().unwrap();
        assert!(betas.iter().any(|v| v == "context-1m-2025-08-07"));
    }
}
