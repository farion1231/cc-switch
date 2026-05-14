//! 国产 Token Plan 额度查询服务
//!
//! 支持 Kimi For Coding、智谱 GLM、MiniMax 的 Token Plan 额度查询。
//! 复用 subscription 模块的 SubscriptionQuota / QuotaTier 类型。

use super::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_WEEKLY_LIMIT,
};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 供应商检测 ──────────────────────────────────────────────

enum CodingPlanProvider {
    Kimi,
    ZhipuCn,
    ZhipuEn,
    MiniMaxCn,
    MiniMaxEn,
}

fn detect_provider(base_url: &str) -> Option<CodingPlanProvider> {
    let url = base_url.to_lowercase();
    if url.contains("api.kimi.com/coding") {
        Some(CodingPlanProvider::Kimi)
    } else if url.contains("open.bigmodel.cn") || url.contains("bigmodel.cn") {
        Some(CodingPlanProvider::ZhipuCn)
    } else if url.contains("api.z.ai") {
        Some(CodingPlanProvider::ZhipuEn)
    } else if url.contains("api.minimaxi.com") {
        Some(CodingPlanProvider::MiniMaxCn)
    } else if url.contains("api.minimax.io") {
        Some(CodingPlanProvider::MiniMaxEn)
    } else {
        None
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn millis_to_iso8601(ms: i64) -> Option<String> {
    let secs = ms / 1000;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
}

/// 从 JSON 值提取重置时间，兼容字符串和数字格式
/// - 字符串：直接返回（ISO 8601）
/// - 数字：自动判断秒/毫秒并转为 ISO 8601
fn extract_reset_time(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = value.as_i64() {
        // 区分秒和毫秒：秒级时间戳 < 1e12，毫秒 >= 1e12
        let ms = if n < 1_000_000_000_000 { n * 1000 } else { n };
        return millis_to_iso8601(ms);
    }
    None
}

/// 解析 JSON 值为 f64，兼容数字和字符串格式（如 `100` 和 `"100"`）
fn parse_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

fn make_error(msg: String) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(msg),
        queried_at: Some(now_millis()),
    }
}

// ── Kimi For Coding ─────────────────────────────────────────

async fn query_kimi(api_key: &str) -> SubscriptionQuota {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.kimi.com/coding/v1/usages")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        };
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let mut tiers = Vec::new();

    // 5 小时窗口限额（优先显示）
    if let Some(limits) = body.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            if let Some(detail) = limit_item.get("detail") {
                let limit = detail.get("limit").and_then(parse_f64).unwrap_or(1.0);
                let remaining = detail.get("remaining").and_then(parse_f64).unwrap_or(0.0);
                let resets_at = detail.get("resetTime").and_then(extract_reset_time);

                let used = (limit - remaining).max(0.0);
                let utilization = if limit > 0.0 {
                    (used / limit) * 100.0
                } else {
                    0.0
                };
                tiers.push(QuotaTier {
                    name: "five_hour".to_string(),
                    utilization,
                    resets_at,
                });
            }
        }
    }

    // 总体用量（周限额）
    if let Some(usage) = body.get("usage") {
        let limit = usage.get("limit").and_then(parse_f64).unwrap_or(1.0);
        let remaining = usage.get("remaining").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = usage.get("resetTime").and_then(extract_reset_time);

        let used = (limit - remaining).max(0.0);
        let utilization = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };
        tiers.push(QuotaTier {
            name: "weekly_limit".to_string(),
            utilization,
            resets_at,
        });
    }

    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

// ── 智谱 GLM ────────────────────────────────────────────────

/// 把智谱 `data` 里的 `limits[]` 解析成 tier 列表。
///
/// 两条 TOKENS_LIMIT 时按 `nextResetTime` 升序：第 0 条 = 五小时桶（`five_hour`）、
/// 第 1 条 = 每周桶（`weekly_limit`）。缺失 `nextResetTime` 按 `i64::MAX` 排末位。
///
/// 仅一条 TOKENS_LIMIT 时，按距下次重置的时间判断桶类型：
/// - 距重置 > 12 小时 → 每周桶（`weekly_limit`）
/// - 距重置 ≤ 12 小时（含已过期） → 五小时桶（`five_hour`）
///   根据 Zhipu API 的 `unit`/`number` 字段确定 tier 类型。
/// - `unit=3, number=5` → 5 小时桶 (TIER_FIVE_HOUR)
/// - `unit=6, number=1` → 周桶 (TIER_WEEKLY_LIMIT)
fn classify_zhipu_tier(unit: Option<i64>, number: Option<i64>) -> Option<&'static str> {
    match (unit, number) {
        (Some(3), Some(5)) => Some(TIER_FIVE_HOUR),
        (Some(6), Some(1)) => Some(TIER_WEEKLY_LIMIT),
        _ => None,
    }
}

