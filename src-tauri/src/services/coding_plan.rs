//! 国产 Token Plan 额度查询服务
//!
//! 支持 Kimi For Coding、智谱 GLM、MiniMax 的 Token Plan 额度查询。
//! 复用 subscription 模块的 SubscriptionQuota / QuotaTier 类型。

use super::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_MONTHLY,
    TIER_WEEKLY_LIMIT,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 供应商检测 ──────────────────────────────────────────────

enum CodingPlanProvider {
    Kimi,
    ZhipuCn,
    ZhipuEn,
    MiniMaxCn,
    MiniMaxEn,
    ZenMux,
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
    } else if url.contains("ark.cn-beijing.volces.com/api/coding") {
        // 精确到 /api/coding：同 host 下还有 DouBaoSeed（/api/compatible）等其它火山产品，
        // 裸 host 会误判。管控面 GetCodingPlanUsage 用 AK/SK 签名，与编码用的 Ark API Key 无关。
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
        .timeout(std::time::Duration::from_secs(15))
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

async fn query_zhipu(base_url: &str, api_key: &str) -> SubscriptionQuota {
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
        .timeout(std::time::Duration::from_secs(15))
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

    // 提取纯函数便于无 mock 单元测试;新接口直接给"剩余百分比",反转为已用百分比
    let tiers = parse_minimax_tiers(&body);

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

// ── ZenMux ──────────────────────────────────────────────────

async fn query_zenmux(base_url: &str, api_key: &str) -> SubscriptionQuota {
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
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return make_error(format!("API error: {msg}"));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
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

    SubscriptionQuota {
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
    }
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

// ── 火山方舟 Coding Plan（AK/SK HMAC-SHA256 签名） ─────────

/// 火山方舟管控面 API host。注意编码用的是 `ark.cn-beijing.volces.com`，
/// 但 GetCodingPlanUsage 这类管控面接口必须打到 `volcengineapi.com`，
/// 且需 AK/SK 签名（Ark API Key 只能调数据面，调管控面会 401）。
const VOLCENGINE_ARK_HOST: &str = "ark.cn-beijing.volcengineapi.com";
const VOLCENGINE_ARK_REGION: &str = "cn-beijing";
const VOLCENGINE_ARK_SERVICE: &str = "ark";

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256：key 为任意字节，msg 为 UTF-8 字符串。
fn hmac_sha256_bytes(key: &[u8], msg: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// 字节数组转小写十六进制串（对应 Python `hashlib.hexdigest()`）。
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// SHA-256 摘要的十六进制串。
fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex_lower(&hasher.finalize())
}

/// RFC 3986 percent-encoding，仅保留 unreserved（`A-Za-z0-9-_.~`），
/// 与 Python `urllib.parse.quote(s, safe="-_.~")` 一致。非 unreserved 字节
/// 编码为大写 `%XX`。
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// 规范化 query string：每个键/值 percent-encode 后按键升序拼接。
fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (percent_encode(k), percent_encode(v)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// 签名 key 派生链：`sk → (short_date) → region → service → "request"`，
/// 每步 HMAC-SHA256。对照 volcenginesdkcore.SignerV4。
fn volc_signing_key(sk: &str, short_date: &str, region: &str, service: &str) -> Vec<u8> {
    let mut k = hmac_sha256_bytes(sk.as_bytes(), short_date);
    k = hmac_sha256_bytes(&k, region);
    k = hmac_sha256_bytes(&k, service);
    hmac_sha256_bytes(&k, "request")
}

/// 计算签名请求所需的 headers（不含 Host——reqwest 按目标 URL 自动设置，
/// 其值与下方 canonical 计算用的 host 一致）。
///
/// 时间作为显式参数传入，便于单元测试做确定性断言。运行时入口
/// [`sign_volcengine_coding_plan`] 用 `Utc::now()` 调用本函数。
fn build_signed_headers(
    now: DateTime<Utc>,
    host: &str,
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    body: &str,
    ak: &str,
    sk: &str,
    region: &str,
    service: &str,
) -> Vec<(String, String)> {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = &x_date[..8];
    let content_type = "application/json; charset=UTF-8";
    let body_hash = sha256_hex(body);

    let signed_str = format!(
        "content-type:{content_type}\nhost:{host}\nx-content-sha256:{body_hash}\nx-date:{x_date}\n"
    );
    let signed_headers = "content-type;host;x-content-sha256;x-date";
    let canonical_request = [
        method.to_string(),
        path.to_string(),
        canonical_query(query),
        signed_str,
        signed_headers.to_string(),
        body_hash.clone(),
    ]
    .join("\n");
    let credential_scope = format!("{short_date}/{region}/{service}/request");
    let string_to_sign = [
        "HMAC-SHA256".to_string(),
        x_date.clone(),
        credential_scope.clone(),
        sha256_hex(&canonical_request),
    ]
    .join("\n");

    let skey = volc_signing_key(sk, short_date, region, service);
    let signature = hex_lower(&hmac_sha256_bytes(&skey, &string_to_sign));

    vec![
        ("Content-Type".to_string(), content_type.to_string()),
        ("X-Date".to_string(), x_date),
        ("X-Content-Sha256".to_string(), body_hash),
        (
            "Authorization".to_string(),
            format!(
                "HMAC-SHA256 Credential={ak}/{credential_scope}, \
                 SignedHeaders={signed_headers}, Signature={signature}"
            ),
        ),
    ]
}

/// 运行时签名入口：固定目标为 GetCodingPlanUsage。
fn sign_volcengine_coding_plan(body: &str, ak: &str, sk: &str) -> Vec<(String, String)> {
    build_signed_headers(
        Utc::now(),
        VOLCENGINE_ARK_HOST,
        "POST",
        "/",
        &[("Action", "GetCodingPlanUsage"), ("Version", "2024-01-01")],
        body,
        ak,
        sk,
        VOLCENGINE_ARK_REGION,
        VOLCENGINE_ARK_SERVICE,
    )
}

/// 解析 GetCodingPlanUsage 响应为 tier 列表。
///
/// 兼容两种实测响应形态：`QuotaUsage` 直接在根，或包在 `Result` 下。
/// Level 取值 `session`(5h) / `weekly`(周) / `monthly`(月)，未知 Level 跳过。
/// 固定展示顺序：5 小时 → 周 → 月。
fn parse_volcengine_tiers(body: &serde_json::Value) -> Vec<QuotaTier> {
    let result = body
        .get("Result")
        .filter(|v| v.is_object())
        .unwrap_or(body);
    // QuotaUsage 优先取 Result 下；若 Result 存在却为空，回退根级（混合形态兜底）。
    let quota_usage = result
        .get("QuotaUsage")
        .and_then(|v| v.as_array())
        .or_else(|| body.get("QuotaUsage").and_then(|v| v.as_array()));

    let mut five_hour: Option<(f64, Option<String>)> = None;
    let mut weekly: Option<(f64, Option<String>)> = None;
    let mut monthly: Option<(f64, Option<String>)> = None;
    if let Some(items) = quota_usage {
        for item in items {
            if !item.is_object() {
                continue;
            }
            let level = item.get("Level").and_then(|v| v.as_str()).unwrap_or("");
            let percent = item.get("Percent").and_then(parse_f64).unwrap_or(0.0);
            // ResetTimestamp 是 epoch 秒；extract_reset_time 已处理秒/毫秒。
            let resets_at = item.get("ResetTimestamp").and_then(extract_reset_time);
            let entry = (percent, resets_at);
            match level {
                "session" if five_hour.is_none() => five_hour = Some(entry),
                "weekly" if weekly.is_none() => weekly = Some(entry),
                "monthly" if monthly.is_none() => monthly = Some(entry),
                _ => {}
            }
        }
    }

    let mut tiers = Vec::new();
    for (name, slot) in [
        (TIER_FIVE_HOUR, five_hour),
        (TIER_WEEKLY_LIMIT, weekly),
        (TIER_MONTHLY, monthly),
    ] {
        if let Some((percent, resets_at)) = slot {
            tiers.push(QuotaTier {
                name: name.to_string(),
                utilization: percent,
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }
    tiers
}

async fn query_volcengine(access_key_id: &str, secret_access_key: &str) -> SubscriptionQuota {
    let client = crate::proxy::http_client::get();
    let body = "{}";
    let url = format!(
        "https://{VOLCENGINE_ARK_HOST}/?Action=GetCodingPlanUsage&Version=2024-01-01"
    );
    let headers = sign_volcengine_coding_plan(body, access_key_id, secret_access_key);

    let mut req = client.post(&url).body(body.to_string());
    for (k, v) in &headers {
        req = req.header(k, v);
    }
    let resp = req.timeout(std::time::Duration::from_secs(15)).send().await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        // 签名错误 / AK 无效 / SK 错误都可能落在 400/403，一并提示凭证问题并回显 body。
        let body_text = resp.text().await.unwrap_or_default();
        return SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid Access Key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status}): {body_text}")),
            queried_at: Some(now_millis()),
        };
    }

    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body_text}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let tiers = parse_volcengine_tiers(&body);

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
    access_key_id: Option<&str>,
    secret_access_key: Option<&str>,
) -> Result<SubscriptionQuota, String> {
    if api_key.trim().is_empty() {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::NotFound,
            credential_message: None,
            success: false,
            tiers: vec![],
            extra_usage: None,
            // 与 balance::get_balance 一致：给出明确错误，避免 footer 显示无信息的失败
            error: Some("API key is empty".to_string()),
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
                // 域名未命中已知套餐供应商（如第三方中转站）：给出明确错误而非静默失败
                error: Some("Unknown coding plan provider".to_string()),
                queried_at: None,
            });
        }
    };

    let quota = match provider {
        CodingPlanProvider::Kimi => query_kimi(api_key).await,
        CodingPlanProvider::ZhipuCn | CodingPlanProvider::ZhipuEn => {
            query_zhipu(base_url, api_key).await
        }
        CodingPlanProvider::MiniMaxCn => query_minimax(api_key, true).await,
        CodingPlanProvider::MiniMaxEn => query_minimax(api_key, false).await,
        CodingPlanProvider::ZenMux => query_zenmux(base_url, api_key).await,
        CodingPlanProvider::Volcengine => {
            // 火山方舟用量查询用 AK/SK 签名，与供应商编码用的 Ark API Key 是两套凭证。
            let ak = access_key_id.unwrap_or("").trim();
            let sk = secret_access_key.unwrap_or("").trim();
            if ak.is_empty() || sk.is_empty() {
                return Ok(SubscriptionQuota {
                    tool: "coding_plan".to_string(),
                    credential_status: CredentialStatus::NotFound,
                    credential_message: None,
                    success: false,
                    tiers: vec![],
                    extra_usage: None,
                    error: Some(
                        "Volcengine Access Key ID and Secret Access Key are required"
                            .to_string(),
                    ),
                    queried_at: None,
                });
            }
            query_volcengine(ak, sk).await
        }
    };

    Ok(quota)
}

