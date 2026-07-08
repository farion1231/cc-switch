//! 国产 Token Plan 额度查询服务
//!
//! 支持 Kimi For Coding、智谱 GLM、MiniMax 的 Token Plan 额度查询。
//! 复用 subscription 模块的 SubscriptionQuota / QuotaTier 类型。

use super::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_MONTHLY, TIER_WEEKLY_LIMIT,
};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 供应商检测 ──────────────────────────────────────────────

enum CodingPlanProvider {
    Kimi,
    ZhipuCn,
    ZhipuEn,
    MiniMaxCn,
    MiniMaxEn,
    ZenMux,
    /// 火山方舟 Agent Plan / Coding Plan（base_url 形如
    /// `https://ark.cn-beijing.volces.com/api/coding[/v3]`）。
    Volcengine,
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
    } else if url.contains("zenmux") {
        Some(CodingPlanProvider::ZenMux)
    } else if url.contains("volces.com/api/coding") {
        // 仅匹配 Coding/Agent Plan 入口；DouBaoSeed 按量付费走 /api/v3 与
        // /api/compatible，没有套餐额度，不在此命中。
        Some(CodingPlanProvider::Volcengine)
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
        // 0/负时间戳（如火山 session 无活跃窗口回 -1）视为无重置时间
        if n <= 0 {
            return None;
        }
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

async fn query_kimi(api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.kimi.com/coding/v1/usages")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
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
                    used_value_usd: None,
                    max_value_usd: None,
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
            used_value_usd: None,
            max_value_usd: None,
        });
    }

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── 智谱 GLM ────────────────────────────────────────────────

/// 智谱 TOKENS_LIMIT 条目按 `unit` 字段的显式窗口分类。
enum ZhipuWindow {
    FiveHour,
    Weekly,
}

/// 按 `unit` 字段判定 TOKENS_LIMIT 条目所属窗口。
///
/// 实测形态（bigmodel.cn 与 z.ai 共用同一后端，字段一致）：
/// - `unit: 3, number: 5` → 5 小时滚动窗口（老/新套餐均有）
/// - `unit: 6, number: 7` 与 `unit: 6, number: 1` → 每周窗口（两种取值都被
///   实测过，故只锚定 `unit`、不绑 `number`）
///
/// `unit` 缺失或值不认识时返回 None，由调用方走重置时间启发式兜底。
fn classify_zhipu_window(item: &serde_json::Value) -> Option<ZhipuWindow> {
    match item.get("unit").and_then(|v| v.as_i64()) {
        Some(3) => Some(ZhipuWindow::FiveHour),
        Some(6) => Some(ZhipuWindow::Weekly),
        _ => None,
    }
}

/// 把智谱 `data` 里的 `limits[]` 解析成 tier 列表。
///
/// 分类优先级：
/// 1. 显式字段：`unit` 标识窗口类型（见 [`classify_zhipu_window`]）。不能按
///    `nextResetTime` 排序代替——周期末尾每周窗口会比 5 小时窗口更早重置
///    （issue #3036），时间排序在该场景必然把两桶标反。
/// 2. 兜底启发式（`unit` 缺失或不识别）：无 `nextResetTime` 的条目优先归
///    five_hour（5 小时桶在 0% 等状态下可能没有 reset），其余按 reset 升序
///    依次填入仍空缺的槽位。
///
/// 老套餐（2026-02-12 前订阅）只回 1 条
/// `TOKENS_LIMIT`，自然降级为仅展示 `five_hour`；新套餐回 2 条。
fn parse_zhipu_token_tiers(data: &serde_json::Value) -> Vec<QuotaTier> {
    type Entry = (Option<i64>, f64, Option<String>);
    let mut five_hour: Option<Entry> = None;
    let mut weekly: Option<Entry> = None;
    let mut unclassified: Vec<Entry> = Vec::new();

    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            let limit_type = limit_item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // 大小写不敏感比较：上游若把 "TOKENS_LIMIT" 改成小写或驼峰，依然能识别
            if !limit_type.eq_ignore_ascii_case("TOKENS_LIMIT") {
                continue;
            }
            let percentage = limit_item
                .get("percentage")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let reset_ms = limit_item.get("nextResetTime").and_then(|v| v.as_i64());
            let reset_iso = reset_ms.and_then(millis_to_iso8601);
            let entry = (reset_ms, percentage, reset_iso);
            match classify_zhipu_window(limit_item) {
                Some(ZhipuWindow::FiveHour) if five_hour.is_none() => five_hour = Some(entry),
                Some(ZhipuWindow::Weekly) if weekly.is_none() => weekly = Some(entry),
                _ => unclassified.push(entry),
            }
        }
    }

    unclassified.sort_by_key(|(reset, _, _)| (reset.is_some(), reset.unwrap_or(i64::MIN)));
    for entry in unclassified {
        if five_hour.is_none() {
            five_hour = Some(entry);
        } else if weekly.is_none() {
            weekly = Some(entry);
        }
        // 智谱当前最多两条 TOKENS_LIMIT，多余的忽略
    }

    let mut tiers = Vec::new();
    for (name, slot) in [(TIER_FIVE_HOUR, five_hour), (TIER_WEEKLY_LIMIT, weekly)] {
        if let Some((_, percentage, resets_at)) = slot {
            tiers.push(QuotaTier {
                name: name.to_string(),
                utilization: percentage,
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }
    tiers
}

/// Resolve the Zhipu quota endpoint from the user's configured `base_url`.
///
/// Zhipu ships as two distinct presets (Zhipu GLM = `open.bigmodel.cn`,
/// Zhipu GLM en = `api.z.ai`) that share the same quota path and JSON shape.
/// The quota endpoint lives on the same host as the user's coding endpoint,
/// so we route by `base_url` and let the caller's existing reachability
/// (they're already using this host to run coding) determine success — no
/// cross-host fallback, no auth-error heuristics.
fn zhipu_quota_base(base_url: &str) -> &'static str {
    if base_url.to_lowercase().contains("bigmodel.cn") {
        "https://open.bigmodel.cn"
    } else {
        "https://api.z.ai"
    }
}

async fn query_zhipu(base_url: &str, api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();
    let url = format!(
        "{}/api/monitor/usage/quota/limit",
        zhipu_quota_base(base_url)
    );

    let resp = client
        .get(&url)
        .header("Authorization", api_key) // 注意：智谱不加 Bearer 前缀
        .header("Content-Type", "application/json")
        .header("Accept-Language", "en-US,en")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    // 检查业务级别错误
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return Ok(make_error(format!("API error: {msg}")));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return Ok(make_error("Missing 'data' field in response".to_string())),
    };

    let tiers = parse_zhipu_token_tiers(data);

    // 套餐等级存入 credential_message
    let level = data
        .get("level")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: level,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── MiniMax ─────────────────────────────────────────────────

async fn query_minimax(api_key: &str, is_cn: bool) -> Result<SubscriptionQuota, String> {
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
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
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
            return Ok(make_error(format!("API error (code {status_code}): {msg}")));
        }
    }

    // 提取纯函数便于无 mock 单元测试;新接口直接给"剩余百分比",反转为已用百分比
    let tiers = parse_minimax_tiers(&body);

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── ZenMux ──────────────────────────────────────────────────