fn parse_zhipu_token_tiers(data: &serde_json::Value) -> Vec<QuotaTier> {
    struct RawLimit {
        tier_name: Option<&'static str>,
        percentage: f64,
        reset_ms: i64,
        resets_at: Option<String>,
    }

    let mut raw_limits: Vec<RawLimit> = Vec::new();
    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            let limit_type = limit_item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !limit_type.eq_ignore_ascii_case("TOKENS_LIMIT") {
                continue;
            }
            let percentage = limit_item
                .get("percentage")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let reset_ms = limit_item
                .get("nextResetTime")
                .and_then(|v| v.as_i64())
                .unwrap_or(i64::MAX);
            let resets_at = if reset_ms == i64::MAX {
                None
            } else {
                millis_to_iso8601(reset_ms)
            };
            let unit = limit_item.get("unit").and_then(|v| v.as_i64());
            let number = limit_item.get("number").and_then(|v| v.as_i64());
            let tier_name = classify_zhipu_tier(unit, number);
            raw_limits.push(RawLimit {
                tier_name,
                percentage,
                reset_ms,
                resets_at,
            });
        }
    }

    if raw_limits.is_empty() {
        return vec![];
    }

    // 主路径：所有 TOKENS_LIMIT 条目都有 unit/number → 直接分类
    let all_classified = raw_limits.iter().all(|r| r.tier_name.is_some());
    if all_classified {
        let mut tiers: Vec<QuotaTier> = raw_limits
            .into_iter()
            .map(|raw| QuotaTier {
                name: raw.tier_name.unwrap().to_string(),
                utilization: raw.percentage,
                resets_at: raw.resets_at,
            })
            .collect();
        // 固定输出顺序：five_hour 在前，weekly_limit 在后
        tiers.sort_by_key(|t| if t.name == TIER_FIVE_HOUR { 0 } else { 1 });
        return tiers;
    }

    // 兜底路径：无 unit/number → 兼容老套餐接口
    raw_limits.sort_by_key(|r| r.reset_ms);

    // 仅一条 TOKENS_LIMIT：按距下次重置的时间判断是每周桶还是五小时桶。
    // 部分老套餐只有周限没有五小时桶，若盲目分配 five_hour 会导致标签错误。
    if raw_limits.len() == 1 {
        let raw = raw_limits.into_iter().next().unwrap();
        let now = now_millis();
        let is_weekly = raw.reset_ms.saturating_sub(now) > 43_200_000; // 12 小时
        return vec![QuotaTier {
            name: (if is_weekly {
                TIER_WEEKLY_LIMIT
            } else {
                TIER_FIVE_HOUR
            })
            .to_string(),
            utilization: raw.percentage,
            resets_at: raw.resets_at,
        }];
    }

    raw_limits
        .into_iter()
        .enumerate()
        .filter_map(|(idx, raw)| {
            let name = match idx {
                0 => TIER_FIVE_HOUR,
                1 => TIER_WEEKLY_LIMIT,
                _ => return None,
            };
            Some(QuotaTier {
                name: name.to_string(),
                utilization: raw.percentage,
                resets_at: raw.resets_at,
            })
        })
        .collect()
}

async fn query_zhipu(api_key: &str) -> SubscriptionQuota {
    let client = crate::proxy::http_client::get();

    // 统一走 api.z.ai 国际站（中国站 bigmodel.cn 有反爬机制）
    let resp = client
        .get("https://api.z.ai/api/monitor/usage/quota/limit")
        .header("Authorization", api_key) // 注意：智谱不加 Bearer 前缀
        .header("Content-Type", "application/json")
        .header("Accept-Language", "en-US,en")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        };
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    // 检查业务级别错误
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return make_error(format!("API error: {msg}"));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
    };

    let tiers = parse_zhipu_token_tiers(data);

    // 套餐等级存入 credential_message
    let level = data
        .get("level")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: level,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

