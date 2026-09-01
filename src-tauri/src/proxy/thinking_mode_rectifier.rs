//! Thinking 形状整流器（双向）
//!
//! 当上游拒绝我们发出的 thinking 形状时，从报错里学出正确形状、写进
//! [`ThinkingCapabilityStore`]，然后重试。两个方向都支持：
//!
//! - [`ThinkingModeRectifyDirection::ToAdaptive`] —— 上游点名 `thinking.type.enabled`
//!   不支持（Opus 5 / 4.8 / 4.7、Sonnet 5、Fable / Mythos）
//! - [`ThinkingModeRectifyDirection::ToLegacy`] —— 上游不认 `adaptive`（旧模型、
//!   老版本第三方网关）
//!
//! 与其它整流器不同，这里**不改请求体**。原因是 Codex→Anthropic 桥接链路上，
//! 失败循环持有的是客户端原始的 Responses 请求体，Anthropic 请求体是在 `forward()`
//! 内部转换出来、用完即弃的。所以整流动作只是把结论写进 store，重试时 `forward()`
//! 重新走一遍转换，发送端查到学到的形状就自然产出正确结果 —— Claude 直通与 Codex
//! 桥接两条链路因此共用同一套机制。

use super::thinking_capability::{ThinkingCapabilityStore, ThinkingMode};
use super::types::RectifierConfig;
use serde_json::{json, Value};

/// 整流方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingModeRectifyDirection {
    /// 改用 `thinking: {"type":"adaptive"}` + `output_config.effort`
    ToAdaptive,
    /// 改用 `thinking: {"type":"enabled","budget_tokens":N}`
    ToLegacy,
}

impl ThinkingModeRectifyDirection {
    fn target_mode(self) -> ThinkingMode {
        match self {
            Self::ToAdaptive => ThinkingMode::Adaptive,
            Self::ToLegacy => ThinkingMode::Legacy,
        }
    }
}

/// 整流结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThinkingModeRectifyResult {
    /// 是否真的改变了解析结论（false 表示已经是目标形状，重试没有意义）
    pub applied: bool,
    /// 整流前解析出的形状
    pub before: Option<ThinkingMode>,
    /// 整流后的形状
    pub after: Option<ThinkingMode>,
}

/// 上游是否在拒绝 `enabled` 形状
fn rejects_enabled(lower: &str) -> bool {
    // 上游明确点名 enabled 形状
    if (lower.contains("thinking.type.enabled") || lower.contains("thinking type enabled"))
        && is_rejection(lower)
    {
        return true;
    }
    // 点名 budget_tokens 不受支持。注意与 budget 整流器区分：那个处理的是
    // ">= 1024" 之类的取值约束，这里处理的是「整个参数不再支持」。
    if lower.contains("budget_tokens") && is_rejection(lower) {
        return true;
    }
    // 兜底：同时点名 adaptive 与 output_config，是典型的「请改用新形状」引导语
    lower.contains("adaptive") && lower.contains("output_config")
}

/// 上游是否在拒绝 `adaptive` 形状
///
/// 只在 [`rejects_enabled`] 不成立时才被询问 —— Anthropic 拒绝 enabled 的报文里
/// 同时含有 "adaptive"（作为建议用法），先判 enabled 方向才能避免误判。
fn rejects_adaptive(lower: &str) -> bool {
    // `Input tag 'adaptive' found using 'type' does not match expected tags: ...`
    // 这条措辞本身已足够特异，不额外要求点名 thinking（报文里确实没有那个词）。
    if lower.contains("adaptive") && lower.contains("does not match expected tags") {
        return true;
    }
    // 泛化措辞（"not supported" / "not permitted" 等）单靠 "adaptive" 一个词太宽，
    // 会把无关 400 拖进整流器、进而挡掉正常的故障转移。额外要求报文点名 thinking
    // 相关字段，才认定是在拒绝 adaptive 形状。
    //
    // 老网关完全不认 output_config 也走这里（output_config 本身就是 thinking 相关字段）：
    // adaptive + effort 这条路走不通，退回 legacy。
    (lower.contains("adaptive") || lower.contains("output_config"))
        && is_rejection(lower)
        && mentions_thinking(lower)
}

/// 报文是否点名了 thinking 相关字段
fn mentions_thinking(lower: &str) -> bool {
    lower.contains("thinking") || lower.contains("output_config") || lower.contains("budget_tokens")
}

/// 报错措辞是否表示「参数不被接受」
///
/// 有意不含 "invalid" —— 那个词过宽，且已由 thinking 签名整流器覆盖。
fn is_rejection(lower: &str) -> bool {
    lower.contains("not supported")
        || lower.contains("unsupported")
        || lower.contains("not permitted")
        || lower.contains("unexpected value")
        || lower.contains("unrecognized")
}

