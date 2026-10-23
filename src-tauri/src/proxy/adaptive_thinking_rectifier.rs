//! Adaptive Thinking 整流器
//!
//! 新一代 Claude 模型（如 `claude-opus-5-5`、`claude-sonnet-5-5`）始终开启 adaptive
//! thinking：不接受 `thinking: {"type": "disabled"}`，也不接受强制 `tool_choice`。
//!
//! 客户端按自己看到的模型名组装请求。当代理把请求映射到这类模型（例如
//! `claude-opus-5` → `claude-opus-5-5`）时，Claude Code 仍会按原模型发送
//! `thinking: disabled` + 强制 `tool_choice`（WebSearch 辅助请求就是这个形态），
//! 上游因此返回 400：
//!
//! - `claude-opus-5-5 requires adaptive thinking; omit thinking or use thinking.type=adaptive and output_config.effort`
//! - `claude-sonnet-5-5 requires adaptive thinking or thinking.type=between_tools; omit thinking or use one of those modes`
//! - `claude-opus-5-5 does not support forced tool_choice; use auto or none`
//!
//! 整流动作按上游报错的指引，把请求改成 Claude Code 对这类模型原生发送的形态：
//! 省略 `thinking`，并把强制 `tool_choice` 放宽为 `auto`，然后对同一供应商重试一次。

use super::types::RectifierConfig;
use serde_json::Value;

/// Adaptive thinking 整流结果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdaptiveThinkingRectifyResult {
    /// 是否应用了整流
    pub applied: bool,
    /// 是否移除了 `thinking: {"type": "disabled"}`
    pub removed_disabled_thinking: bool,
    /// 被放宽为 `auto` 的原强制 `tool_choice` 类型（`tool` / `any`）
    pub relaxed_tool_choice: Option<String>,
}

