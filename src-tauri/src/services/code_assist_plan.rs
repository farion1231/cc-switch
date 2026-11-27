//! Gemini CLI 额度查询顺带拿到的套餐名（`loadCodeAssist` 响应）。

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssistResponse {
    current_tier: Option<Value>,
    /// 订阅（Google AI Pro / Ultra）在这里；有时 Pro 号也缺，缺了看 currentTier
    paid_tier: Option<Value>,
}

/// `loadCodeAssist` 响应里的套餐名：先看 `paidTier.name`（去掉前面多余的「Google 」，如
/// 「Google AI Pro」→「AI Pro」），没有时按 `currentTier.id` 写 Free / Paid（gemini-cli 和
/// CodexBar 都这样取）
pub(crate) fn code_assist_plan(body: &[u8]) -> Option<String> {
    let response: LoadCodeAssistResponse = serde_json::from_slice(body).ok()?;
    fn tier_name(tier: &Option<Value>) -> Option<&str> {
        tier.as_ref()?
            .get("name")?
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }
    if let Some(name) = tier_name(&response.paid_tier) {
        let id = response
            .paid_tier
            .as_ref()
            .and_then(|tier| tier.get("id")?.as_str());
        return Some(format_plan_name(name, id));
    }
    let id = response.current_tier.as_ref()?.get("id")?.as_str()?;
    match id {
        "free-tier" => Some("Free".to_string()),
        "standard-tier" => Some("Paid".to_string()),
        _ => tier_name(&response.current_tier).map(|name| format_plan_name(name, Some(id))),
    }
}

/// 规整 `loadCodeAssist` 的套餐显示名：去掉 `Google AI <档位>` 前面多余的 `Google `，
/// 再补齐 Ultra 5x / 20x 档位。
fn format_plan_name(name: &str, tier_id: Option<&str>) -> String {
    let short = strip_google_ai_prefix(name);
    with_ultra_variant(short, tier_id)
}

/// 去掉 `Google AI <档位>` 前面多余的 `Google `：官方卡本身已标明服务，留着「Google 」既不增加信息，
/// 又容易把第二行后面的内容截成省略号。
/// 仅当 `Google ` 后紧跟 `AI <非空内容>` 时裁剪，避免误伤 `"Google AI"` 本身或其它名称。
fn strip_google_ai_prefix(name: &str) -> &str {
    let Some(prefix) = name.get(..7) else {
        return name;
    };
    if !prefix.eq_ignore_ascii_case("google ") {
        return name;
    }
    let rest = name[7..].trim_start();
    match rest.get(..3) {
        Some(ai) if ai.eq_ignore_ascii_case("ai ") && !rest[3..].trim().is_empty() => rest,
        _ => name,
    }
}

/// Google AI Ultra 分 5x、20x 两档，`paidTier.name` 都只写「Google AI Ultra」，靠 id 区分
/// （11-07 实测）：`g1-ultra-lite-tier` 是 5x（升级提示写「可升级到更高的 Ultra 方案」），
/// `g1-ultra-tier` 是 20x（「已是最好的 Google AI 方案」）。名字里已带档位时不重复补。
fn with_ultra_variant(name: &str, tier_id: Option<&str>) -> String {
    let variant = match tier_id {
        Some("g1-ultra-lite-tier") => "5x",
        Some("g1-ultra-tier") => "20x",
        _ => return name.to_string(),
    };
    let lower = name.to_ascii_lowercase();
    if !lower.contains("ultra") || lower.contains("5x") || lower.contains("20x") {
        return name.to_string();
    }
    format!("{name} {variant}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_prefers_the_paid_tier_name() {
        let plan = |body: &str| code_assist_plan(body.as_bytes());
        assert_eq!(
            plan(r#"{"currentTier":{"id":"standard-tier","name":"Gemini Code Assist"},"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#)
                .as_deref(),
            Some("AI Pro")
        );
        // paidTier 缺了：按当前档写
        assert_eq!(
            plan(r#"{"currentTier":{"id":"standard-tier","name":"Gemini Code Assist"}}"#)
                .as_deref(),
            Some("Paid")
        );
        assert_eq!(
            plan(
                r#"{"currentTier":{"id":"free-tier","name":"Gemini Code Assist for individuals"}}"#
            )
            .as_deref(),
            Some("Free")
        );
        assert_eq!(plan(r#"{"ineligibleTiers":[]}"#), None);
    }

    #[test]
    fn ultra_plans_show_their_5x_or_20x_variant() {
        let plan = |body: &str| code_assist_plan(body.as_bytes());
        // loadCodeAssist 的真实 paidTier（11-07），两档名字一样，只有 id 不同
        assert_eq!(
            plan(r#"{"currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("AI Ultra 5x")
        );
        assert_eq!(
            plan(r#"{"currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"g1-ultra-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("AI Ultra 20x")
        );
        // 以后 Google 自己把档位写进名字时不重复补；别的套餐不动
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-ultra-tier","name":"Google AI Ultra 20x"}}"#).as_deref(),
            Some("AI Ultra 20x")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra 5X"}}"#)
                .as_deref(),
            Some("AI Ultra 5X")
        );
        assert_eq!(
            plan(r#"{"currentTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("AI Ultra 5x")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#).as_deref(),
            Some("AI Pro")
        );
    }

    #[test]
    fn strips_redundant_google_prefix_only_from_google_ai_plans() {
        let plan = |body: &str| code_assist_plan(body.as_bytes());
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-plus-tier","name":"Google AI Plus"}}"#).as_deref(),
            Some("AI Plus")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-pro-tier","name":"Google  AI Pro"}}"#).as_deref(),
            Some("AI Pro")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-ultra-tier","name":"google ai ultra"}}"#).as_deref(),
            Some("ai ultra 20x")
        );
        // 仅剩 "Google AI" 或并非 "Google AI <档位>" 时原样保留，避免截出残缺名或踩 UTF-8 边界
        assert_eq!(
            plan(r#"{"paidTier":{"id":"other","name":"Google AI"}}"#).as_deref(),
            Some("Google AI")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"other","name":"Google One AI Premium"}}"#).as_deref(),
            Some("Google One AI Premium")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"other","name":"Google AI专业版"}}"#).as_deref(),
            Some("Google AI专业版")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"other","name":"中文套餐名称"}}"#).as_deref(),
            Some("中文套餐名称")
        );
    }
}