/// 检测是否需要触发 thinking 形状整流器，并给出方向。
///
/// 返回 `None` 表示不触发。会检查配置开关。
pub fn detect_thinking_mode_error(
    error_message: Option<&str>,
    config: &RectifierConfig,
) -> Option<ThinkingModeRectifyDirection> {
    // 检查总开关
    if !config.enabled {
        return None;
    }
    // 检查子开关
    if !config.request_thinking_mode {
        return None;
    }

    detect_direction(error_message?)
}

/// 报文是否在拒绝某个 thinking 形状（不查配置开关）。
///
/// 给别的整流器做优先级判断用：形状问题必须由本模块处理 —— 其它整流器改的东西
/// 不影响形状，重试只会拿回同一个 400，反而把学习机会吃掉。
pub fn looks_like_thinking_mode_error(error_message: &str) -> bool {
    detect_direction(error_message).is_some()
}

fn detect_direction(error_message: &str) -> Option<ThinkingModeRectifyDirection> {
    let lower = error_message.to_lowercase();

    // 顺序有意义：拒绝 enabled 的报文里通常也含 "adaptive"（建议用法），
    // 必须先判这个方向。
    if rejects_enabled(&lower) {
        return Some(ThinkingModeRectifyDirection::ToAdaptive);
    }
    if rejects_adaptive(&lower) {
        return Some(ThinkingModeRectifyDirection::ToLegacy);
    }
    None
}

/// Anthropic 直通链路的发送端归一化：把请求体里的 thinking 形状改写成 `mode`。
///
/// 用于客户端自己发 thinking 的场景 —— 例如 Claude Code 对着 Opus 5 发
/// `{"type":"enabled","budget_tokens":N}`。只在「Anthropic 格式请求体发往 Anthropic
/// 上游」时调用；Codex 桥接链路由 transform 自己按 capability 产出，不走这里。
///
/// `disabled` 与缺失 thinking 都不动 —— 那是调用方的明确意图，不是形状问题。
/// 改写形状的同时会同步与形状绑定的 beta 标识，见 [`sync_interleaved_thinking_beta`]。
/// 返回是否改写了请求体。
pub fn normalize_thinking_shape(body: &mut Value, mode: ThinkingMode) -> bool {
    let Some(thinking_type) = body
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
    else {
        return false;
    };

    match (thinking_type, mode) {
        ("enabled", ThinkingMode::Adaptive) => {
            // 把 budget 折算成 effort，不丢调用方的思考深度意图
            let effort = body
                .pointer("/thinking/budget_tokens")
                .and_then(Value::as_u64)
                .map(budget_to_effort);
            body["thinking"] = json!({ "type": "adaptive" });
            if let Some(effort) = effort {
                set_effort(body, effort);
            }
            sync_interleaved_thinking_beta(body, false);
            true
        }
        ("adaptive", ThinkingMode::Legacy) => {
            let effort = body
                .pointer("/output_config/effort")
                .and_then(Value::as_str)
                .unwrap_or("high");
            let max_tokens = body.get("max_tokens").and_then(Value::as_u64);
            let legacy_thinking = match legacy_budget(effort, max_tokens) {
                Some(budget) => {
                    body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
                    true
                }
                None => {
                    // 挤不出 Anthropic 要求的 1024 下限，发 thinking 只会 400。
                    // 干脆不发，让上游按无思考处理。
                    //
                    // 这是唯一一处「静默丢掉调用方明确要的思考」的路径，且丢掉之后请求
                    // 通常会成功 —— 用户只会觉得模型忽然不思考了，没有别的线索。留一条
                    // 告警让它可诊断。
                    log::warn!(
                        "[RECT-025] adaptive→legacy: max_tokens={} 挤不出 Anthropic 要求的 1024 budget 下限，本次按无思考发送（effort={effort} 一并丢弃）",
                        max_tokens
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| "缺失".to_string())
                    );
                    if let Some(obj) = body.as_object_mut() {
                        obj.remove("thinking");
                    }
                    false
                }
            };
            remove_effort(body);
            sync_interleaved_thinking_beta(body, legacy_thinking);
            true
        }
        _ => false,
    }
}

/// 与 legacy thinking 形状绑定的 beta 标识。
///
/// 只在 `thinking: {"type":"enabled"}` 下有意义 —— adaptive 的交错思考是默认行为。
/// 与 [`super::thinking_optimizer`] legacy 分支注入的标识保持一致。
const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";

