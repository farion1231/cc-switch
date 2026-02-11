//! Thinking Budget 整流器
//!
//! 用于自动修复 Anthropic API 中因 thinking budget 约束导致的请求错误。
//! 当上游 API 返回 budget_tokens 相关错误时，系统会自动调整 budget 参数并重试。

use super::types::RectifierConfig;
use serde_json::Value;

/// 最大 thinking budget tokens
const MAX_THINKING_BUDGET: u64 = 32000;

/// 最大 max_tokens 值
const MAX_TOKENS_VALUE: u64 = 64000;

/// max_tokens 必须大于 budget_tokens
const MIN_MAX_TOKENS_FOR_BUDGET: u64 = MAX_THINKING_BUDGET + 1;

/// Budget 整流结果
#[derive(Debug, Clone, Default)]
pub struct BudgetRectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 是否修改了 thinking type
    pub type_changed: bool,
    /// 是否修改了 budget_tokens
    pub budget_changed: bool,
    /// 是否修改了 max_tokens
    pub max_tokens_changed: bool,
}

/// 检测是否需要触发 thinking budget 整流器
///
/// 检测条件：error message 同时包含 `budget_tokens` + `thinking` 相关约束
pub fn should_rectify_thinking_budget(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> bool {
    // 检查总开关
    if !config.enabled {
        return false;
    }
    // 检查子开关
    if !config.request_thinking_budget {
        return false;
    }

    let Some(msg) = error_message else {
        return false;
    };
    let lower = msg.to_lowercase();

    // 覆盖常见上游文案变体：
    // - budget_tokens >= 1024 约束
    // - budget_tokens 与 max_tokens 关系约束
    let has_budget_tokens_reference =
        lower.contains("budget_tokens") || lower.contains("budget tokens");
    let has_1024_constraint = lower.contains("greater than or equal to 1024")
        || lower.contains(">= 1024")
        || lower.contains("at least 1024")
        || (lower.contains("1024") && lower.contains("input should be"));
    let has_max_tokens_constraint = lower.contains("less than max_tokens")
        || (lower.contains("budget_tokens")
            && lower.contains("max_tokens")
            && (lower.contains("must be less than") || lower.contains("should be less than")));
    let has_thinking_reference = lower.contains("thinking");

    if has_budget_tokens_reference && (has_1024_constraint || has_max_tokens_constraint) {
        return true;
    }

    // 兜底：部分网关会省略 budget_tokens 字段名，但保留 thinking + 1024 线索
    if has_thinking_reference && has_1024_constraint {
        return true;
    }

    false
}

/// 对请求体执行 budget 整流
///
/// 整流动作：
/// - `thinking.type = "enabled"`
/// - `thinking.budget_tokens = 32000`
/// - 如果 `max_tokens < 32001`，设为 `64000`
pub fn rectify_thinking_budget(body: &mut Value) -> BudgetRectifyResult {
    let mut result = BudgetRectifyResult::default();

    // 仅允许对显式 thinking.type=enabled 的请求做 budget 整流，避免静默语义升级。
    let Some(thinking_obj) = body.get("thinking").and_then(|t| t.as_object()) else {
        log::warn!("[RECT-BUD-001] budget 整流命中但请求缺少 thinking 对象，跳过");
        return result;
    };
    let current_type = thinking_obj.get("type").and_then(|t| t.as_str());
    if current_type == Some("adaptive") {
        log::warn!("[RECT-BUD-002] budget 整流命中但 thinking.type=adaptive，跳过");
        return result;
    }
    if current_type != Some("enabled") {
        log::warn!(
            "[RECT-BUD-003] budget 整流命中但 thinking.type 不是 enabled（当前: {}），跳过",
            current_type.unwrap_or("<missing>")
        );
        return result;
    }
    let Some(thinking) = body.get_mut("thinking").and_then(|t| t.as_object_mut()) else {
        return result;
    };

    // 设置 budget_tokens = MAX_THINKING_BUDGET
    let current_budget = thinking.get("budget_tokens").and_then(|v| v.as_u64());
    if current_budget != Some(MAX_THINKING_BUDGET) {
        thinking.insert(
            "budget_tokens".to_string(),
            Value::Number(MAX_THINKING_BUDGET.into()),
        );
        result.budget_changed = true;
    }

    // 确保 max_tokens >= MIN_MAX_TOKENS_FOR_BUDGET
    let current_max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    if current_max_tokens < MIN_MAX_TOKENS_FOR_BUDGET {
        body["max_tokens"] = Value::Number(MAX_TOKENS_VALUE.into());
        result.max_tokens_changed = true;
    }

    result.applied = result.type_changed || result.budget_changed || result.max_tokens_changed;
    if !result.applied {
        log::warn!("[RECT-BUD-004] budget 整流命中但请求已满足约束，跳过重试");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: true,
        }
    }

    fn budget_disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: true,
            request_thinking_signature: true,
            request_thinking_budget: false,
        }
    }

    fn master_disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: false,
            request_thinking_signature: true,
            request_thinking_budget: true,
        }
    }

    // ==================== should_rectify_thinking_budget 测试 ====================

    #[test]
    fn test_detect_budget_tokens_thinking_error() {
        assert!(should_rectify_thinking_budget(
            Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_budget_tokens_max_tokens_error() {
        assert!(should_rectify_thinking_budget(
            Some("budget_tokens must be less than max_tokens"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_budget_tokens_1024_error() {
        assert!(should_rectify_thinking_budget(
            Some("budget_tokens: value must be at least 1024"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_detect_budget_tokens_with_thinking_and_1024_error() {
        assert!(should_rectify_thinking_budget(
            Some("thinking budget_tokens must be >= 1024"),
            &enabled_config()
        ));
    }

    #[test]
    fn test_no_trigger_for_unrelated_error() {
        assert!(!should_rectify_thinking_budget(
            Some("Request timeout"),
            &enabled_config()
        ));
        assert!(!should_rectify_thinking_budget(None, &enabled_config()));
    }

    #[test]
    fn test_disabled_budget_config() {
        assert!(!should_rectify_thinking_budget(
            Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
            &budget_disabled_config()
        ));
    }

    #[test]
    fn test_master_disabled() {
        assert!(!should_rectify_thinking_budget(
            Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
            &master_disabled_config()
        ));
    }

    // ==================== rectify_thinking_budget 测试 ====================

    #[test]
    fn test_rectify_budget_basic() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 512 },
            "max_tokens": 1024
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(result.applied);
        assert!(result.budget_changed);
        assert!(result.max_tokens_changed);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], MAX_THINKING_BUDGET);
        assert_eq!(body["max_tokens"], MAX_TOKENS_VALUE);
    }

    #[test]
    fn test_rectify_budget_skips_adaptive() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "adaptive", "budget_tokens": 512 },
            "max_tokens": 1024
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(!result.applied);
        assert!(!result.type_changed);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["thinking"]["budget_tokens"], 512);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_rectify_budget_preserves_large_max_tokens() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 512 },
            "max_tokens": 100000
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(result.applied);
        assert!(!result.max_tokens_changed);
        assert_eq!(body["max_tokens"], 100000);
    }

    #[test]
    fn test_rectify_budget_creates_thinking_object_when_missing() {
        let mut body = json!({
            "model": "claude-test",
            "max_tokens": 1024
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(!result.applied);
        assert!(body.get("thinking").is_none());
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_rectify_budget_no_max_tokens() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 512 }
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(result.applied);
        assert!(result.max_tokens_changed);
        assert_eq!(body["max_tokens"], MAX_TOKENS_VALUE);
    }

    #[test]
    fn test_rectify_budget_skips_non_enabled_type() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "disabled", "budget_tokens": 512 },
            "max_tokens": 1024
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(!result.applied);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["thinking"]["budget_tokens"], 512);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn test_rectify_budget_no_change_when_already_valid() {
        let mut body = json!({
            "model": "claude-test",
            "thinking": { "type": "enabled", "budget_tokens": 32000 },
            "max_tokens": 64001
        });

        let result = rectify_thinking_budget(&mut body);

        assert!(!result.applied);
        assert!(!result.budget_changed);
        assert!(!result.max_tokens_changed);
        assert_eq!(body["thinking"]["budget_tokens"], 32000);
        assert_eq!(body["max_tokens"], 64001);
    }
}
