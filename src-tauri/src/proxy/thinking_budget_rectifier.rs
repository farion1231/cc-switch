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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BudgetRectifySnapshot {
    /// max_tokens
    pub max_tokens: Option<u64>,
    /// thinking.type
    pub thinking_type: Option<String>,
    /// thinking.budget_tokens
    pub thinking_budget_tokens: Option<u64>,
}

/// Budget 整流结果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BudgetRectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 整流前快照
    pub before: BudgetRectifySnapshot,
    /// 整流后快照
    pub after: BudgetRectifySnapshot,
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

    // 与 CCH 对齐：仅在包含 budget_tokens + thinking + 1024 约束时触发
    let has_budget_tokens_reference =
        lower.contains("budget_tokens") || lower.contains("budget tokens");
    let has_thinking_reference = lower.contains("thinking");
    let has_1024_constraint = lower.contains("greater than or equal to 1024")
        || lower.contains(">= 1024")
        || (lower.contains("1024") && lower.contains("input should be"));
    if has_budget_tokens_reference && has_thinking_reference && has_1024_constraint {
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
    let before = snapshot_budget(body);

    // 与 CCH 对齐：adaptive 请求不改写
    if before.thinking_type.as_deref() == Some("adaptive") {
        return BudgetRectifyResult {
            applied: false,
            before: before.clone(),
            after: before,
        };
    }

    // 与 CCH 对齐：缺少/非法 thinking 时自动创建后再整流
    if !body.get("thinking").is_some_and(Value::is_object) {
        body["thinking"] = Value::Object(serde_json::Map::new());
    }

    let Some(thinking) = body.get_mut("thinking").and_then(|t| t.as_object_mut()) else {
        return BudgetRectifyResult {
            applied: false,
            before: before.clone(),
            after: before,
        };
    };

    thinking.insert("type".to_string(), Value::String("enabled".to_string()));
    thinking.insert(
        "budget_tokens".to_string(),
        Value::Number(MAX_THINKING_BUDGET.into()),
    );

    if before.max_tokens.is_none() || before.max_tokens < Some(MIN_MAX_TOKENS_FOR_BUDGET) {
        body["max_tokens"] = Value::Number(MAX_TOKENS_VALUE.into());
    }

    let after = snapshot_budget(body);
    BudgetRectifyResult {
        applied: before != after,
        before,
        after,
    }
}

/// 检测上游是否拒绝关闭思考
///
/// 有的上游模型思考常开，收到 `thinking.type = "disabled"` 直接报 400。
/// Claude Code 的 auto mode 分类器请求固定带 disabled（Issue #8887），
/// 例如 MiniMax-M3.1-Flash 返回：
/// `model "MiniMax-M3.1-Flash" requires adaptive thinking; thinking.type="disabled" ... is not allowed (2013)`。
/// 与 budget 整流共用 `request_thinking_budget` 子开关。
pub fn should_rectify_thinking_disabled(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> bool {
    if !config.enabled || !config.request_thinking_budget {
        return false;
    }
    let Some(msg) = error_message else {
        return false;
    };
    let lower = msg.to_lowercase();
    lower.contains("thinking")
        && lower.contains("disabled")
        && (lower.contains("not allowed") || lower.contains("requires adaptive thinking"))
}

/// 对请求体执行关思考整流：删掉 `thinking.type = "disabled"`，让上游按默认思考
///
/// 只删 disabled，其它取值不动（不是这个报错的成因，删了也不会好）。
pub fn rectify_thinking_disabled(body: &mut Value) -> BudgetRectifyResult {
    let before = snapshot_budget(body);
    if before.thinking_type.as_deref() == Some("disabled") {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("thinking");
        }
    }
    let after = snapshot_budget(body);
    BudgetRectifyResult {
        applied: before != after,
        before,
        after,
    }
}