/// 改写形状后同步 `anthropic_beta` 里与形状绑定的标识。
///
/// Bedrock 的 PRE-SEND 优化器按形状注入 beta（adaptive 分支 `context-1m-2025-08-07`、
/// legacy 分支 `interleaved-thinking-2025-05-14`），但它在 `forward()` 之外就把请求体
/// 定型了，整流重试不会重算 —— 形状被改写后 beta 数组会与形状脱节。
///
/// 两条边界有意为之：
/// - **绝不新建 `anthropic_beta` 字段**。它是 Bedrock 的 body 级参数；Anthropic 直通链路
///   走 `anthropic-beta` 请求头，往 body 里塞这个字段会被上游以「extra inputs are not
///   permitted」拒掉。字段不存在就说明不是那条链路，直接返回。
/// - **不动 `context-1m-2025-08-07`**。那是上下文窗口的 beta，与 thinking 形状无关
///   （Sonnet 4.5 这类 legacy 形状的模型同样支持 1M 上下文）。它对当前模型合不合法在
///   形状改写前后是同一个答案；这里贸然摘掉，长上下文请求会以另一种方式 400。
fn sync_interleaved_thinking_beta(body: &mut Value, legacy_thinking: bool) {
    let Some(betas) = body.get_mut("anthropic_beta").and_then(Value::as_array_mut) else {
        return;
    };

    let present = betas
        .iter()
        .any(|beta| beta.as_str() == Some(INTERLEAVED_THINKING_BETA));

    match (legacy_thinking, present) {
        (true, false) => betas.push(json!(INTERLEAVED_THINKING_BETA)),
        (false, true) => betas.retain(|beta| beta.as_str() != Some(INTERLEAVED_THINKING_BETA)),
        _ => {}
    }
}

/// budget_tokens → effort 档位（与 Codex 侧 `effort_to_thinking_budget` 的分档对称）
fn budget_to_effort(budget: u64) -> &'static str {
    match budget {
        0..=2048 => "low",
        2049..=8192 => "medium",
        8193..=16384 => "high",
        _ => "max",
    }
}

/// effort → budget_tokens，并为可见回答留出余量。
///
/// 返回 `None` 表示挤不出 Anthropic 的 1024 下限，此时不该发 thinking。
fn legacy_budget(effort: &str, max_tokens: Option<u64>) -> Option<u64> {
    let desired = match effort.trim().to_ascii_lowercase().as_str() {
        "low" | "minimal" => 2048,
        "medium" => 8192,
        "xhigh" => 24576,
        "max" => 32000,
        // 含 "high" 与任何未知档位
        _ => 16384,
    };
    // 与 Codex 侧一致：thinking 最多吃掉一半 max_tokens，避免可见回答被挤空
    let budget = match max_tokens {
        Some(max) => desired.min(max / 2),
        None => desired,
    };
    (budget >= 1024).then_some(budget)
}

fn set_effort(body: &mut Value, effort: &str) {
    match body.get_mut("output_config").and_then(Value::as_object_mut) {
        Some(output_config) => {
            output_config.insert("effort".to_string(), json!(effort));
        }
        None => body["output_config"] = json!({ "effort": effort }),
    }
}

/// 移除 `output_config.effort`，并在 output_config 变空时一并清理。
///
/// 与 `claude.rs` 里 DeepSeek 的同类处理保持一致：只动 effort，不碰其它字段。
fn remove_effort(body: &mut Value) {
    let Some(output_config) = body.get_mut("output_config").and_then(Value::as_object_mut) else {
        return;
    };
    output_config.remove("effort");
    if output_config.is_empty() {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("output_config");
        }
    }
}

/// 从请求体里取客户端原始模型名（store 的键之一）。
///
/// Codex Responses 与 Anthropic Messages 两种请求体都有顶层 `model`。
/// 字段缺失或不是字符串时返回 `None`（第三方客户端什么都可能发）。
pub fn client_model_from_body(body: &Value) -> Option<&str> {
    body.get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
}

