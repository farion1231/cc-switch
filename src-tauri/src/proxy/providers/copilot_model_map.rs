//! GitHub Copilot 模型 ID 归一化与 live-list 解析
//!
//! Copilot upstream 仅接受 dot 形式的 Claude 4.x 模型 ID（如 `claude-sonnet-4.6`），
//! 而 Claude Code 客户端发出 dash 形式（如 `claude-sonnet-4-6`、`claude-sonnet-4-6[1m]`）。
//! 不归一化会触发上游 400 `model_not_supported`。
//!
//! 仅做语法归一化不够：账号订阅级别可能不开放某个具体模型。
//! `resolve_against_models` 用 `/models` live 列表做精确匹配，找不到时
//! 按 family（haiku/sonnet/opus）+ 最高版本号 fallback。

use super::copilot_auth::CopilotModel;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotProtocol {
    Messages,
    Responses,
    Chat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCopilotModel {
    pub id: String,
    pub vendor: String,
    pub protocol: CopilotProtocol,
}

/// 归一化客户端 model ID 为 Copilot upstream 接受的形式。
/// 返回 `None` 表示无需变换（已归一化、非 Claude 4.x 系列、或空输入）。
pub(super) fn normalize_to_copilot_id(client_id: &str) -> Option<String> {
    let trimmed = client_id.trim();
    let bytes = trimmed.as_bytes();

    if bytes.len() < 8 || !bytes[..7].eq_ignore_ascii_case(b"claude-") {
        return None;
    }

    let has_one_m_bracket = ends_with_ascii_ci(bytes, b"[1m]");

    // Fast path: 已含点 + 不带 [1m] → 已归一化（绝大多数请求走这里）
    if trimmed.contains('.') && !has_one_m_bracket {
        return None;
    }

    let (base, has_1m_suffix) = split_one_m_suffix(trimmed);
    let stripped = strip_trailing_date(base);
    let dotted = dashes_to_dot_in_last_version(stripped);

    if dotted.is_none() && !has_1m_suffix {
        return None;
    }

    let mut candidate = dotted.unwrap_or_else(|| stripped.to_string());
    if has_1m_suffix {
        candidate.push_str("-1m");
    }
    (candidate != trimmed).then_some(candidate)
}

/// 在请求体中应用 model ID 归一化。
pub fn apply_copilot_model_normalization(mut body: Value) -> Value {
    let Some(orig) = body.get("model").and_then(|v| v.as_str()) else {
        return body;
    };
    if let Some(normalized) = normalize_to_copilot_id(orig) {
        log::debug!("[CopilotNormalizer] {orig} → {normalized}");
        body["model"] = Value::String(normalized);
    }
    body
}

fn ends_with_ascii_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[haystack.len() - needle.len()..].eq_ignore_ascii_case(needle)
}

fn split_one_m_suffix(id: &str) -> (&str, bool) {
    let bytes = id.as_bytes();
    if ends_with_ascii_ci(bytes, b"[1m]") {
        return (&id[..bytes.len() - 4], true);
    }
    if ends_with_ascii_ci(bytes, b"-1m") {
        return (&id[..bytes.len() - 3], true);
    }
    (id, false)
}

fn strip_trailing_date(id: &str) -> &str {
    let Some(last_dash) = id.rfind('-') else {
        return id;
    };
    let suffix = &id[last_dash + 1..];
    if suffix.len() == 8 && suffix.bytes().all(|b| b.is_ascii_digit()) {
        &id[..last_dash]
    } else {
        id
    }
}

/// 把 `…-X-Y`（X、Y 都是纯数字的末两段）变成 `…-X.Y`。
/// 返回 `None` 表示模式不匹配（保守策略避免误伤 `claude-3-5-sonnet` 等历史 ID）。
fn dashes_to_dot_in_last_version(id: &str) -> Option<String> {
    let last_dash = id.rfind('-')?;
    let last_segment = &id[last_dash + 1..];
    if last_segment.is_empty() || !last_segment.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let head = &id[..last_dash];
    let prev_dash = head.rfind('-')?;
    let prev_segment = &head[prev_dash + 1..];
    if prev_segment.is_empty() || !prev_segment.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{head}.{last_segment}"))
}

