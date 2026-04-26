//! Thinking 优化器

use super::types::OptimizerConfig;
use serde_json::{json, Value};

/// 根据模型类型自动优化 thinking 配置
///
/// 三路径分发：
/// - skip: haiku 模型直接跳过
/// - adaptive: opus-4-7 / opus-4-6 / sonnet-4-6 使用 adaptive thinking
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

    if model.contains("opus-4-7") || model.contains("opus-4-6") || model.contains("sonnet-4-6") {
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
    match body.get("anthropic_beta") {
        Some(Value::Array(arr)) => {
            if arr.iter().any(|v| v.as_str() == Some(beta)) {
                return;
            }
            body["anthropic_beta"]
                .as_array_mut()
                .unwrap()
                .push(json!(beta));
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
            cache_ttl: "1h".to_string(),
        }
    }

    fn disabled_config() -> OptimizerConfig {
        OptimizerConfig {
            enabled: true,
            thinking_optimizer: false,
            cache_injection: true,
            cache_ttl: "1h".to_string(),
        }
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