#[cfg(test)]
mod tests {
    use super::{
        build_signed_headers, canonical_query, parse_minimax_tiers, parse_volcengine_tiers,
        parse_zhipu_token_tiers, percent_encode, zhipu_quota_base, TIER_FIVE_HOUR,
        TIER_MONTHLY, TIER_WEEKLY_LIMIT,
    };
    use chrono::{DateTime, Utc};
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

    // ── 火山方舟 ──

    #[test]
    fn volcengine_three_windows_from_root_level_quota_usage() {
        // 设计文档实测样本：QuotaUsage 直接在根，Percent 为已用百分比，
        // ResetTimestamp 为 epoch 秒。session→5h、weekly→周、monthly→月。
        let body = json!({
            "Status": "Running",
            "UpdateTimestamp": 1781674850,
            "QuotaUsage": [
                { "Level": "session",  "Percent": 4.103,  "ResetTimestamp": 1781690434 },
                { "Level": "weekly",   "Percent": 0.547,  "ResetTimestamp": 1782057600 },
                { "Level": "monthly",  "Percent": 0.2735, "ResetTimestamp": 1784303999 }
            ]
        });
        let tiers = parse_volcengine_tiers(&body);
        assert_eq!(tiers.len(), 3);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 4.103).abs() < 1e-9);
        assert!(tiers[0].resets_at.is_some());
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert!((tiers[1].utilization - 0.547).abs() < 1e-9);
        assert_eq!(tiers[2].name, TIER_MONTHLY);
        assert!((tiers[2].utilization - 0.2735).abs() < 1e-9);
    }

    #[test]
    fn volcengine_quota_usage_under_result_wrapper() {
        // provider.py 读 Result.QuotaUsage：包在 Result 下也要识别。
        let body = json!({
            "Result": {
                "QuotaUsage": [
                    { "Level": "session", "Percent": 12.5 },
                    { "Level": "weekly",  "Percent": 3.0 }
                ]
            }
        });
        let tiers = parse_volcengine_tiers(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 12.5).abs() < 1e-9);
        assert!(tiers[0].resets_at.is_none());
        assert_eq!(tiers[1].name, TIER_WEEKLY_LIMIT);
        assert!((tiers[1].utilization - 3.0).abs() < 1e-9);
    }

    #[test]
    fn volcengine_unknown_level_skipped_and_fixed_order() {
        // 未知 Level 跳过；即便乱序输入，仍按 5h→周→月 固定顺序输出。
        let body = json!({
            "QuotaUsage": [
                { "Level": "monthly", "Percent": 9.0 },
                { "Level": "unknown", "Percent": 99.0 },
                { "Level": "session", "Percent": 1.0 }
            ]
        });
        let tiers = parse_volcengine_tiers(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, TIER_FIVE_HOUR);
        assert!((tiers[0].utilization - 1.0).abs() < 1e-9);
        assert_eq!(tiers[1].name, TIER_MONTHLY);
        assert!((tiers[1].utilization - 9.0).abs() < 1e-9);
    }

    #[test]
    fn volcengine_reset_timestamp_seconds_parsed_as_iso8601() {
        // ResetTimestamp=1781690434（epoch 秒）应转为 ISO 8601，不按毫秒误判。
        let body = json!({
            "QuotaUsage": [{ "Level": "session", "Percent": 1.0, "ResetTimestamp": 1781690434 }]
        });
        let tiers = parse_volcengine_tiers(&body);
        assert_eq!(tiers.len(), 1);
        let resets_at = tiers[0].resets_at.as_ref().expect("reset time present");
        // 秒级时间戳 1781690434 → 2026-06-...；若误按毫秒会得到 1970-01-21。
        assert!(resets_at.starts_with("2026-"));
    }

    #[test]
    fn volcengine_percent_encode_matches_unreserved_set() {
        // unreserved（A-Za-z0-9-_.~）原样保留，其余编码为大写 %XX。
        assert_eq!(percent_encode("GetCodingPlanUsage"), "GetCodingPlanUsage");
        assert_eq!(percent_encode("2024-01-01"), "2024-01-01");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_encode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn volcengine_canonical_query_sorted_and_encoded() {
        let cq = canonical_query(&[("Version", "2024-01-01"), ("Action", "GetCodingPlanUsage")]);
        // 按键字典序：Action 在 Version 前
        assert_eq!(cq, "Action=GetCodingPlanUsage&Version=2024-01-01");
    }

    #[test]
    fn volcengine_signing_matches_reference_algorithm() {
        // 固定时间 + 固定 AK/SK/body，断言与参考 Python 实现
        // (ai-plan-insight providers/volcengine_signing.py) 产出完全一致的签名。
        // 预期值由 /tmp/volc_sign_ref.py 以同一输入（now=2024-01-02T03:04:05Z,
        // ak=AKTEST, sk=SKTEST, body="{}"）算出。
        let now: DateTime<Utc> =
            DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z").unwrap().with_timezone(&Utc);
        let headers = build_signed_headers(
            now,
            "ark.cn-beijing.volcengineapi.com",
            "POST",
            "/",
            &[("Action", "GetCodingPlanUsage"), ("Version", "2024-01-01")],
            "{}",
            "AKTEST",
            "SKTEST",
            "cn-beijing",
            "ark",
        );
        let map: std::collections::HashMap<String, String> = headers.into_iter().collect();
        assert_eq!(map.get("X-Date").map(String::as_str), Some("20240102T030405Z"));
        assert_eq!(
            map.get("X-Content-Sha256").map(String::as_str),
            Some("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a")
        );
        assert_eq!(
            map.get("Authorization").map(String::as_str),
            Some(
                "HMAC-SHA256 Credential=AKTEST/20240102/cn-beijing/ark/request, \
                 SignedHeaders=content-type;host;x-content-sha256;x-date, \
                 Signature=68cb581113e8ce19462c02b6ca1bc1379b0bf5e5a9fad26df7d24f93d3d8a698"
            )
        );
    }
}