/// 用 Copilot live 模型列表确认/降级 model ID。
///
/// 流程：
/// 1. 先做语法归一化（dash→dot、`[1m]`→`-1m`）
/// 2. 在 `models` 中精确匹配；找到则使用归一化后的 ID
/// 3. 找不到时按 family（haiku/sonnet/opus）取最高版本号 fallback
///    （优先保留 `-1m` 标志；都没有则取 base 版）
///
/// 返回 `None` 表示无需变换或无可降级的 family 候选（保留原 ID 让上游决定，
/// 让用户拿到明确的 `model_not_supported` 而非被静默替换）。
pub fn resolve_against_models(client_id: &str, models: &[CopilotModel]) -> Option<String> {
    let normalized = normalize_to_copilot_id(client_id);
    let target = normalized.as_deref().unwrap_or(client_id);

    if models.iter().any(|m| m.id.eq_ignore_ascii_case(target)) {
        return normalized.filter(|s| s != client_id);
    }

    let fallback = family_fallback(target, models)?;
    if fallback.eq_ignore_ascii_case(client_id) {
        None
    } else {
        Some(fallback)
    }
}

pub fn resolve_model(client_id: &str, models: &[CopilotModel]) -> Option<ResolvedCopilotModel> {
    let normalized = normalize_to_copilot_id(client_id);
    let target = normalized.as_deref().unwrap_or(client_id);
    let model = models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(target))
        .or_else(|| {
            let fallback = family_fallback(target, models)?;
            models
                .iter()
                .find(|model| model.id.eq_ignore_ascii_case(&fallback))
        })?;

    Some(ResolvedCopilotModel {
        id: model.id.clone(),
        vendor: model.vendor.clone(),
        protocol: protocol_for(model),
    })
}

/// `/models` 暂时不可用时的保守回退。OpenAI 风格 ID 继续走 Responses，
/// 其余模型走兼容面更广的 Chat；一旦 live 数据恢复即由 supported_endpoints 覆盖。
pub fn fallback_protocol_for_model_id(model_id: &str) -> CopilotProtocol {
    let lower = model_id.trim().to_ascii_lowercase();
    if lower.starts_with("gpt-")
        || lower.starts_with("chatgpt-")
        || lower.starts_with('o')
            && lower[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
    {
        CopilotProtocol::Responses
    } else {
        CopilotProtocol::Chat
    }
}

fn protocol_for(model: &CopilotModel) -> CopilotProtocol {
    if model
        .supported_endpoints
        .iter()
        .any(|endpoint| endpoint == "/v1/messages")
    {
        CopilotProtocol::Messages
    } else if model
        .supported_endpoints
        .iter()
        .any(|endpoint| endpoint == "/responses")
    {
        CopilotProtocol::Responses
    } else if model
        .supported_endpoints
        .iter()
        .any(|endpoint| endpoint == "/chat/completions")
    {
        CopilotProtocol::Chat
    } else if model.vendor.eq_ignore_ascii_case("openai") {
        CopilotProtocol::Responses
    } else {
        CopilotProtocol::Chat
    }
}

/// 先按 Copilot 的模型归一化/fallback 规则解析客户端 model id，再返回真实 vendor。
pub fn vendor_for<'a>(model_id: &str, models: &'a [CopilotModel]) -> Option<&'a str> {
    let normalized = normalize_to_copilot_id(model_id);
    let target = normalized.as_deref().unwrap_or(model_id);
    if let Some(model) = models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(target))
    {
        return Some(model.vendor.as_str());
    }

    let fallback = family_fallback(target, models)?;
    models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(&fallback))
        .map(|model| model.vendor.as_str())
}