/// 执行形状整流：把学到的形状写进 store。
///
/// 不修改请求体 —— 重试时由发送端重新查 store 产出正确形状。
///
/// `client_model` 是缓存键（失败重试循环只拿得到客户端原始请求体）；
/// `upstream_model` 是真正发往上游的模型名。两者必须与发送端
/// [`ThinkingCapabilityStore::resolve_mapped`] 的取值一致 —— 否则在模型映射
/// （`apply_model_mapping` / `apply_codex_upstream_model`）生效时双方会算出不同的
/// `before`：发送端按上游模型判 Adaptive 发出去被拒，整流器却按客户端模型判成
/// Legacy，得出「已经是目标形状」的结论，于是永不 `learn()`，自愈死循环在同一个
/// 被拒形状上打转。`upstream_model` 为空时回落到 `client_model`。
pub fn rectify_thinking_mode(
    store: &ThinkingCapabilityStore,
    provider_id: &str,
    client_model: &str,
    upstream_model: &str,
    direction: ThinkingModeRectifyDirection,
) -> ThinkingModeRectifyResult {
    let before = store
        .resolve_mapped(provider_id, client_model, upstream_model)
        .mode;
    let target = direction.target_mode();

    if before == target {
        // 已经是目标形状，说明这次 400 不是形状问题，不做无意义重试
        return ThinkingModeRectifyResult {
            applied: false,
            before: Some(before),
            after: Some(before),
        };
    }

    store.learn(provider_id, client_model, target);

    ThinkingModeRectifyResult {
        applied: true,
        before: Some(before),
        after: Some(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_config() -> RectifierConfig {
        RectifierConfig::default()
    }

    fn mode_disabled_config() -> RectifierConfig {
        RectifierConfig {
            request_thinking_mode: false,
            ..RectifierConfig::default()
        }
    }

    fn master_disabled_config() -> RectifierConfig {
        RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        }
    }

    // ==================== 检测：legacy → adaptive ====================

    #[test]
    fn detects_the_reported_opus_5_error() {
        // 用户实际报的报文
        let msg = r#""thinking.type.enabled" is not supported for this model. Use "thinking.type.adaptive" and "output_config.effort" to control thinking behavior."#;
        assert_eq!(
            detect_thinking_mode_error(Some(msg), &enabled_config()),
            Some(ThinkingModeRectifyDirection::ToAdaptive)
        );
    }

    #[test]
    fn detects_enabled_rejection_in_nested_json_body() {
        // 第三方渠道常见的嵌套 JSON 错误体
        let nested = r#"{"error":{"message":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"\\\"thinking.type.enabled\\\" is not supported for this model. Use \\\"thinking.type.adaptive\\\" and \\\"output_config.effort\\\"\"},\"request_id\":\"req_xxx\"}"}}"#;
        assert_eq!(
            detect_thinking_mode_error(Some(nested), &enabled_config()),
            Some(ThinkingModeRectifyDirection::ToAdaptive)
        );
    }

    #[test]
    fn detects_budget_tokens_no_longer_supported() {
        assert_eq!(
            detect_thinking_mode_error(
                Some("thinking.budget_tokens: Extra inputs are not permitted"),
                &enabled_config()
            ),
            Some(ThinkingModeRectifyDirection::ToAdaptive)
        );
        assert_eq!(
            detect_thinking_mode_error(
                Some("budget_tokens is not supported for this model"),
                &enabled_config()
            ),
            Some(ThinkingModeRectifyDirection::ToAdaptive)
        );
    }

    // ==================== 检测：adaptive → legacy ====================

    #[test]
    fn detects_adaptive_tag_mismatch() {
        // thinking_rectifier 明确不接这条（见其 test_do_not_detect_thinking_type_tag_mismatch），
        // 正是留给本整流器的信号
        let msg = "Input tag 'adaptive' found using 'type' does not match expected tags: 'enabled', 'disabled'";
        assert_eq!(
            detect_thinking_mode_error(Some(msg), &enabled_config()),
            Some(ThinkingModeRectifyDirection::ToLegacy)
        );
    }

    #[test]
    fn detects_adaptive_rejected_as_extra_input() {
        assert_eq!(
            detect_thinking_mode_error(
                Some("thinking.adaptive: Extra inputs are not permitted"),
                &enabled_config()
            ),
            Some(ThinkingModeRectifyDirection::ToLegacy)
        );
    }

    #[test]
    fn detects_gateway_that_rejects_output_config() {
        assert_eq!(
            detect_thinking_mode_error(
                Some("output_config: Extra inputs are not permitted"),
                &enabled_config()
            ),
            Some(ThinkingModeRectifyDirection::ToLegacy)
        );
    }

    // ==================== 方向优先级 ====================

    #[test]
    fn enabled_rejection_wins_over_the_adaptive_mention_in_the_same_message() {
        // Anthropic 拒绝 enabled 的报文里也含 "adaptive"（作为建议用法）。
        // 判成 ToLegacy 会让整流器把请求推向更错的方向。
        let msg = r#""thinking.type.enabled" is not supported for this model. Use "thinking.type.adaptive" and "output_config.effort"."#;
        assert_ne!(
            detect_thinking_mode_error(Some(msg), &enabled_config()),
            Some(ThinkingModeRectifyDirection::ToLegacy)
        );
    }

    #[test]
    fn tag_mismatch_message_mentioning_enabled_still_goes_to_legacy() {
        // 这条报文含 'enabled'（作为期望值之一），不能被误判成 ToAdaptive
        let msg = "Input tag 'adaptive' found using 'type' does not match expected tags: 'enabled', 'disabled'";
        assert_eq!(
            detect_thinking_mode_error(Some(msg), &enabled_config()),
            Some(ThinkingModeRectifyDirection::ToLegacy)
        );
    }

    // ==================== 不触发 ====================

    #[test]
    fn does_not_trigger_on_errors_that_merely_mention_adaptive_or_output_config() {
        // 泛化措辞 + "adaptive" / "output_config" 但没点名 thinking 相关字段的报文，
        // 必须留在正常的错误分类与故障转移路径上，不能被整流器吃掉
        for msg in [
            "model 'adaptive-router-v2' is not supported on this endpoint",
            "adaptive rate limiting is not permitted for this key",
            "unrecognized parameter: adaptive_batching",
            "The requested model adaptive-mix is unsupported",
        ] {
            assert_eq!(
                detect_thinking_mode_error(Some(msg), &enabled_config()),
                None,
                "msg={msg}"
            );
        }
    }

    #[test]
    fn still_triggers_when_a_generic_rejection_names_a_thinking_field() {
        for msg in [
            "thinking.type: adaptive is not supported by this endpoint",
            "output_config.effort: Extra inputs are not permitted",
            "thinking.type: unexpected value 'adaptive'",
        ] {
            assert_eq!(
                detect_thinking_mode_error(Some(msg), &enabled_config()),
                Some(ThinkingModeRectifyDirection::ToLegacy),
                "msg={msg}"
            );
        }
    }

    #[test]
    fn does_not_trigger_on_unrelated_errors() {
        for msg in [
            "Request timeout",
            "Connection refused",
            "rate_limit_error: too many requests",
            "messages.1.content.0: Invalid `signature` in `thinking` block",
            "thinking.budget_tokens: Input should be greater than or equal to 1024",
        ] {
            assert_eq!(
                detect_thinking_mode_error(Some(msg), &enabled_config()),
                None,
                "msg={msg}"
            );
        }
        assert_eq!(detect_thinking_mode_error(None, &enabled_config()), None);
    }

    #[test]
    fn respects_config_switches() {
        let msg = r#""thinking.type.enabled" is not supported for this model. Use "thinking.type.adaptive" and "output_config.effort"."#;
        assert_eq!(
            detect_thinking_mode_error(Some(msg), &mode_disabled_config()),
            None
        );
        assert_eq!(
            detect_thinking_mode_error(Some(msg), &master_disabled_config()),
            None
        );
    }

    #[test]
    fn looks_like_thinking_mode_error_ignores_config_and_covers_both_directions() {
        // 签名整流器用它做优先级判断（形状问题不该被通用 invalid request 兜底吃掉），
        // 所以这里不看配置开关，只看报文本身。
        for msg in [
            r#""thinking.type.enabled" is not supported for this model. Use "thinking.type.adaptive"."#,
            "Input tag 'adaptive' found using 'type' does not match expected tags: 'enabled'",
            "output_config: Extra inputs are not permitted",
        ] {
            assert!(looks_like_thinking_mode_error(msg), "msg={msg}");
        }
        for msg in [
            "Request timeout",
            "invalid request: malformed JSON",
            "messages.1.content.0: Invalid `signature` in `thinking` block",
        ] {
            assert!(!looks_like_thinking_mode_error(msg), "msg={msg}");
        }
    }

    // ==================== normalize_thinking_shape ====================

    #[test]
    fn rewrites_enabled_to_adaptive_and_folds_budget_into_effort() {
        // Claude Code 对着 Opus 5 发 enabled + budget_tokens 的场景
        let mut body = json!({
            "model": "claude-opus-5",
            "max_tokens": 32000,
            "thinking": { "type": "enabled", "budget_tokens": 16384 }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
        assert_eq!(body["output_config"]["effort"], "high");
    }

    #[test]
    fn budget_to_effort_covers_every_band() {
        for (budget, expected) in [
            (1024_u64, "low"),
            (2048, "low"),
            (4096, "medium"),
            (8192, "medium"),
            (12288, "high"),
            (16384, "high"),
            (24576, "max"),
            (32000, "max"),
        ] {
            let mut body = json!({
                "max_tokens": 64000,
                "thinking": { "type": "enabled", "budget_tokens": budget }
            });
            assert!(normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
            assert_eq!(body["output_config"]["effort"], expected, "budget={budget}");
        }
    }

    #[test]
    fn rewrite_to_adaptive_preserves_other_output_config_fields() {
        let mut body = json!({
            "max_tokens": 32000,
            "thinking": { "type": "enabled", "budget_tokens": 2048 },
            "output_config": { "format": { "type": "json_schema" } }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }

    #[test]
    fn rewrites_adaptive_to_enabled_using_effort() {
        let mut body = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 64000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "medium" }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8192);
        // effort 是 adaptive 专属参数，legacy 形状下要摘掉；output_config 空了就一并清理
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn rewrite_to_legacy_defaults_to_high_without_effort() {
        let mut body = json!({
            "max_tokens": 64000,
            "thinking": { "type": "adaptive" }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert_eq!(body["thinking"]["budget_tokens"], 16384);
    }

    #[test]
    fn rewrite_to_legacy_leaves_output_headroom() {
        // budget 最多吃掉一半 max_tokens，否则可见回答会被挤空
        let mut body = json!({
            "max_tokens": 8000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "max" }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert_eq!(body["thinking"]["budget_tokens"], 4000);
    }

    #[test]
    fn rewrite_to_legacy_drops_thinking_when_below_the_1024_floor() {
        // 挤不出 Anthropic 的 1024 下限时发 thinking 只会 400，不如不发
        let mut body = json!({
            "max_tokens": 1500,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "high" }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
    }

    // ==================== 形状绑定的 beta 标识 ====================

    fn betas(body: &Value) -> Vec<&str> {
        body["anthropic_beta"]
            .as_array()
            .expect("anthropic_beta 应仍是数组")
            .iter()
            .map(|beta| beta.as_str().expect("beta 应是字符串"))
            .collect()
    }

    #[test]
    fn rewrite_to_legacy_adds_the_interleaved_beta_and_keeps_the_context_beta() {
        // Bedrock 场景：PRE-SEND 优化器按 adaptive 定型了请求体（含 context-1m beta），
        // 上游拒绝 adaptive 后整流重试必须补上 legacy 形状要的 interleaved beta ——
        // 否则重试体的 beta 数组与形状脱节。context-1m 是上下文窗口的 beta，与形状无关，
        // 摘掉它会让长上下文请求以另一种方式失败，所以必须留着。
        let mut body = json!({
            "model": "anthropic.claude-opus-4-6-20250514-v1:0",
            "max_tokens": 32000,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "max" },
            "anthropic_beta": ["context-1m-2025-08-07"]
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(
            betas(&body),
            vec!["context-1m-2025-08-07", "interleaved-thinking-2025-05-14"]
        );
    }

    #[test]
    fn rewrite_to_adaptive_drops_the_legacy_only_interleaved_beta() {
        let mut body = json!({
            "model": "claude-opus-5",
            "max_tokens": 32000,
            "thinking": { "type": "enabled", "budget_tokens": 8192 },
            "anthropic_beta": ["interleaved-thinking-2025-05-14", "context-1m-2025-08-07"]
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(betas(&body), vec!["context-1m-2025-08-07"]);
    }

    #[test]
    fn beta_sync_is_idempotent_and_never_creates_the_field() {
        // 已经有 interleaved：不重复追加
        let mut body = json!({
            "max_tokens": 32000,
            "thinking": { "type": "adaptive" },
            "anthropic_beta": ["interleaved-thinking-2025-05-14"]
        });
        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert_eq!(betas(&body), vec!["interleaved-thinking-2025-05-14"]);

        // 没有 anthropic_beta 字段：一律不新建 —— 那是 Bedrock 的 body 级参数，
        // Anthropic 直通链路走请求头，body 里多这个字段会被上游拒掉。
        for mode in [ThinkingMode::Legacy, ThinkingMode::Adaptive] {
            let mut direct = json!({
                "max_tokens": 32000,
                "thinking": match mode {
                    ThinkingMode::Legacy => json!({ "type": "adaptive" }),
                    _ => json!({ "type": "enabled", "budget_tokens": 8192 }),
                }
            });
            assert!(normalize_thinking_shape(&mut direct, mode));
            assert!(
                direct.get("anthropic_beta").is_none(),
                "mode={mode:?} 不该新建 anthropic_beta"
            );
        }
    }

    #[test]
    fn dropping_thinking_at_the_floor_also_drops_the_interleaved_beta() {
        // 挤不出 1024 下限时整个 thinking 都不发了，此时 interleaved beta 更没有意义
        let mut body = json!({
            "max_tokens": 1500,
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "low" },
            "anthropic_beta": ["context-1m-2025-08-07", "interleaved-thinking-2025-05-14"]
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        assert!(body.get("thinking").is_none());
        assert_eq!(betas(&body), vec!["context-1m-2025-08-07"]);
    }

    #[test]
    fn beta_sync_tolerates_a_malformed_beta_field() {
        // 第三方客户端什么都可能发：非数组的 anthropic_beta 一律不动
        for malformed in [
            json!("interleaved-thinking-2025-05-14"),
            json!(null),
            json!(7),
        ] {
            let mut body = json!({
                "max_tokens": 32000,
                "thinking": { "type": "adaptive" },
                "anthropic_beta": malformed.clone()
            });

            assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
            assert_eq!(body["thinking"]["type"], "enabled");
            assert_eq!(body["anthropic_beta"], malformed);
        }
    }

    #[test]
    fn normalize_leaves_matching_shapes_and_explicit_intent_alone() {
        // 已经是目标形状
        let mut adaptive = json!({"thinking": {"type": "adaptive"}});
        assert!(!normalize_thinking_shape(
            &mut adaptive,
            ThinkingMode::Adaptive
        ));

        let mut legacy = json!({"thinking": {"type": "enabled", "budget_tokens": 2048}});
        assert!(!normalize_thinking_shape(&mut legacy, ThinkingMode::Legacy));

        // disabled 是调用方的明确意图，两个方向都不动
        for mode in [ThinkingMode::Adaptive, ThinkingMode::Legacy] {
            let mut disabled = json!({"thinking": {"type": "disabled"}});
            assert!(!normalize_thinking_shape(&mut disabled, mode));
            assert_eq!(disabled["thinking"]["type"], "disabled");
        }
    }

    #[test]
    fn normalize_is_a_noop_on_malformed_or_absent_thinking() {
        // 第三方客户端什么都可能发，不能 panic 也不能乱改
        for mut body in [
            json!({}),
            json!({"model": "claude-opus-5"}),
            json!({"thinking": null}),
            json!({"thinking": "adaptive"}),
            json!({"thinking": []}),
            json!({"thinking": {}}),
            json!({"thinking": {"type": 5}}),
            json!({"thinking": {"type": "unexpected"}}),
        ] {
            let before = body.clone();
            assert!(!normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
            assert_eq!(body, before);
        }
    }

    #[test]
    fn rewrite_to_adaptive_tolerates_missing_or_malformed_budget() {
        // 没有 budget_tokens 就只改形状，不硬造 effort
        let mut body = json!({"thinking": {"type": "enabled"}});
        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Adaptive));
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body.get("output_config").is_none());

        let mut malformed = json!({"thinking": {"type": "enabled", "budget_tokens": "lots"}});
        assert!(normalize_thinking_shape(
            &mut malformed,
            ThinkingMode::Adaptive
        ));
        assert_eq!(malformed["thinking"]["type"], "adaptive");
        assert!(malformed.get("output_config").is_none());
    }

    #[test]
    fn rewrite_to_legacy_tolerates_malformed_effort_and_max_tokens() {
        let mut body = json!({
            "max_tokens": "many",
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": 7 }
        });

        assert!(normalize_thinking_shape(&mut body, ThinkingMode::Legacy));
        // effort 不是字符串 → 落到 high 默认档；max_tokens 不是数字 → 不做余量裁剪
        assert_eq!(body["thinking"]["budget_tokens"], 16384);
    }

    // ==================== client_model_from_body ====================

    #[test]
    fn extracts_client_model_defensively() {
        assert_eq!(
            client_model_from_body(&json!({"model": "claude-opus-5"})),
            Some("claude-opus-5")
        );
        assert_eq!(
            client_model_from_body(&json!({"model": "  claude-opus-5  "})),
            Some("claude-opus-5")
        );
        // 缺失 / 类型不对 / 空串都要安全返回 None
        assert_eq!(client_model_from_body(&json!({})), None);
        assert_eq!(client_model_from_body(&json!({"model": 5})), None);
        assert_eq!(client_model_from_body(&json!({"model": null})), None);
        assert_eq!(client_model_from_body(&json!({"model": ""})), None);
        assert_eq!(client_model_from_body(&json!({"model": "   "})), None);
        assert_eq!(client_model_from_body(&json!([])), None);
    }

    // ==================== 整流动作 ====================

    /// 客户端模型名与上游模型名一致时的便捷包装（多数直通场景）。
    fn rectify(
        store: &ThinkingCapabilityStore,
        provider_id: &str,
        model: &str,
        direction: ThinkingModeRectifyDirection,
    ) -> ThinkingModeRectifyResult {
        rectify_thinking_mode(store, provider_id, model, model, direction)
    }

    #[test]
    fn learns_the_corrected_mode() {
        let store = ThinkingCapabilityStore::new();
        // 启发式把这个解析不出版本的 Claude 别名判成 Adaptive；上游说不行
        let result = rectify(
            &store,
            "p1",
            "claude-legacy-alias",
            ThinkingModeRectifyDirection::ToLegacy,
        );

        assert!(result.applied);
        assert_eq!(result.before, Some(ThinkingMode::Adaptive));
        assert_eq!(result.after, Some(ThinkingMode::Legacy));
        assert_eq!(
            store.resolve("p1", "claude-legacy-alias").mode,
            ThinkingMode::Legacy
        );
    }

    #[test]
    fn learns_adaptive_for_a_model_heuristically_judged_legacy() {
        let store = ThinkingCapabilityStore::new();
        let result = rectify(
            &store,
            "p1",
            "claude-sonnet-4-5",
            ThinkingModeRectifyDirection::ToAdaptive,
        );

        assert!(result.applied);
        assert_eq!(result.before, Some(ThinkingMode::Legacy));
        assert_eq!(result.after, Some(ThinkingMode::Adaptive));
        assert_eq!(
            store.resolve("p1", "claude-sonnet-4-5").mode,
            ThinkingMode::Adaptive
        );
    }

    // ==================== 自愈闭环 ====================

    /// 把「上游拒绝 → 学习 → 重试」这条闭环串起来验，不经过 HTTP。
    ///
    /// 检验的是各环节的键与形状能对上：整流器写进 store 的条目，发送端要能查到
    /// 并产出与之匹配的请求体。
    fn codex_request(model: &str) -> Value {
        json!({
            "model": model,
            "max_output_tokens": 40000,
            "reasoning": { "effort": "high" },
            "input": [{ "role": "user", "content": "hi" }]
        })
    }

    fn convert(store: &ThinkingCapabilityStore, provider_id: &str, model: &str) -> Value {
        let capability = store.resolve(provider_id, model);
        crate::proxy::providers::transform_codex_anthropic::
            responses_request_to_anthropic_with_capability(
                codex_request(model),
                4096,
                capability,
            )
            .expect("convert")
    }

    #[test]
    fn self_heals_from_adaptive_to_legacy_after_upstream_rejects_adaptive() {
        let store = ThinkingCapabilityStore::new();
        // 解析不出版本的 Claude 别名 → 启发式判 Adaptive
        let first = convert(&store, "p1", "claude-gateway-alias");
        assert_eq!(first["thinking"]["type"], "adaptive");

        // 上游：不认 adaptive
        let error = "Input tag 'adaptive' found using 'type' does not match expected tags: 'enabled', 'disabled'";
        let direction = detect_thinking_mode_error(Some(error), &enabled_config())
            .expect("should trigger the rectifier");
        assert!(rectify(&store, "p1", "claude-gateway-alias", direction).applied);

        // 重试：发送端查到学到的形状，产出 legacy
        let retry = convert(&store, "p1", "claude-gateway-alias");
        assert_eq!(retry["thinking"]["type"], "enabled");
        assert_eq!(retry["thinking"]["budget_tokens"], 16384);
        assert!(retry.get("output_config").is_none());
    }

    #[test]
    fn self_heals_from_legacy_to_adaptive_after_upstream_rejects_enabled() {
        let store = ThinkingCapabilityStore::new();
        // 网关把这个旧模型名映射到了新模型 → 启发式判 Legacy，但上游只认 adaptive
        let first = convert(&store, "p1", "claude-sonnet-4-5");
        assert_eq!(first["thinking"]["type"], "enabled");

        let error = r#""thinking.type.enabled" is not supported for this model. Use "thinking.type.adaptive" and "output_config.effort" to control thinking behavior."#;
        let direction = detect_thinking_mode_error(Some(error), &enabled_config())
            .expect("should trigger the rectifier");
        assert!(rectify(&store, "p1", "claude-sonnet-4-5", direction).applied);

        let retry = convert(&store, "p1", "claude-sonnet-4-5");
        assert_eq!(retry["thinking"]["type"], "adaptive");
        assert!(retry["thinking"].get("budget_tokens").is_none());
        assert_eq!(retry["output_config"]["effort"], "high");
    }

    #[test]
    fn a_second_rejection_of_the_same_kind_does_not_loop() {
        // 学到之后再收到同方向报错，applied=false → 上层不会再发起重试
        let store = ThinkingCapabilityStore::new();
        let direction = ThinkingModeRectifyDirection::ToLegacy;
        assert!(rectify(&store, "p1", "claude-gateway-alias", direction).applied);
        assert!(!rectify(&store, "p1", "claude-gateway-alias", direction).applied);
    }

    #[test]
    fn does_not_apply_when_already_in_the_target_mode() {
        let store = ThinkingCapabilityStore::new();
        // 已经是 Adaptive，再往 Adaptive 整流没有意义 —— 不该触发重试
        let result = rectify(
            &store,
            "p1",
            "claude-opus-5",
            ThinkingModeRectifyDirection::ToAdaptive,
        );

        assert!(!result.applied);
        assert_eq!(result.before, Some(ThinkingMode::Adaptive));
        assert_eq!(result.after, Some(ThinkingMode::Adaptive));
    }

    // ==================== 模型映射下的自愈（回归） ====================

    #[test]
    fn self_heals_under_model_mapping_where_client_and_upstream_names_differ() {
        // Codex 客户端发 gpt-5-codex，provider 配了 codexUpstreamModel: claude-opus-5。
        // 发送端按上游模型名判 Adaptive 并发出；上游（老网关）不认 adaptive。
        // 整流器若按客户端模型名解析会判成 Legacy（gpt-5-codex 认不出 Claude）＝目标形状，
        // 于是 applied=false、永不 learn()，重试永远重发被拒的 adaptive。
        let store = ThinkingCapabilityStore::new();
        let sent = store
            .resolve_mapped("p1", "gpt-5-codex", "claude-opus-5")
            .mode;
        assert_eq!(sent, ThinkingMode::Adaptive);

        let result = rectify_thinking_mode(
            &store,
            "p1",
            "gpt-5-codex",
            "claude-opus-5",
            ThinkingModeRectifyDirection::ToLegacy,
        );

        assert!(result.applied, "上游亲口拒了 adaptive，必须学到 Legacy");
        assert_eq!(result.before, Some(ThinkingMode::Adaptive));
        assert_eq!(result.after, Some(ThinkingMode::Legacy));
        // 重试时发送端查到学到的形状 —— 自愈闭环真正闭上
        assert_eq!(
            store
                .resolve_mapped("p1", "gpt-5-codex", "claude-opus-5")
                .mode,
            ThinkingMode::Legacy
        );
    }

    #[test]
    fn mapped_self_heal_still_guards_against_looping() {
        // 学到之后同方向再报错：applied=false，上层不会无限重试
        let store = ThinkingCapabilityStore::new();
        let direction = ThinkingModeRectifyDirection::ToLegacy;
        assert!(
            rectify_thinking_mode(&store, "p1", "gpt-5-codex", "claude-opus-5", direction).applied
        );
        assert!(
            !rectify_thinking_mode(&store, "p1", "gpt-5-codex", "claude-opus-5", direction).applied
        );
    }

    #[test]
    fn empty_upstream_model_falls_back_to_the_client_model() {
        // 拿不到上游模型名时退化为旧行为，不 panic 也不误判
        let store = ThinkingCapabilityStore::new();
        let result = rectify_thinking_mode(
            &store,
            "p1",
            "claude-sonnet-4-5",
            "",
            ThinkingModeRectifyDirection::ToAdaptive,
        );

        assert!(result.applied);
        assert_eq!(result.before, Some(ThinkingMode::Legacy));
        assert_eq!(result.after, Some(ThinkingMode::Adaptive));
    }
}