fn snapshot_budget(body: &Value) -> BudgetRectifySnapshot {
    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64());
    let thinking = body.get("thinking").and_then(|t| t.as_object());
    let thinking_type = thinking
        .and_then(|t| t.get("type"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let thinking_budget_tokens = thinking
        .and_then(|t| t.get("budget_tokens"))
        .and_then(|v| v.as_u64());
    BudgetRectifySnapshot {
        max_tokens,
        thinking_type,
        thinking_budget_tokens,
    }
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
            request_media_fallback: true,
            request_media_heuristic: true,
        }
    }

    #[test]
    fn should_rectify_thinking_budget_rules() {
        let cfg = enabled_config();
        for (case, msg, expected) in [
            (
                "budget_tokens_gte_1024",
                Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
                true,
            ),
            (
                "thinking_budget_tokens_symbol_1024",
                Some("thinking budget_tokens must be >= 1024"),
                true,
            ),
            (
                "max_tokens_constraint_ignored",
                Some("budget_tokens must be less than max_tokens"),
                false,
            ),
            (
                "missing_thinking_keyword_ignored",
                Some("budget_tokens: value must be at least 1024"),
                false,
            ),
            ("unrelated_error", Some("Request timeout"), false),
            ("none_message", None, false),
        ] {
            assert_eq!(
                should_rectify_thinking_budget(msg, &cfg),
                expected,
                "{case}"
            );
        }

        let trigger = Some("thinking.budget_tokens: Input should be greater than or equal to 1024");
        assert!(!should_rectify_thinking_budget(
            trigger,
            &RectifierConfig {
                request_thinking_budget: false,
                ..cfg.clone()
            }
        ));
        assert!(!should_rectify_thinking_budget(
            trigger,
            &RectifierConfig {
                enabled: false,
                ..cfg
            }
        ));
    }

    #[test]
    fn rectify_thinking_budget_cases() {
        for (case, mut body, expected_applied, expected_type, expected_budget, expected_max) in [
            (
                "basic_raise_budget_and_max_tokens",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "enabled", "budget_tokens": 512 },
                    "max_tokens": 1024
                }),
                true,
                "enabled",
                MAX_THINKING_BUDGET,
                MAX_TOKENS_VALUE,
            ),
            (
                "skips_adaptive",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "adaptive", "budget_tokens": 512 },
                    "max_tokens": 1024
                }),
                false,
                "adaptive",
                512,
                1024,
            ),
            (
                "preserves_large_max_tokens",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "enabled", "budget_tokens": 512 },
                    "max_tokens": 100000
                }),
                true,
                "enabled",
                MAX_THINKING_BUDGET,
                100000,
            ),
            (
                "creates_missing_thinking_object",
                json!({ "model": "claude-test", "max_tokens": 1024 }),
                true,
                "enabled",
                MAX_THINKING_BUDGET,
                MAX_TOKENS_VALUE,
            ),
            (
                "fills_missing_max_tokens",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "enabled", "budget_tokens": 512 }
                }),
                true,
                "enabled",
                MAX_THINKING_BUDGET,
                MAX_TOKENS_VALUE,
            ),
            (
                "normalizes_disabled_type_to_enabled",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "disabled", "budget_tokens": 512 },
                    "max_tokens": 1024
                }),
                true,
                "enabled",
                MAX_THINKING_BUDGET,
                MAX_TOKENS_VALUE,
            ),
            (
                "noop_when_already_valid",
                json!({
                    "model": "claude-test",
                    "thinking": { "type": "enabled", "budget_tokens": 32000 },
                    "max_tokens": 64001
                }),
                false,
                "enabled",
                32000,
                64001,
            ),
        ] {
            let result = rectify_thinking_budget(&mut body);
            assert_eq!(result.applied, expected_applied, "{case}");
            assert_eq!(result.before == result.after, !expected_applied, "{case}");
            assert_eq!(
                result.after.thinking_type.as_deref(),
                Some(expected_type),
                "{case}"
            );
            assert_eq!(
                result.after.thinking_budget_tokens,
                Some(expected_budget),
                "{case}"
            );
            assert_eq!(result.after.max_tokens, Some(expected_max), "{case}");
            assert_eq!(body["thinking"]["type"], expected_type, "{case}");
            assert_eq!(body["thinking"]["budget_tokens"], expected_budget, "{case}");
            assert_eq!(body["max_tokens"], expected_max, "{case}");
        }
    }

    // Issue #8887: MiniMax-M3.1-Flash 等常开思考模型拒绝 thinking.type="disabled"
    const MINIMAX_DISABLED_ERROR: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"invalid params, model \"MiniMax-M3.1-Flash\" requires adaptive thinking; thinking.type=\"disabled\" (including reasoning.effort=none) is not allowed (2013)"},"request_id":"072e005c15d09825a8667dba1a975010"}"#;

    #[test]
    fn should_rectify_thinking_disabled_rules() {
        let cfg = enabled_config();
        assert!(should_rectify_thinking_disabled(
            Some(MINIMAX_DISABLED_ERROR),
            &cfg
        ));
        // 不该被 budget 整流误领
        assert!(!should_rectify_thinking_budget(
            Some(MINIMAX_DISABLED_ERROR),
            &cfg
        ));

        for msg in [
            Some("Invalid `signature` in `thinking` block"),
            Some("thinking is disabled for this account"),
            None,
        ] {
            assert!(!should_rectify_thinking_disabled(msg, &cfg), "{msg:?}");
        }

        assert!(!should_rectify_thinking_disabled(
            Some(MINIMAX_DISABLED_ERROR),
            &RectifierConfig {
                request_thinking_budget: false,
                ..cfg.clone()
            }
        ));
        assert!(!should_rectify_thinking_disabled(
            Some(MINIMAX_DISABLED_ERROR),
            &RectifierConfig {
                enabled: false,
                ..cfg
            }
        ));
    }

    #[test]
    fn rectify_thinking_disabled_removes_only_disabled_type() {
        // Issue #8887: 删掉 disabled 保留其他字段
        let mut disabled_body = json!({
            "model": "MiniMax-M3.1-Flash",
            "max_tokens": 64,
            "thinking": { "type": "disabled" },
            "stop_sequences": ["</block>"]
        });
        let result = rectify_thinking_disabled(&mut disabled_body);
        assert!(result.applied);
        assert_eq!(result.before.thinking_type.as_deref(), Some("disabled"));
        assert_eq!(result.after.thinking_type, None);
        assert!(disabled_body.get("thinking").is_none());
        assert_eq!(disabled_body["max_tokens"], 64);
        assert_eq!(disabled_body["stop_sequences"], json!(["</block>"]));

        // 非 disabled 类型不动
        let mut adaptive_body = json!({
            "model": "MiniMax-M3.1-Flash",
            "max_tokens": 64,
            "thinking": { "type": "adaptive" }
        });
        let result = rectify_thinking_disabled(&mut adaptive_body);
        assert!(!result.applied);
        assert_eq!(adaptive_body["thinking"]["type"], "adaptive");
    }
}