fn detect_family(id: &str) -> Option<&'static str> {
    let lower = id.to_ascii_lowercase();
    if lower.contains("haiku") {
        Some("haiku")
    } else if lower.contains("sonnet") {
        Some("sonnet")
    } else if lower.contains("opus") {
        Some("opus")
    } else {
        None
    }
}

/// 提取 family 后第一段 `MAJOR.MINOR` 版本号。
/// 例：`claude-sonnet-4.6` → (4, 6)；`claude-sonnet-4.6-1m` → (4, 6)。
fn extract_major_minor(id: &str) -> Option<(u32, u32)> {
    let lower = id.to_ascii_lowercase();
    let family = detect_family(&lower)?;
    let after = &lower[lower.find(family)? + family.len()..];
    let after = after.strip_prefix('-')?;
    let segment = after.split(['-', '[', ' ']).next()?;
    let mut parts = segment.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

fn family_fallback(target: &str, models: &[CopilotModel]) -> Option<String> {
    let family = detect_family(target)?;
    let want_1m = target.ends_with("-1m");

    let pick_best = |require_1m: bool| -> Option<String> {
        models
            .iter()
            .filter(|m| {
                let lower = m.id.to_ascii_lowercase();
                let is_1m = m.context_window.is_some_and(|tokens| tokens >= 1_000_000)
                    || lower.contains("-1m");
                lower.contains(family) && is_1m == require_1m
            })
            .filter_map(|m| extract_major_minor(&m.id).map(|v| (m, v)))
            .max_by_key(|(_, v)| *v)
            .map(|(m, _)| m.id.clone())
    };

    if want_1m {
        pick_best(true).or_else(|| pick_best(false))
    } else {
        pick_best(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dashes_to_dot_basic() {
        assert_eq!(
            normalize_to_copilot_id("claude-sonnet-4-6"),
            Some("claude-sonnet-4.6".to_string())
        );
        assert_eq!(
            normalize_to_copilot_id("claude-opus-4-6"),
            Some("claude-opus-4.6".to_string())
        );
        assert_eq!(
            normalize_to_copilot_id("claude-haiku-4-5"),
            Some("claude-haiku-4.5".to_string())
        );
    }

    #[test]
    fn one_m_bracket_to_dash() {
        assert_eq!(
            normalize_to_copilot_id("claude-sonnet-4-6[1m]"),
            Some("claude-sonnet-4.6-1m".to_string())
        );
        assert_eq!(
            normalize_to_copilot_id("claude-opus-4-6[1m]"),
            Some("claude-opus-4.6-1m".to_string())
        );
    }

    #[test]
    fn one_m_bracket_on_already_dotted() {
        // claude-sonnet-4.6[1m] 走非 fast-path 分支（has_one_m_bracket=true），
        // 应被改写为 -1m 形式
        assert_eq!(
            normalize_to_copilot_id("claude-sonnet-4.6[1m]"),
            Some("claude-sonnet-4.6-1m".to_string())
        );
    }

    #[test]
    fn date_suffix_stripped() {
        assert_eq!(
            normalize_to_copilot_id("claude-haiku-4-5-20251001"),
            Some("claude-haiku-4.5".to_string())
        );
        assert_eq!(
            normalize_to_copilot_id("claude-sonnet-4-5-20250929"),
            Some("claude-sonnet-4.5".to_string())
        );
    }

    #[test]
    fn already_copilot_format_returns_none() {
        assert_eq!(normalize_to_copilot_id("claude-sonnet-4.6"), None);
        assert_eq!(normalize_to_copilot_id("claude-opus-4.6-1m"), None);
        assert_eq!(normalize_to_copilot_id("claude-haiku-4.5"), None);
    }

    #[test]
    fn non_claude_models_untouched() {
        assert_eq!(normalize_to_copilot_id("gpt-5"), None);
        assert_eq!(normalize_to_copilot_id("gpt-4o-mini"), None);
        assert_eq!(normalize_to_copilot_id("o3"), None);
        assert_eq!(normalize_to_copilot_id(""), None);
    }

    #[test]
    fn legacy_three_part_versions_untouched() {
        assert_eq!(normalize_to_copilot_id("claude-3-5-sonnet"), None);
        assert_eq!(normalize_to_copilot_id("claude-3-5-sonnet-20241022"), None);
    }

    #[test]
    fn case_insensitive_on_prefix_and_suffix() {
        assert_eq!(
            normalize_to_copilot_id("Claude-Sonnet-4-6"),
            Some("Claude-Sonnet-4.6".to_string())
        );
        assert_eq!(
            normalize_to_copilot_id("claude-sonnet-4-6[1M]"),
            Some("claude-sonnet-4.6-1m".to_string())
        );
    }

    #[test]
    fn bracket_one_m_with_date_combined() {
        assert_eq!(
            normalize_to_copilot_id("claude-haiku-4-5-20251001[1m]"),
            Some("claude-haiku-4.5-1m".to_string())
        );
    }

    #[test]
    fn apply_rewrites_body() {
        let body = json!({"model": "claude-sonnet-4-6", "max_tokens": 1024});
        let out = apply_copilot_model_normalization(body);
        assert_eq!(out["model"], "claude-sonnet-4.6");
        assert_eq!(out["max_tokens"], 1024);
    }

    #[test]
    fn apply_no_change_when_already_normalized() {
        let body = json!({"model": "claude-sonnet-4.6"});
        let out = apply_copilot_model_normalization(body);
        assert_eq!(out["model"], "claude-sonnet-4.6");
    }

    #[test]
    fn apply_handles_missing_model() {
        let body = json!({"messages": []});
        let out = apply_copilot_model_normalization(body);
        assert!(out.get("model").is_none());
    }

    fn model(id: &str) -> CopilotModel {
        CopilotModel {
            id: id.to_string(),
            name: id.to_string(),
            vendor: "anthropic".to_string(),
            model_picker_enabled: true,
            context_window: None,
            supported_endpoints: vec![],
        }
    }

    #[test]
    fn resolve_exact_match_after_normalize() {
        let models = vec![
            model("claude-sonnet-4.6"),
            model("claude-opus-4.6"),
            model("claude-haiku-4.5"),
        ];
        assert_eq!(
            resolve_against_models("claude-sonnet-4-6", &models),
            Some("claude-sonnet-4.6".to_string())
        );
    }

    #[test]
    fn resolve_returns_none_when_already_valid() {
        let models = vec![model("claude-sonnet-4.6")];
        assert_eq!(resolve_against_models("claude-sonnet-4.6", &models), None);
    }

    #[test]
    fn resolve_falls_back_to_highest_family_version() {
        // 用户请求 opus 4.8 但 Copilot 账号只有 opus 4.6
        let models = vec![
            model("claude-opus-4.5"),
            model("claude-opus-4.6"),
            model("claude-sonnet-4.6"),
        ];
        assert_eq!(
            resolve_against_models("claude-opus-4.8", &models),
            Some("claude-opus-4.6".to_string())
        );
    }

    #[test]
    fn resolve_prefers_1m_when_requested() {
        let models = vec![
            model("claude-sonnet-4.6"),
            model("claude-sonnet-4.6-1m"),
            model("claude-opus-4.6"),
        ];
        assert_eq!(
            resolve_against_models("claude-sonnet-4-6[1m]", &models),
            Some("claude-sonnet-4.6-1m".to_string())
        );
    }

    #[test]
    fn resolve_falls_back_to_base_when_1m_unavailable() {
        // 账号没开 -1m 变体时降级到 base
        let models = vec![model("claude-sonnet-4.6")];
        assert_eq!(
            resolve_against_models("claude-sonnet-4-6[1m]", &models),
            Some("claude-sonnet-4.6".to_string())
        );
    }

    #[test]
    fn resolve_returns_none_when_family_absent() {
        // 账号完全没有 opus 时不做强行替换，让上游报错
        let models = vec![model("claude-sonnet-4.6"), model("claude-haiku-4.5")];
        assert_eq!(resolve_against_models("claude-opus-4.6", &models), None);
    }

    #[test]
    fn resolve_handles_non_claude_target() {
        let models = vec![model("claude-sonnet-4.6")];
        assert_eq!(resolve_against_models("gpt-5", &models), None);
    }

    fn model_with_context(id: &str, context_window: u64) -> CopilotModel {
        CopilotModel {
            context_window: Some(context_window),
            ..model(id)
        }
    }

    #[test]
    fn resolve_one_m_marker_to_internal_variant_by_context_window() {
        let models = vec![
            model_with_context("claude-opus-4.5", 200_000),
            model_with_context("claude-opus-4.7-1m-internal", 1_000_000),
        ];
        assert_eq!(
            resolve_against_models("claude-opus-4-5[1M]", &models),
            Some("claude-opus-4.7-1m-internal".to_string())
        );
    }

    #[test]
    fn vendor_for_resolves_one_m_marker_before_lookup() {
        let models = vec![model_with_context("claude-opus-4.7-1m-internal", 1_000_000)];
        assert_eq!(
            vendor_for("claude-opus-4-5[1M]", &models),
            Some("anthropic")
        );
    }

    #[test]
    fn resolve_model_prefers_messages_then_responses_then_chat() {
        let mut messages = model("claude-opus-4.8");
        messages.supported_endpoints = vec![
            "/chat/completions".to_string(),
            "/responses".to_string(),
            "/v1/messages".to_string(),
        ];
        let resolved = resolve_model("claude-opus-4.8", &[messages]).unwrap();
        assert_eq!(resolved.protocol, CopilotProtocol::Messages);

        let mut responses = model("gpt-5.5");
        responses.vendor = "OpenAI".to_string();
        responses.supported_endpoints =
            vec!["/chat/completions".to_string(), "/responses".to_string()];
        let resolved = resolve_model("gpt-5.5", &[responses]).unwrap();
        assert_eq!(resolved.protocol, CopilotProtocol::Responses);

        let mut chat = model("claude-sonnet-4.6");
        chat.supported_endpoints = vec!["/chat/completions".to_string()];
        let resolved = resolve_model("claude-sonnet-4.6", &[chat]).unwrap();
        assert_eq!(resolved.protocol, CopilotProtocol::Chat);
    }

    #[test]
    fn resolve_model_falls_back_by_vendor_without_endpoint_metadata() {
        let mut openai = model("gpt-5.5");
        openai.vendor = "OpenAI".to_string();
        assert_eq!(
            resolve_model("gpt-5.5", &[openai]).unwrap().protocol,
            CopilotProtocol::Responses
        );

        let anthropic = model("claude-opus-4.8");
        assert_eq!(
            resolve_model("claude-opus-4.8", &[anthropic])
                .unwrap()
                .protocol,
            CopilotProtocol::Chat
        );
    }

    #[test]
    fn fallback_protocol_for_model_id_is_independent_of_legacy_provider_format() {
        assert_eq!(
            fallback_protocol_for_model_id("gpt-5.5"),
            CopilotProtocol::Responses
        );
        assert_eq!(
            fallback_protocol_for_model_id("o3-mini"),
            CopilotProtocol::Responses
        );
        assert_eq!(
            fallback_protocol_for_model_id("claude-opus-4.8"),
            CopilotProtocol::Chat
        );
    }

    #[test]
    fn resolve_model_family_fallback_keeps_capabilities_from_final_model() {
        let mut fallback = model_with_context("claude-opus-4.6", 200_000);
        fallback.supported_endpoints = vec!["/v1/messages".to_string()];
        let resolved = resolve_model("claude-opus-4.8", &[fallback]).unwrap();
        assert_eq!(resolved.id, "claude-opus-4.6");
        assert_eq!(resolved.protocol, CopilotProtocol::Messages);
    }
}