/// 检测是否需要触发 adaptive thinking 整流器
///
/// 只匹配上游明确给出修正指引的报错，避免误捕无关的 400。
pub fn should_rectify_adaptive_thinking(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> bool {
    // 检查总开关
    if !config.enabled {
        return false;
    }
    // 检查子开关
    if !config.request_adaptive_thinking {
        return false;
    }

    let Some(msg) = error_message else {
        return false;
    };
    let lower = msg.to_lowercase();

    // 场景1: 模型只支持 adaptive thinking，拒绝 thinking.type=disabled
    // 错误示例: "claude-opus-5-5 requires adaptive thinking; omit thinking or use thinking.type=adaptive ..."
    if lower.contains("requires adaptive thinking")
        || (lower.contains("omit thinking") && lower.contains("adaptive"))
    {
        return true;
    }

    // 场景2: thinking 常开的模型拒绝强制 tool_choice
    // 错误示例: "claude-opus-5-5 does not support forced tool_choice; use auto or none"
    if lower.contains("forced tool_choice") && lower.contains("not support") {
        return true;
    }

    false
}

/// 对请求体执行 adaptive thinking 整流
///
/// 整流动作：
/// - 移除顶层 `thinking: {"type": "disabled"}`（其他 thinking 形状保持不变）
/// - 强制 `tool_choice`（`tool` / `any`）改为 `auto`，保留 `disable_parallel_tool_use`
///
/// 这类模型始终开启 thinking，而 thinking 开启时 API 不接受强制 tool_choice，
/// 所以两项要在同一次重试里一起整流。
pub fn rectify_adaptive_thinking(body: &mut Value) -> AdaptiveThinkingRectifyResult {
    let mut result = AdaptiveThinkingRectifyResult::default();
    let Some(obj) = body.as_object_mut() else {
        return result;
    };

    let thinking_disabled = obj
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
        == Some("disabled");
    if thinking_disabled {
        obj.remove("thinking");
        result.removed_disabled_thinking = true;
    }

    if let Some(tool_choice) = obj.get_mut("tool_choice").and_then(Value::as_object_mut) {
        let forced = tool_choice
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| matches!(*kind, "tool" | "any"))
            .map(ToString::to_string);
        if let Some(kind) = forced {
            tool_choice.retain(|key, _| key == "disable_parallel_tool_use");
            tool_choice.insert("type".to_string(), Value::String("auto".to_string()));
            result.relaxed_tool_choice = Some(kind);
        }
    }

    result.applied = result.removed_disabled_thinking || result.relaxed_tool_choice.is_some();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OPUS_ERROR: &str = r#"{"error":{"message":"claude-opus-5-5 requires adaptive thinking; omit thinking or use thinking.type=adaptive and output_config.effort","type":"invalid_request_error"},"type":"error"}"#;
    const SONNET_ERROR: &str = r#"{"error":{"message":"claude-sonnet-5-5 requires adaptive thinking or thinking.type=between_tools; omit thinking or use one of those modes","type":"invalid_request_error"},"type":"error"}"#;
    const FORCED_TOOL_CHOICE_ERROR: &str = r#"{"error":{"message":"claude-opus-5-5 does not support forced tool_choice; use auto or none","type":"invalid_request_error"},"type":"error"}"#;

    fn enabled_config() -> RectifierConfig {
        RectifierConfig::default()
    }

    /// Claude Code WebSearch 辅助请求在映射到 opus-5-5 之前的实际形态
    fn websearch_side_query() -> Value {
        json!({
            "model": "claude-opus-5-5",
            "max_tokens": 32000,
            "thinking": {"type": "disabled"},
            "output_config": {"effort": "high"},
            "tool_choice": {"type": "tool", "name": "web_search"},
            "tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 8}],
            "messages": [{"role": "user", "content": "Perform a web search for the query: cc-switch"}],
            "stream": true
        })
    }

    #[test]
    fn detects_adaptive_only_errors() {
        let config = enabled_config();
        assert!(should_rectify_adaptive_thinking(Some(OPUS_ERROR), &config));
        assert!(should_rectify_adaptive_thinking(
            Some(SONNET_ERROR),
            &config
        ));
        assert!(should_rectify_adaptive_thinking(
            Some(FORCED_TOOL_CHOICE_ERROR),
            &config
        ));
    }

    #[test]
    fn ignores_unrelated_thinking_errors() {
        let config = enabled_config();
        // DeepSeek 的 disabled 与 effort 冲突由 providers::claude 预先处理，这里不接手
        assert!(!should_rectify_adaptive_thinking(
            Some("thinking options type cannot be disabled when reasoning_effort is set"),
            &config
        ));
        assert!(!should_rectify_adaptive_thinking(
            Some("Invalid `signature` in `thinking` block"),
            &config
        ));
        assert!(!should_rectify_adaptive_thinking(
            Some("thinking.budget_tokens: Input should be greater than or equal to 1024"),
            &config
        ));
        assert!(!should_rectify_adaptive_thinking(None, &config));
    }

    #[test]
    fn respects_switches() {
        let sub_switch_off = RectifierConfig {
            request_adaptive_thinking: false,
            ..RectifierConfig::default()
        };
        assert!(!should_rectify_adaptive_thinking(
            Some(OPUS_ERROR),
            &sub_switch_off
        ));

        let master_off = RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        };
        assert!(!should_rectify_adaptive_thinking(
            Some(OPUS_ERROR),
            &master_off
        ));
    }

    #[test]
    fn rectifies_websearch_side_query_to_native_adaptive_shape() {
        let mut body = websearch_side_query();

        let result = rectify_adaptive_thinking(&mut body);

        assert!(result.applied);
        assert!(result.removed_disabled_thinking);
        assert_eq!(result.relaxed_tool_choice.as_deref(), Some("tool"));
        assert!(body.get("thinking").is_none());
        assert_eq!(body["tool_choice"], json!({"type": "auto"}));
        // 其余字段原样保留
        assert_eq!(body["output_config"], json!({"effort": "high"}));
        assert_eq!(body["tools"][0]["type"], "web_search_20250305");
        assert_eq!(body["model"], "claude-opus-5-5");
    }

    #[test]
    fn relaxes_forced_any_and_keeps_parallel_flag() {
        let mut body = json!({
            "model": "claude-opus-5-5",
            "tool_choice": {"type": "any", "disable_parallel_tool_use": true},
            "messages": []
        });

        let result = rectify_adaptive_thinking(&mut body);

        assert!(result.applied);
        assert!(!result.removed_disabled_thinking);
        assert_eq!(result.relaxed_tool_choice.as_deref(), Some("any"));
        assert_eq!(
            body["tool_choice"],
            json!({"type": "auto", "disable_parallel_tool_use": true})
        );
    }

    #[test]
    fn leaves_valid_adaptive_requests_untouched() {
        let mut body = json!({
            "model": "claude-opus-5-5",
            "thinking": {"type": "adaptive", "display": "omitted"},
            "tool_choice": {"type": "auto"},
            "messages": []
        });
        let original = body.clone();

        let result = rectify_adaptive_thinking(&mut body);

        assert!(!result.applied);
        assert_eq!(body, original);
    }

    #[test]
    fn does_not_rewrite_other_thinking_shapes() {
        let mut body = json!({
            "model": "claude-opus-5-5",
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": []
        });
        let original = body.clone();

        let result = rectify_adaptive_thinking(&mut body);

        assert!(!result.applied);
        assert_eq!(body, original);
    }
}