async fn query_zenmux(base_url: &str, api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get(base_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    // 检查业务级别错误
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return Ok(make_error(format!("API error: {msg}")));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return Ok(make_error("Missing 'data' field in response".to_string())),
    };

    let mut tiers = Vec::new();

    // 5 小时窗口限额
    if let Some(q5h) = data.get("quota_5_hour") {
        let usage_pct = q5h
            .get("usage_percentage")
            .and_then(parse_f64)
            .unwrap_or(0.0);
        let resets_at = q5h
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        let used_usd = q5h.get("used_value_usd").and_then(parse_f64);
        let max_usd = q5h.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: "five_hour".to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value_usd: used_usd,
            max_value_usd: max_usd,
        });
    }

    // 7 天窗口限额
    if let Some(q7d) = data.get("quota_7_day") {
        let usage_pct = q7d
            .get("usage_percentage")
            .and_then(parse_f64)
            .unwrap_or(0.0);
        let resets_at = q7d
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        let used_usd = q7d.get("used_value_usd").and_then(parse_f64);
        let max_usd = q7d.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: "weekly_limit".to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value_usd: used_usd,
            max_value_usd: max_usd,
        });
    }

    // 套餐等级和账户状态存入 credential_message
    let plan_tier = data
        .get("plan")
        .and_then(|p| p.get("tier"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let account_status = data
        .get("account_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let plan_info = if !plan_tier.is_empty() {
        format!("{plan_tier} ({account_status})")
    } else {
        String::new()
    };

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: if plan_info.is_empty() {
            None
        } else {
            Some(plan_info)
        },
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

/// 从 `/coding_plan/remains` 响应中解析 MiniMax 编程套餐的额度 tier。
///
/// 新接口语义:`current_*_remaining_percent` 是"剩余百分比"(0-100),
/// `model_remains` 数组里有 `general`(编程套餐)和 `video` 等其他模型,
/// 这里只取 `general`,跳过 video。
///
/// 5h 桶始终存在;周桶并非所有套餐都有,靠 `current_weekly_status == 1`
/// 判定激活(无周限额套餐该字段为 3,`remaining_percent` 恒为 100,不应展示)。
fn parse_minimax_tiers(body: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();

    let Some(model_remains) = body.get("model_remains").and_then(|v| v.as_array()) else {
        return tiers;
    };

    // 只取 model_name == "general" 的条目,跳过 video 等非编程模型
    let Some(item) = model_remains.iter().find(|item| {
        item.get("model_name")
            .and_then(|v| v.as_str())
            .map(|s| s == "general")
            .unwrap_or(false)
    }) else {
        return tiers;
    };

    // 5h 桶:剩余百分比 → 已用百分比
    if let Some(remain_pct) = item
        .get("current_interval_remaining_percent")
        .and_then(|v| v.as_f64())
    {
        let resets_at = item
            .get("end_time")
            .and_then(|v| v.as_i64())
            .and_then(millis_to_iso8601);
        tiers.push(QuotaTier {
            name: TIER_FIVE_HOUR.to_string(),
            utilization: 100.0 - remain_pct,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }

    // 周桶:仅当 status=1 时激活;status=3 等表示该套餐无周限额,跳过
    if item.get("current_weekly_status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(remain_pct) = item
            .get("current_weekly_remaining_percent")
            .and_then(|v| v.as_f64())
        {
            let resets_at = item
                .get("weekly_end_time")
                .and_then(|v| v.as_i64())
                .and_then(millis_to_iso8601);
            tiers.push(QuotaTier {
                name: TIER_WEEKLY_LIMIT.to_string(),
                utilization: 100.0 - remain_pct,
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }

    tiers
}

// ── 火山方舟 Agent Plan / Coding Plan ───────────────────────
//
// 与 Kimi/MiniMax（数据面 Bearer 余额接口）不同，火山用量接口是**控制面
// OpenAPI**：统一网关 `open.volcengineapi.com`（**不是**数据面推理域名
// `ark.cn-beijing.volces.com`），形如
// `POST https://open.volcengineapi.com/?Action=...&Version=2024-01-01&Region=cn-beijing`，
// **强制火山引擎签名 V4（AK/SK）**——实测复用推理 Bearer Key 会被网关以
// `400 InvalidAuthorization` 拒绝（格式层拒绝，非权限问题）。因此用户需在用量查询
// 里另填火山账号的 AccessKey ID + Secret（与推理 Key 是两套凭据）。两个 plan 用
// 同一份 AK/SK，故鉴权类错误直接停、不再试另一个 plan。
//
// 自动探测：先调 `GetAFPUsage`（Agent Plan，回绝对额度 Quota/Used），未订阅再调
// `GetCodingPlanUsage`（Coding Plan，回百分比）。

/// 控制面 OpenAPI 统一网关（区别于数据面推理域名 ark.cn-beijing.volces.com）。
const VOLCENGINE_OPENAPI_HOST: &str = "open.volcengineapi.com";
const VOLCENGINE_API_VERSION: &str = "2024-01-01";
/// ark 控制面 OpenAPI 的默认 Region（Agent/Coding Plan 目前在 cn-beijing）。
const VOLCENGINE_DEFAULT_REGION: &str = "cn-beijing";

/// 单次 OpenAPI 调用的归类结果。
enum VolcCall {
    /// 2xx 且 JSON 可解析、无 OpenAPI 级错误（业务 Result 仍可能为空=未订阅）。
    Body(serde_json::Value),
    /// 硬鉴权失败（HTTP 401/403 或 AccessDenied/Signature 等错误码）——两个 plan
    /// 共用凭据，命中即停。
    Auth(String),
    /// 非鉴权 HTTP 错误 / 响应体非法 JSON——记录后可继续尝试另一个 plan。
    Soft(String),
    /// 瞬时传输失败（网络/超时/读体中断）——同 host 的另一个 plan 大概率同样
    /// 失败，调用方应立即以 `Err` 传播（前端 reject → retry + 保留上次成功值）。
    Transient(String),
}

/// 从数据面 base_url 提取控制面 OpenAPI 所需的 Region（如
/// `ark.cn-beijing.volces.com` → `cn-beijing`）；无法识别时回落 cn-beijing。
/// 控制面 Host 是固定网关（`VOLCENGINE_OPENAPI_HOST`），不随 base_url 变化。
fn volcengine_region(base_url: &str) -> String {
    let host = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("");
    host.split('.')
        .find(|p| p.starts_with("cn-") || p.starts_with("ap-"))
        .map(|p| p.to_string())
        .unwrap_or_else(|| VOLCENGINE_DEFAULT_REGION.to_string())
}

/// 判断 OpenAPI 错误码是否属于鉴权类（需要硬停并提示换 AK/SK）。
fn volcengine_is_auth_error_code(code: &str) -> bool {
    let c = code.to_lowercase();
    c.contains("auth")
        || c.contains("signature")
        || c.contains("accessdenied")
        || c.contains("denied")
        || c.contains("unauthorized")
        || c.contains("forbidden")
        || c.contains("credential")
        || c.contains("token")
}

/// 提取火山 OpenAPI 响应里的 `ResponseMetadata.Error`（或顶层 `Error`）。
fn volcengine_response_error(body: &serde_json::Value) -> Option<(String, String)> {
    let err = body
        .get("ResponseMetadata")
        .and_then(|m| m.get("Error"))
        .or_else(|| body.get("Error"))?;
    let code = err
        .get("Code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let msg = err
        .get("Message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if code.is_empty() && msg.is_empty() {
        None
    } else {
        Some((code, msg))
    }
}

/// 鉴权失败时的引导文案，附加在错误后。
const VOLCENGINE_AKSK_HINT: &str =
    "Check the AccessKey ID / Secret are correct and the account has Ark usage-query (OpenAPI) permission.";

// ── 火山引擎签名 V4（AK/SK）─────────────────────────────────
//
// 算法是 AWS SigV4 的火山变体（对照官方 volc-openapi-demos/signature/java/Sign.java）。
// **两处致命差异，照搬 s3.rs 的标准 SigV4 会签名失败**：
//   1. canonical headers 与 SignedHeaders 用**固定顺序**
//      `host;x-date;x-content-sha256;content-type`（**不按字母序**，s3.rs 是字母序）；
//   2. algorithm 串 `HMAC-SHA256`（无 `AWS4` 前缀）、credential scope 结尾 `request`
//      （非 `aws4_request`）、签名密钥 `kDate=HMAC(SK, date)`（SK 不加 `AWS4` 前缀）。
// canonical query 仍按 key 字母序（与标准 SigV4 一致）；service=`ark`、POST、空 body。

const VOLCENGINE_SERVICE: &str = "ark";
const VOLCENGINE_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const VOLCENGINE_SIGNED_HEADERS: &str = "host;x-date;x-content-sha256;content-type";

fn volc_hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn volc_sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// RFC3986 unreserved 之外全部按 `%XX` 编码（用于 canonical query string）。
fn volc_uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// 构造按 key 字母序排序、逐段 URL 编码的 canonical query string。
/// 同一份字符串既用于签名也用于实际请求 URL，保证两者完全一致。
fn volcengine_canonical_query(action: &str, region: &str) -> String {
    let mut pairs = [
        ("Action", action),
        ("Region", region),
        ("Version", VOLCENGINE_API_VERSION),
    ];
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", volc_uri_encode(k), volc_uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 生成火山引擎签名 V4 的鉴权头，返回 `(Authorization, X-Date, X-Content-Sha256)`，
/// 三者都要塞进请求头；`canonical_query` 必须与实际请求 URL 的 query 完全一致。
/// `now` 作参数传入便于写确定性单测。
fn volcengine_sign(
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    canonical_query: &str,
    body: &[u8],
    now: chrono::DateTime<chrono::Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let x_content_sha256 = volc_sha256_hex(body);

    // 固定顺序 canonical headers（火山特有，**不排序**）。
    let canonical_headers = format!(
        "host:{VOLCENGINE_OPENAPI_HOST}\nx-date:{x_date}\nx-content-sha256:{x_content_sha256}\ncontent-type:{VOLCENGINE_CONTENT_TYPE}\n"
    );
    let canonical_request = format!(
        "POST\n/\n{canonical_query}\n{canonical_headers}\n{VOLCENGINE_SIGNED_HEADERS}\n{x_content_sha256}"
    );

    let credential_scope = format!("{short_date}/{region}/{VOLCENGINE_SERVICE}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{credential_scope}\n{}",
        volc_sha256_hex(canonical_request.as_bytes())
    );

    // 签名密钥派生：kDate=HMAC(SK, date)（SK **不加** AWS4 前缀），终止串 `request`。
    let k_date = volc_hmac_sha256(secret_access_key.as_bytes(), short_date.as_bytes());
    let k_region = volc_hmac_sha256(&k_date, region.as_bytes());
    let k_service = volc_hmac_sha256(&k_region, VOLCENGINE_SERVICE.as_bytes());
    let k_signing = volc_hmac_sha256(&k_service, b"request");
    let signature: String = volc_hmac_sha256(&k_signing, string_to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let authorization = format!(
        "HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={VOLCENGINE_SIGNED_HEADERS}, Signature={signature}"
    );
    (authorization, x_date, x_content_sha256)
}

async fn volcengine_openapi_call(
    region: &str,
    access_key_id: &str,
    secret_access_key: &str,
    action: &str,
) -> VolcCall {
    let client = crate::proxy::http_client::get();
    // canonical query 同时用于签名与实际 URL，确保两者逐字一致（否则签名不匹配）。
    let canonical_query = volcengine_canonical_query(action, region);
    let url = format!("https://{VOLCENGINE_OPENAPI_HOST}/?{canonical_query}");
    let body: &[u8] = b"";
    let (authorization, x_date, x_content_sha256) = volcengine_sign(
        access_key_id,
        secret_access_key,
        region,
        &canonical_query,
        body,
        chrono::Utc::now(),
    );

    let resp = client
        .post(&url)
        .header("X-Date", x_date)
        .header("X-Content-Sha256", x_content_sha256)
        .header("Content-Type", VOLCENGINE_CONTENT_TYPE)
        .header("Authorization", authorization)
        .body(body.to_vec())
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return VolcCall::Transient(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return VolcCall::Auth(format!(
            "Authentication failed (HTTP {status}). {VOLCENGINE_AKSK_HINT}"
        ));
    }
    if !status.is_success() {
        // 火山 OpenAPI 网关对签名/凭据类错误常返 4xx（多为 HTTP 400）并携带与 200
        // 路径相同的 ResponseMetadata.Error 信封，而非 401/403。这里也解析信封，让
        // Bearer 被拒时仍能给出 AK/SK 引导并标记凭据失效，而不是当成普通 API 错误。
        let raw = resp.text().await.unwrap_or_default();
        if let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some((code, msg)) = volcengine_response_error(&body) {
                if volcengine_is_auth_error_code(&code) {
                    return VolcCall::Auth(format!(
                        "Authentication failed (HTTP {status}, {code}): {msg}. {VOLCENGINE_AKSK_HINT}"
                    ));
                }
                return VolcCall::Soft(format!("API error (HTTP {status}, {code}): {msg}"));
            }
        }
        return VolcCall::Soft(format!("API error (HTTP {status}): {raw}"));
    }

    // 同 Bearer 路径：先 bytes() 再解析——读体失败是瞬时（Transient），解析失败
    // 是确定性（Soft）。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return VolcCall::Transient(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return VolcCall::Soft(format!("Failed to parse response: {e}")),
    };

    // 火山 OpenAPI 业务错误常以 200 + ResponseMetadata.Error 返回。
    if let Some((code, msg)) = volcengine_response_error(&body) {
        if volcengine_is_auth_error_code(&code) {
            return VolcCall::Auth(format!(
                "Authentication failed ({code}): {msg}. {VOLCENGINE_AKSK_HINT}"
            ));
        }
        return VolcCall::Soft(format!("API error ({code}): {msg}"));
    }

    VolcCall::Body(body)
}

/// 解析 `GetAFPUsage` 的 `Result` 为 tier 列表。
///
/// 展示 5h / 周 / 月三个窗口（与控制台一致）；`AFPDaily` 被官方控制台隐藏
/// （其 Quota 常高于周上限，属历史默认值而非强制限额），故跳过。
/// `Quota`/`Used` 是绝对 AFP 值，已用百分比 = Used/Quota×100；`Quota<=0` 视为
/// 该窗口未订阅/未启用，跳过——也用于把"已鉴权但无 Agent Plan"识别为空结果，
/// 从而回落到 Coding Plan 探测。
fn parse_afp_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    for (key, name) in [
        ("AFPFiveHour", TIER_FIVE_HOUR),
        ("AFPWeekly", TIER_WEEKLY_LIMIT),
        ("AFPMonthly", TIER_MONTHLY),
    ] {
        let Some(win) = result.get(key) else { continue };
        let quota = win.get("Quota").and_then(parse_f64).unwrap_or(0.0);
        if quota <= 0.0 {
            continue;
        }
        let used = win.get("Used").and_then(parse_f64).unwrap_or(0.0);
        // 已用百分比；不做范围裁剪，与 parse_zhipu_token_tiers/parse_minimax_tiers
        // 的约定一致（下游渲染层负责显示策略）。
        let utilization = used / quota * 100.0;
        let resets_at = win.get("ResetTime").and_then(extract_reset_time);
        tiers.push(QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    tiers
}

/// 把 `GetCodingPlanUsage` 的 window 标签归一到 tier 名。
fn volcengine_coding_window(label: &str) -> Option<&'static str> {
    match label.to_lowercase().as_str() {
        "session" | "5h" | "fivehour" | "five_hour" | "rolling_5h" => Some(TIER_FIVE_HOUR),
        "weekly" | "week" | "7d" => Some(TIER_WEEKLY_LIMIT),
        "monthly" | "month" => Some(TIER_MONTHLY),
        _ => None,
    }
}

/// 解析 `GetCodingPlanUsage` 的 `Result` 为 tier 列表（防御式）。
///
/// 该接口官方文档未给出逐字段规格，依据官方 ark-cli 描述：回 session/weekly/
/// monthly 窗口、**只给百分比**（已用）、重置时间是秒级。这里宽松匹配
/// `QuotaUsage`/`Usages`/`Details` 数组及多种字段名，命中即用、未命中跳过。
fn parse_coding_plan_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    let arr = result
        .get("QuotaUsage")
        .and_then(|v| v.as_array())
        .or_else(|| result.get("Usages").and_then(|v| v.as_array()))
        .or_else(|| result.get("Details").and_then(|v| v.as_array()));
    let Some(arr) = arr else { return tiers };

    for item in arr {
        // 真实字段是 `Level`（实测 2026-06-21：session/weekly/monthly）；其余作防御式 fallback。
        let label = item
            .get("Level")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("Type").and_then(|v| v.as_str()))
            .or_else(|| item.get("Period").and_then(|v| v.as_str()))
            .or_else(|| item.get("Label").and_then(|v| v.as_str()))
            .or_else(|| item.get("Window").and_then(|v| v.as_str()))
            .unwrap_or("");
        let Some(name) = volcengine_coding_window(label) else {
            continue;
        };
        let utilization = item
            .get("Percent")
            .and_then(parse_f64)
            .or_else(|| item.get("UsedPercent").and_then(parse_f64))
            .or_else(|| item.get("UsagePercent").and_then(parse_f64))
            .unwrap_or(0.0);
        // 兼容秒/毫秒/字符串（extract_reset_time 内部已区分秒与毫秒）。
        let resets_at = item
            .get("ResetTime")
            .or_else(|| item.get("ResetTimestamp"))
            .and_then(extract_reset_time);
        tiers.push(QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    tiers
}

fn volcengine_success(tiers: Vec<QuotaTier>, plan: Option<String>) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: plan,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

fn volcengine_auth_error(detail: String) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Expired,
        credential_message: Some("Invalid API key".to_string()),
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(detail),
        queried_at: Some(now_millis()),
    }
}

async fn query_volcengine(
    base_url: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<SubscriptionQuota, String> {
    let region = volcengine_region(base_url);
    let mut soft_errors: Vec<String> = Vec::new();
    // 2xx + 无 Error 信封但解析不出额度时，截断原始响应用于诊断（区分"真没订阅"
    // 与"字段名/包裹层猜错"）。签名若不通会走 Auth/Soft 分支，到不了这里。
    let mut empty_responses: Vec<String> = Vec::new();
    let summarize = |action: &str, body: &serde_json::Value| -> String {
        let raw: String = body.to_string().chars().take(700).collect();
        format!("{action}={raw}")
    };

    // 1) Agent Plan：GetAFPUsage
    match volcengine_openapi_call(&region, access_key_id, secret_access_key, "GetAFPUsage").await {
        VolcCall::Auth(detail) => return Ok(volcengine_auth_error(detail)),
        VolcCall::Transient(detail) => return Err(format!("GetAFPUsage: {detail}")),
        VolcCall::Soft(detail) => soft_errors.push(format!("GetAFPUsage: {detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_afp_tiers(result);
            if !tiers.is_empty() {
                let plan = result
                    .get("PlanType")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("Agent Plan {s}"));
                return Ok(volcengine_success(tiers, plan));
            }
            empty_responses.push(summarize("GetAFPUsage", &body));
        }
    }

    // 2) Coding Plan：GetCodingPlanUsage
    match volcengine_openapi_call(
        &region,
        access_key_id,
        secret_access_key,
        "GetCodingPlanUsage",
    )
    .await
    {
        VolcCall::Auth(detail) => return Ok(volcengine_auth_error(detail)),
        VolcCall::Transient(detail) => return Err(format!("GetCodingPlanUsage: {detail}")),
        VolcCall::Soft(detail) => soft_errors.push(format!("GetCodingPlanUsage: {detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_coding_plan_tiers(result);
            if !tiers.is_empty() {
                return Ok(volcengine_success(tiers, Some("Coding Plan".to_string())));
            }
            empty_responses.push(summarize("GetCodingPlanUsage", &body));
        }
    }

    if !soft_errors.is_empty() {
        Ok(make_error(soft_errors.join("; ")))
    } else if !empty_responses.is_empty() {
        // 签名已通过、请求到达业务层，但响应里没有可解析的额度。带上原始响应，
        // 便于核对真实字段名/包裹层，或确认确实未订阅。
        Ok(make_error(format!(
            "No active subscription found (signature OK). Raw: {}",
            empty_responses.join(" || ")
        )))
    } else {
        Ok(make_error(
            "No active Agent Plan or Coding Plan subscription found for this credential"
                .to_string(),
        ))
    }
}

// ── 公开入口 ────────────────────────────────────────────────

/// 构造"凭据缺失 / 域名未命中"的失败结果（NotFound 状态 + 明确错误文案）。
fn coding_plan_not_found(error: &str) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::NotFound,
        credential_message: None,
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(error.to_string()),
        queried_at: None,
    }
}

/// 查询编程套餐额度。瞬时传输失败（网络/超时/读体中断）返回 `Err`（前端 reject →
/// retry + 保留上次成功值）；确定性失败（凭据缺失/未知域名/鉴权/非 2xx/业务错误）
/// 返回 `Ok(success:false)` 立即透出文案。判定按 reqwest 错误种类在折叠点完成。
pub async fn get_coding_plan_quota(
    base_url: &str,
    api_key: &str,
    access_key_id: Option<&str>,
    secret_access_key: Option<&str>,
) -> Result<SubscriptionQuota, String> {
    let provider = match detect_provider(base_url) {
        Some(p) => p,
        // 域名未命中已知套餐供应商（如第三方中转站）：给出明确错误而非静默失败
        None => return Ok(coding_plan_not_found("Unknown coding plan provider")),
    };

    // 火山方舟走控制面 AK/SK 签名（区别于其他供应商的数据面 Bearer api_key），凭据
    // 校验与查询路径都不同，单独分支提前处理。
    if let CodingPlanProvider::Volcengine = provider {
        let ak = access_key_id.unwrap_or("").trim();
        let sk = secret_access_key.unwrap_or("").trim();
        if ak.is_empty() || sk.is_empty() {
            return Ok(coding_plan_not_found(
                "Volcengine usage query needs the account AccessKey ID + Secret (not the inference API key)",
            ));
        }
        return query_volcengine(base_url, ak, sk).await;
    }

    // 其余供应商：数据面 Bearer api_key。
    // 与 balance::get_balance 一致：给出明确错误，避免 footer 显示无信息的失败
    if api_key.trim().is_empty() {
        return Ok(coding_plan_not_found("API key is empty"));
    }

    match provider {
        CodingPlanProvider::Kimi => query_kimi(api_key).await,
        CodingPlanProvider::ZhipuCn | CodingPlanProvider::ZhipuEn => {
            query_zhipu(base_url, api_key).await
        }
        CodingPlanProvider::MiniMaxCn => query_minimax(api_key, true).await,
        CodingPlanProvider::MiniMaxEn => query_minimax(api_key, false).await,
        CodingPlanProvider::ZenMux => query_zenmux(base_url, api_key).await,
        // 火山已在上面的 AK/SK 分支提前返回，此处不可达。
        CodingPlanProvider::Volcengine => {
            unreachable!("volcengine handled via AK/SK branch above")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_afp_tiers, parse_coding_plan_tiers, parse_minimax_tiers, parse_zhipu_token_tiers,
        volcengine_canonical_query, volcengine_is_auth_error_code, volcengine_region,
        volcengine_response_error, volcengine_sign, zhipu_quota_base, TIER_FIVE_HOUR, TIER_MONTHLY,
        TIER_WEEKLY_LIMIT,
    };
    use serde_json::json;

    #[test]
    fn zhipu_new_plan_two_tiers_sorted_by_reset_time() {
        // 新套餐：两条 TOKENS_LIMIT，nextResetTime 较近的归 five_hour、较远的归 weekly_limit。
        // 故意把"周限"放数组前面，验证不依赖输入顺序。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 53.0, "nextResetTime": 2_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 44.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TIME_LIMIT",   "percentage":  7.0 },
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
    fn zhipu_old_plan_single_tier_falls_back_to_five_hour() {
        // 老套餐（2026-02-12 前订阅）：仅一条 TOKENS_LIMIT，无周限。
        let data = json!({
            "limits": [
                {
                    "type": "TOKENS_LIMIT",
                    "percentage": 2.0,
                    "nextResetTime": 1_774_967_594_803_i64
                },
                { "type": "TIME_LIMIT", "percentage": 0.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 2.0);
    }

    #[test]
    fn zhipu_no_token_limits_returns_empty() {
        let data = json!({ "limits": [{ "type": "TIME_LIMIT", "percentage": 5.0 }] });
        assert!(parse_zhipu_token_tiers(&data).is_empty());
    }

    #[test]
    fn zhipu_missing_reset_time_is_five_hour_when_weekly_has_reset() {
        // 真实反馈：5 小时桶为 0% 时可能没有 nextResetTime；每周桶带 reset。
        // 这种形态不能按 reset 升序把每周桶误判为 five_hour。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 25.0, "nextResetTime": 2_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 0.0 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 0.0);
        assert!(tiers[0].resets_at.is_none());
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 25.0);
        assert!(tiers[1].resets_at.is_some());
    }

    #[test]
    fn zhipu_type_is_case_insensitive() {
        // 防御性：上游若把 "TOKENS_LIMIT" 改成 "tokens_limit"（仅大小写变化）仍能识别。
        // 注意：分隔符差异（如 "TokensLimit" 去掉下划线）不在兼容范围。
        let data = json!({
            "limits": [
                { "type": "tokens_limit", "percentage": 12.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "Tokens_Limit", "percentage": 34.0, "nextResetTime": 2_000_000_000_000_i64 }
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
        // percentage 为字符串或 null 时不应崩溃，按 0 处理（仍展示 tier，但用量为 0）。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": "invalid", "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": null,      "nextResetTime": 2_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].utilization, 0.0);
        assert_eq!(tiers[1].utilization, 0.0);
    }

    #[test]
    fn zhipu_extreme_percentage_values_pass_through() {
        // 负数 / 超 100 不做范围裁剪——下游渲染层负责显示策略，解析层只负责忠实搬运。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "percentage": -5.0,  "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 150.0, "nextResetTime": 2_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].utilization, -5.0);
        assert_eq!(tiers[1].utilization, 150.0);
    }

    #[test]
    fn zhipu_unit_field_overrides_reset_order_when_weekly_resets_sooner() {
        // 真实案例（issue #3036，2026-06-10 再次复现）：每周周期末尾，周桶比
        // 5 小时桶更早重置。官网真实值：5h 用 1%（约 5h 后重置）、每周用 42%
        // （约 1h 后重置）。旧逻辑按 reset 升序必然标反，unit 字段须优先。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "unit": 6, "number": 7, "percentage": 42.0, "nextResetTime": 1_000_003_600_000_i64 },
                { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 1.0,  "nextResetTime": 1_000_018_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 1.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 42.0);
    }

    #[test]
    fn zhipu_weekly_unit_six_number_one_variant() {
        // z.ai 也观测过 (unit:6, number:1) 表示每周窗口（按"1 周"计），
        // 分类只看 unit，number 取值不影响。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 30.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 10.0, "nextResetTime": 2_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 10.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 30.0);
    }

    #[test]
    fn zhipu_partial_unit_fields_fill_remaining_slot() {
        // 只有周桶带 unit 时，缺 unit 的另一条应填入剩下的 five_hour 槽位，
        // 即便它的 reset 更晚——显式分类结果不受时间排序干扰。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "unit": 6, "number": 7, "percentage": 42.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 1.0, "nextResetTime": 2_000_000_000_000_i64 }
            ]
        });
        let tiers = parse_zhipu_token_tiers(&data);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 1.0);
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 42.0);
    }

    #[test]
    fn zhipu_unknown_unit_values_fall_back_to_reset_order() {
        // 未识别的 unit 枚举值不猜语义，整体回落旧的重置时间启发式。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "unit": 9, "percentage": 44.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "unit": 9, "percentage": 53.0, "nextResetTime": 2_000_000_000_000_i64 }
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
    fn zhipu_duplicate_unit_classification_fills_other_slot() {
        // 防御性：两条都标成 5 小时窗（上游异常）时，第一条占 five_hour，
        // 第二条降级走兜底填入 weekly，保证不丢数据也不 panic。
        let data = json!({
            "limits": [
                { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 10.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 20.0, "nextResetTime": 2_000_000_000_000_i64 }
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
    fn zhipu_more_than_two_token_limits_keeps_first_two() {
        // 防御性：智谱当前最多两条 TOKENS_LIMIT，若上游意外增加第三条应被丢弃，避免命名空缺。
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

    // ── MiniMax ──

    #[test]
    fn minimax_general_two_tiers_from_remaining_percent() {
        // 主路径:general 桶 5h 剩 98% / weekly 剩 95% → 已用 2% / 5%
        let body = json!({
            "model_remains": [
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 98.0,
                    "current_weekly_remaining_percent": 95.0,
                    "current_interval_status": 1,
                    "current_weekly_status": 1,
                    "end_time": 1_780_329_600_000_i64,
                    "weekly_end_time": 1_780_848_000_000_i64
                },
                {
                    "model_name": "video",
                    "current_interval_remaining_percent": 100.0,
                    "current_weekly_remaining_percent": 100.0
                }
            ],
            "base_resp": { "status_code": 0, "status_msg": "success" }
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 2.0);
        assert!(tiers[0].resets_at.is_some());
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert_eq!(tiers[1].utilization, 5.0);
        assert!(tiers[1].resets_at.is_some());
    }

    #[test]
    fn minimax_skips_video_and_finds_general_in_any_position() {
        // 防御性:即使 video 排在数组前面,general 排在后面,仍应被定位到。
        let body = json!({
            "model_remains": [
                {
                    "model_name": "video",
                    "current_interval_remaining_percent": 50.0,
                    "current_weekly_remaining_percent": 50.0
                },
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 80.0,
                    "current_weekly_remaining_percent": 70.0,
                    "current_interval_status": 1,
                    "current_weekly_status": 1
                }
            ]
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 2);
        // 取的是 general 桶,不是 video(20%/30% 而非 50%/50%)
        assert_eq!(tiers[0].utilization, 20.0);
        assert_eq!(tiers[1].utilization, 30.0);
    }

    #[test]
    fn minimax_missing_general_returns_empty() {
        // model_remains 只有 video / 空 / 缺字段 → 不应崩溃,tiers 为空
        let body = json!({
            "model_remains": [
                {
                    "model_name": "video",
                    "current_interval_remaining_percent": 100.0,
                    "current_weekly_remaining_percent": 100.0
                }
            ]
        });
        assert!(parse_minimax_tiers(&body).is_empty());

        let body_empty: serde_json::Value = json!({ "model_remains": [] });
        assert!(parse_minimax_tiers(&body_empty).is_empty());

        let body_no_field = json!({});
        assert!(parse_minimax_tiers(&body_no_field).is_empty());
    }

    #[test]
    fn minimax_missing_percent_fields_skips_tier() {
        // 字段缺失时只跳过对应桶,另一边仍能展示
        let body = json!({
            "model_remains": [{
                "model_name": "general",
                "current_interval_remaining_percent": 60.0,
                "current_weekly_status": 1
                // 缺 current_weekly_remaining_percent
            }]
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 40.0);
    }

    #[test]
    fn minimax_negative_percent_passes_through() {
        // 防御性:与 parse_zhipu_token_tiers 约定一致,负数 / 超 100 不做范围裁剪
        let body = json!({
            "model_remains": [{
                "model_name": "general",
                "current_interval_remaining_percent": -5.0,
                "current_weekly_remaining_percent": 150.0,
                "current_interval_status": 1,
                "current_weekly_status": 1
            }]
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].utilization, 105.0); // 100 - (-5)
        assert_eq!(tiers[1].utilization, -50.0); // 100 - 150
    }

    #[test]
    fn minimax_weekly_status_3_skips_weekly_tier() {
        // 无周限额套餐:current_weekly_status=3,remaining_percent 恒为 100,
        // 不应推 weekly_limit tier(否则会显示"0% 已用"的假周桶)
        let body = json!({
            "model_remains": [
                {
                    "model_name": "general",
                    "start_time": 1_780_347_600_000_i64,
                    "end_time": 1_780_365_600_000_i64,
                    "remains_time": 4_161_372_i64,
                    "current_interval_remaining_percent": 99,
                    "current_interval_status": 1,
                    "current_weekly_total_count": 0,
                    "current_weekly_usage_count": 0,
                    "weekly_start_time": 1_780_243_200_000_i64,
                    "weekly_end_time": 1_780_848_000_000_i64,
                    "weekly_remains_time": 486_561_372_i64,
                    "current_weekly_status": 3,
                    "current_weekly_remaining_percent": 100
                },
                {
                    "model_name": "video",
                    "current_interval_remaining_percent": 100,
                    "current_weekly_status": 3,
                    "current_weekly_remaining_percent": 100
                }
            ],
            "base_resp": { "status_code": 0, "status_msg": "success" }
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 1.0);
        assert!(tiers[0].resets_at.is_some());
    }

    #[test]
    fn minimax_weekly_status_2_also_skips_weekly_tier() {
        // 防御性:除 1 之外的 status 都视为周桶未激活,跳过
        let body = json!({
            "model_remains": [{
                "model_name": "general",
                "current_interval_remaining_percent": 80.0,
                "current_weekly_remaining_percent": 50.0,
                "current_weekly_status": 2
            }]
        });
        let tiers = parse_minimax_tiers(&body);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert_eq!(tiers[0].utilization, 20.0);
    }

    #[test]
    fn zhipu_quota_base_routes_bigmodel_url_to_cn_endpoint() {
        assert_eq!(
            zhipu_quota_base("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn"
        );
    }

    #[test]
    fn zhipu_quota_base_routes_z_ai_url_to_en_endpoint() {
        assert_eq!(
            zhipu_quota_base("https://api.z.ai/api/paas/v4"),
            "https://api.z.ai"
        );
    }

    #[test]
    fn zhipu_quota_base_defaults_to_en_for_unknown_url() {
        // 没有明显 Zhipu 域名特征时,默认走国际站(更通用的入口)
        assert_eq!(
            zhipu_quota_base("https://example.com/zhipu"),
            "https://api.z.ai"
        );
    }

    #[test]
    fn zhipu_quota_base_routes_uppercase_cn_url_to_cn_endpoint() {
        // 大小写不敏感:与 detect_provider 保持一致的约定,避免大写 preset URL 静默路由到国际站
        assert_eq!(
            zhipu_quota_base("HTTPS://OPEN.BIGMODEL.CN/api/paas/v4"),
            "https://open.bigmodel.cn"
        );
        assert_eq!(
            zhipu_quota_base("https://Open.BigModel.cn/api/paas/v4"),
            "https://open.bigmodel.cn"
        );
    }

    // ── 火山方舟 Agent Plan / Coding Plan ──

    #[test]
    fn volcengine_afp_three_windows_from_official_example() {
        // 官方文档 GetAFPUsage 返回示例（逐字）：5h 25% / weekly 30% / monthly
        // 42.525%；AFPDaily 被控制台隐藏，应跳过。
        let result = json!({
            "PlanType": "Large",
            "AFPFiveHour": { "Quota": 50.0,   "Used": 12.5,  "SubscribeTime": 1778788800000_i64, "ResetTime": 1778806800000_i64 },
            "AFPDaily":    { "Quota": 100.0,  "Used": 22.5,  "SubscribeTime": 1778716800000_i64, "ResetTime": 1778803200000_i64 },
            "AFPWeekly":   { "Quota": 500.0,  "Used": 150.0, "SubscribeTime": 1778457600000_i64, "ResetTime": 1779062400000_i64 },
            "AFPMonthly":  { "Quota": 2000.0, "Used": 850.5, "SubscribeTime": 1777939200000_i64, "ResetTime": 1780531200000_i64 }
        });
        let tiers = parse_afp_tiers(&result);
        assert_eq!(tiers.len(), 3, "daily 应被跳过，只剩 5h/周/月");
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 25.0).abs() < 1e-9);
        assert!(tiers[0].resets_at.is_some());
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert!((tiers[1].utilization - 30.0).abs() < 1e-9);
        assert_eq!(tiers[2].name, TIER_MONTHLY);
        assert!((tiers[2].utilization - 42.525).abs() < 1e-9);
        assert!(tiers[2].resets_at.is_some());
    }

    #[test]
    fn volcengine_afp_zero_quota_windows_treated_as_unbound() {
        // 已鉴权但无 Agent Plan：窗口 Quota=0 → 空结果，调用方据此回落 Coding Plan。
        let result = json!({
            "PlanType": "",
            "AFPFiveHour": { "Quota": 0.0, "Used": 0.0 },
            "AFPWeekly":   { "Quota": 0.0, "Used": 0.0 },
            "AFPMonthly":  { "Quota": 0.0, "Used": 0.0 }
        });
        assert!(parse_afp_tiers(&result).is_empty());
    }

    #[test]
    fn volcengine_afp_partial_windows_only_subscribed_ones() {
        // 仅 5h 窗口有额度（缺周/月）→ 只产出一个 tier。
        let result = json!({
            "AFPFiveHour": { "Quota": 40.0, "Used": 10.0, "ResetTime": 1778806800000_i64 },
            "AFPWeekly":   { "Quota": 0.0,  "Used": 0.0 }
        });
        let tiers = parse_afp_tiers(&result);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 25.0).abs() < 1e-9);
    }

    #[test]
    fn volcengine_coding_plan_real_response_levels() {
        // 真实 GetCodingPlanUsage 响应（用户实测 2026-06-21）：字段名是 `Level`（非 `Type`），
        // 仅百分比，秒级 ResetTimestamp；session 无活跃窗口回 -1 → 无重置时间。
        let result = json!({
            "Status": "Running",
            "UpdateTimestamp": 1782053286_i64,
            "QuotaUsage": [
                { "Level": "session", "Percent": 0.0,      "ResetTimestamp": -1_i64 },
                { "Level": "weekly",  "Percent": 1.672568, "ResetTimestamp": 1782057600_i64 },
                { "Level": "monthly", "Percent": 0.836284, "ResetTimestamp": 1784303999_i64 }
            ]
        });
        let tiers = parse_coding_plan_tiers(&result);
        assert_eq!(tiers.len(), 3);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 0.0).abs() < 1e-9);
        assert!(
            tiers[0].resets_at.is_none(),
            "session ResetTimestamp=-1 应无重置时间"
        );
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert!((tiers[1].utilization - 1.672568).abs() < 1e-6);
        assert!(tiers[1].resets_at.is_some());
        assert_eq!(tiers[2].name, TIER_MONTHLY);
        assert!((tiers[2].utilization - 0.836284).abs() < 1e-6);
    }

    #[test]
    fn volcengine_coding_plan_unknown_window_skipped_and_missing_array_empty() {
        let result = json!({
            "QuotaUsage": [
                { "Level": "daily", "Percent": 9.0 },
                { "Level": "weekly", "Percent": 20.0 }
            ]
        });
        let tiers = parse_coding_plan_tiers(&result);
        assert_eq!(tiers.len(), 1, "未知 daily 窗口跳过");
        assert_eq!(tiers[0].name, TIER_WEEKLY_LIMIT);

        assert!(parse_coding_plan_tiers(&json!({})).is_empty());
    }

    #[test]
    fn volcengine_region_derivation() {
        assert_eq!(
            volcengine_region("https://ark.cn-beijing.volces.com/api/coding"),
            "cn-beijing"
        );
        // 其他 region 的数据面域名按段提取。
        assert_eq!(
            volcengine_region("https://ark.cn-shanghai.volces.com/api/coding/v3"),
            "cn-shanghai"
        );
        // 无可识别 region 段时回落默认 cn-beijing。
        assert_eq!(
            volcengine_region("https://example.com/api/coding"),
            "cn-beijing"
        );
    }

    #[test]
    fn volcengine_canonical_query_is_sorted_and_encoded() {
        // 按 key 字母序：Action < Region < Version；值含 `-` 属 unreserved，不编码。
        assert_eq!(
            volcengine_canonical_query("GetAFPUsage", "cn-beijing"),
            "Action=GetAFPUsage&Region=cn-beijing&Version=2024-01-01"
        );
    }

    #[test]
    fn volcengine_sign_structure_and_determinism() {
        // 没有服务端金标准向量时，锁定签名的结构契约 + 确定性（足以抓住 header 顺序、
        // scope 后缀、algorithm 前缀、空 body hash 等实现错误）。真实正确性靠用户实测。
        let now = chrono::DateTime::parse_from_rfc3339("2024-06-21T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let region = "cn-beijing";
        let query = volcengine_canonical_query("GetAFPUsage", region);
        let (auth, x_date, x_content) =
            volcengine_sign("AKLTtest", "secretkey", region, &query, b"", now);

        // 空 body 的 SHA-256（固定值），证明走的是空 body。
        assert_eq!(
            x_content,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // X-Date 形如 20240621T000000Z。
        assert_eq!(x_date, "20240621T000000Z");
        // Authorization 结构：算法无 AWS4 前缀、scope 结尾 ark/request、固定 SignedHeaders。
        assert!(
            auth.starts_with("HMAC-SHA256 Credential=AKLTtest/20240621/cn-beijing/ark/request,"),
            "unexpected credential/scope: {auth}"
        );
        assert!(
            auth.contains("SignedHeaders=host;x-date;x-content-sha256;content-type,"),
            "unexpected signed headers: {auth}"
        );
        // Signature 是 64 位十六进制。
        let sig = auth.rsplit("Signature=").next().unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.bytes().all(|b| b.is_ascii_hexdigit()));

        // 确定性：同输入同输出。
        let (auth2, _, _) = volcengine_sign("AKLTtest", "secretkey", region, &query, b"", now);
        assert_eq!(auth, auth2);
    }

    #[test]
    fn volcengine_auth_error_code_detection_and_extraction() {
        assert!(volcengine_is_auth_error_code("AccessDenied"));
        assert!(volcengine_is_auth_error_code("SignatureDoesNotMatch"));
        assert!(volcengine_is_auth_error_code("InvalidAuthorization"));
        assert!(volcengine_is_auth_error_code("Unauthorized"));
        assert!(!volcengine_is_auth_error_code("InvalidParameter.Action"));
        assert!(!volcengine_is_auth_error_code("InternalError"));

        // ResponseMetadata.Error 抽取
        let body = json!({
            "ResponseMetadata": { "RequestId": "x", "Error": { "Code": "AccessDenied", "Message": "no permission" } }
        });
        let (code, msg) = volcengine_response_error(&body).expect("应抽到 Error");
        assert_eq!(code, "AccessDenied");
        assert_eq!(msg, "no permission");

        // 无 Error 时返回 None
        let ok_body = json!({ "ResponseMetadata": { "RequestId": "x" }, "Result": {} });
        assert!(volcengine_response_error(&ok_body).is_none());
    }

    // ── 传输层错误通道语义：瞬时 → Err（前端 reject/retry），确定性 → Ok(success:false) ──
    //
    // 借 ZenMux 分支可指向任意 base_url 的特性，用本地 listener 驱动真实 HTTP
    // 路径，锁定 send 失败 / 读体中断 / 4xx / 非法 JSON 各自落在哪条通道。
    // balance / subscription 服务与本文件共用同一折叠模式，这里的用例同时充当
    // 三个服务的语义回归锚。

    use super::get_coding_plan_quota;
    use crate::services::subscription::CredentialStatus;
    use std::io::{Read, Write};

    /// 测试进程内可能有其他用例临时 set_var HTTP_PROXY（http_client 的
    /// loopback 检测测试），NO_PROXY 保证本地回环请求始终直连。
    fn ensure_no_proxy_for_loopback() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("no_proxy", "127.0.0.1,localhost");
        });
    }

    /// 起一个只服务一次连接的本地 HTTP server。`response=None` 表示读完请求
    /// 直接断开（模拟响应前连接中断）。返回可命中 ZenMux 分支的 base_url。
    fn spawn_once_server(response: Option<String>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let port = listener.local_addr().expect("local addr").port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                if let Some(resp) = response {
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.flush();
                }
            }
        });
        (format!("http://127.0.0.1:{port}/zenmux"), handle)
    }

    fn http_response(status_line: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn transient_connection_refused_returns_err() {
        ensure_no_proxy_for_loopback();
        // 绑定后立刻释放端口 → 连接被拒（send 失败）
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);

        let result =
            get_coding_plan_quota(&format!("http://127.0.0.1:{port}/zenmux"), "k", None, None)
                .await;
        let err = result.expect_err("send 失败必须走 Err 通道（瞬时，前端 reject 后重试）");
        assert!(err.contains("Network error"), "err={err}");
    }

    #[tokio::test]
    async fn transient_connection_closed_before_response_returns_err() {
        ensure_no_proxy_for_loopback();
        let (base_url, handle) = spawn_once_server(None);

        let result = get_coding_plan_quota(&base_url, "k", None, None).await;
        let err = result.expect_err("响应前连接中断必须走 Err 通道（瞬时）");
        assert!(err.contains("Network error"), "err={err}");
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn transient_truncated_body_returns_err() {
        ensure_no_proxy_for_loopback();
        // 声明 content-length: 100 但只写一小段就断开 → 读体中断。
        // 锁定 bytes() 先于解析：这类失败必须走 Err（瞬时），不能因 reqwest 把
        // 读体错误包成 decode 而被误判成确定性的 "Failed to parse response"。
        let (base_url, handle) = spawn_once_server(Some(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\r\npartial"
                .to_string(),
        ));

        let result = get_coding_plan_quota(&base_url, "k", None, None).await;
        let err = result.expect_err("读体中断必须走 Err 通道（瞬时，前端 reject 后重试）");
        assert!(err.contains("Failed to read response"), "err={err}");
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn deterministic_http_401_stays_ok_with_auth_error() {
        ensure_no_proxy_for_loopback();
        let (base_url, handle) = spawn_once_server(Some(http_response("401 Unauthorized", "{}")));

        let quota = get_coding_plan_quota(&base_url, "k", None, None)
            .await
            .expect("鉴权失败是确定性失败，必须保持 Ok(success:false) 展示文案");
        assert!(!quota.success);
        assert!(matches!(quota.credential_status, CredentialStatus::Expired));
        let err = quota.error.expect("应有错误文案");
        assert!(err.contains("Authentication failed (HTTP 401"), "err={err}");
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn deterministic_http_429_stays_ok_with_status_in_error() {
        ensure_no_proxy_for_loopback();
        let (base_url, handle) =
            spawn_once_server(Some(http_response("429 Too Many Requests", "slow down")));

        let quota = get_coding_plan_quota(&base_url, "k", None, None)
            .await
            .expect("非 2xx 保持 Ok(success:false)，状态码留在文案里交前端分类");
        assert!(!quota.success);
        // 前端 isTransientUsageError 靠 /http\s+(\d{3})/ 提取状态码把 429 归瞬时，
        // 文案格式是跨层契约，勿改。
        let err = quota.error.expect("应有错误文案");
        assert!(err.contains("HTTP 429"), "err={err}");
        handle.join().expect("server thread");
    }

    #[tokio::test]
    async fn deterministic_invalid_json_body_stays_ok_with_parse_error() {
        ensure_no_proxy_for_loopback();
        // 完整读到响应体但不是 JSON → is_decode → 确定性解析失败
        let (base_url, handle) = spawn_once_server(Some(http_response("200 OK", "not-json")));

        let quota = get_coding_plan_quota(&base_url, "k", None, None)
            .await
            .expect("完整但非法的响应体是确定性失败，必须保持 Ok(success:false)");
        assert!(!quota.success);
        let err = quota.error.expect("应有错误文案");
        assert!(err.contains("Failed to parse response"), "err={err}");
        handle.join().expect("server thread");
    }
}