// ── MiniMax ─────────────────────────────────────────────────

async fn query_minimax(api_key: &str, is_cn: bool) -> SubscriptionQuota {
    let client = crate::proxy::http_client::get();

    let api_domain = if is_cn {
        "api.minimaxi.com"
    } else {
        "api.minimax.io"
    };
    let url = format!("https://{api_domain}/v1/api/openplatform/coding_plan/remains");

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        };
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    // 检查业务级别错误
    if let Some(base_resp) = body.get("base_resp") {
        let status_code = base_resp
            .get("status_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if status_code != 0 {
            let msg = base_resp
                .get("status_msg")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return make_error(format!("API error (code {status_code}): {msg}"));
        }
    }

    let mut tiers = Vec::new();

    if let Some(model_remains) = body.get("model_remains").and_then(|v| v.as_array()) {
        // 只取第一个模型（MiniMax-M*，主力编程模型）
        if let Some(item) = model_remains.first() {
            // usage_count 是剩余量（满额=total，用完=0），需反转为已用百分比
            let interval_total = item
                .get("current_interval_total_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let interval_remaining = item
                .get("current_interval_usage_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let end_time = item.get("end_time").and_then(|v| v.as_i64());

            if interval_total > 0.0 {
                tiers.push(QuotaTier {
                    name: "five_hour".to_string(),
                    utilization: ((interval_total - interval_remaining) / interval_total) * 100.0,
                    resets_at: end_time.and_then(millis_to_iso8601),
                });
            }

            // 周额度
            let weekly_total = item
                .get("current_weekly_total_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let weekly_remaining = item
                .get("current_weekly_usage_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let weekly_end = item.get("weekly_end_time").and_then(|v| v.as_i64());

            if weekly_total > 0.0 {
                tiers.push(QuotaTier {
                    name: "weekly_limit".to_string(),
                    utilization: ((weekly_total - weekly_remaining) / weekly_total) * 100.0,
                    resets_at: weekly_end.and_then(millis_to_iso8601),
                });
            }
        }
    }

    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

// ── 公开入口 ────────────────────────────────────────────────

pub async fn get_coding_plan_quota(
    base_url: &str,
    api_key: &str,
) -> Result<SubscriptionQuota, String> {
    if api_key.trim().is_empty() {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::NotFound,
            credential_message: None,
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: None,
            queried_at: None,
        });
    }

    let provider = match detect_provider(base_url) {
        Some(p) => p,
        None => {
            return Ok(SubscriptionQuota {
                tool: "coding_plan".to_string(),
                credential_status: CredentialStatus::NotFound,
                credential_message: None,
                success: false,
                tiers: vec![],
                extra_usage: None,
                error: None,
                queried_at: None,
            })
        }
    };

    let quota = match provider {
        CodingPlanProvider::Kimi => query_kimi(api_key).await,
        CodingPlanProvider::ZhipuCn | CodingPlanProvider::ZhipuEn => query_zhipu(api_key).await,
        CodingPlanProvider::MiniMaxCn => query_minimax(api_key, true).await,
        CodingPlanProvider::MiniMaxEn => query_minimax(api_key, false).await,
    };

    Ok(quota)
}

#[cfg(test)]
mod tests {
    use super::{parse_zhipu_token_tiers, TIER_FIVE_HOUR, TIER_WEEKLY_LIMIT};
    use serde_json::json;

    #[test]
    fn zhipu_two_tiers_classified_by_unit_number() {
        // 两条 TOKENS_LIMIT，通过 unit/number 字段确定类型，不依赖 nextResetTime 排序。
        // 故意把周限放数组前面，验证分类不依赖输入顺序。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 53.0, "nextResetTime": 2_000_000_000_000_i64, "unit": 6, "number": 1 },
                { "type": "TOKENS_LIMIT", "percentage": 44.0, "nextResetTime": 1_000_000_000_000_i64, "unit": 3, "number": 5 },
                { "type": "TIME_LIMIT",   "percentage":  7.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 44.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 53.0);
    }

    #[test]
    fn zhipu_single_five_hour_tier_by_unit_number() {
        // 老套餐：仅一条 TOKENS_LIMIT，unit=3, number=5 → 5 小时桶。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 2.0, "nextResetTime": 1_774_967_594_803_i64, "unit": 3, "number": 5 },
                { "type": "TIME_LIMIT", "percentage": 0.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 2.0);
    }

    #[test]
    fn zhipu_single_weekly_tier_by_unit_number() {
        // 仅一条 TOKENS_LIMIT，unit=6, number=1 → 周限套餐。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 30.0, "nextResetTime": 2_000_000_000_000_i64, "unit": 6, "number": 1 },
                { "type": "TIME_LIMIT", "percentage": 5.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[0].utilization, 30.0);
    }

    #[test]
    fn zhipu_no_token_limits_returns_empty() {
        let data = json!({ "limits": [{ "type": "TIME_LIMIT", "percentage": 5.0 }] });
        assert!(parse_zhipu_token_tiers(&data).is_empty());
    }

    #[test]
    fn zhipu_missing_reset_time_sorts_last() {
        // 无 unit/number → 兜底路径：没有 nextResetTime 的条目排到末位。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 99.0 },
                { "type": "TOKENS_LIMIT", "percentage": 10.0, "nextResetTime": 1_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 10.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 99.0);
        assert!(tiers[1].resets_at.is_none());
    }

    #[test]
    fn zhipu_type_is_case_insensitive() {
        // type 字段大小写不敏感，同时带 unit/number 验证主路径。
        let data = json!({
            "limits": [
                { "type": "tokens_limit", "percentage": 12.0, "nextResetTime": 1_000_000_000_000_i64, "unit": 3, "number": 5 },
                { "type": "Tokens_Limit", "percentage": 34.0, "nextResetTime": 2_000_000_000_000_i64, "unit": 6, "number": 1 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 12.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 34.0);
    }

    #[test]
    fn zhipu_invalid_percentage_falls_back_to_zero() {
        // percentage 为字符串或 null 时按 0 处理，带 unit/number。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": "invalid", "nextResetTime": 1_000_000_000_000_i64, "unit": 3, "number": 5 },
                { "type": "TOKENS_LIMIT", "percentage": null,      "nextResetTime": 2_000_000_000_000_i64, "unit": 6, "number": 1 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].utilization, 0.0);
        assert_eq!(tiers[1].utilization, 0.0);
    }

    #[test]
    fn zhipu_extreme_percentage_values_pass_through() {
        // 负数 / 超 100 不裁剪，带 unit/number。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": -5.0,  "nextResetTime": 1_000_000_000_000_i64, "unit": 3, "number": 5 },
                { "type": "TOKENS_LIMIT", "percentage": 150.0, "nextResetTime": 2_000_000_000_000_i64, "unit": 6, "number": 1 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].utilization, -5.0);
        assert_eq!(tiers[1].utilization, 150.0);
    }

    #[test]
    fn zhipu_more_than_two_token_limits_keeps_first_two() {
        // 无 unit/number → 兜底路径最多保留两条。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 1.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 2.0, "nextResetTime": 2_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 3.0, "nextResetTime": 3_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
    }

    #[test]
    fn zhipu_mixed_unit_number_triggers_fallback() {
        // 部分条目有 unit/number，部分没有 → 走兜底路径。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 10.0, "nextResetTime": 1_000_000_000_000_i64, "unit": 3, "number": 5 },
                { "type": "TOKENS_LIMIT", "percentage": 20.0, "nextResetTime": 2_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 10.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 20.0);
    }

    #[test]
    fn zhipu_fallback_single_tier_near_reset_is_five_hour() {
        // 老套餐接口（无 unit/number）：仅一条 TOKENS_LIMIT，nextResetTime 在 12 小时内 → five_hour。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 2.0, "nextResetTime": 1_774_967_594_803_i64 },
                { "type": "TIME_LIMIT", "percentage": 0.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 2.0);
    }

    #[test]
    fn zhipu_fallback_single_tier_far_future_is_weekly() {
        // 老套餐接口（无 unit/number）：仅一条 TOKENS_LIMIT，nextResetTime 在数天后 → weekly_limit。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 30.0, "nextResetTime": 2_000_000_000_000_i64 },
                { "type": "TIME_LIMIT", "percentage": 5.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[0].utilization, 30.0);
    }
}
