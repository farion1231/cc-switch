//! 国产 Token Plan 额度查询服务
//!
//! 支持 Kimi For Coding、智谱 GLM、MiniMax 的 Token Plan 额度查询。
//! 复用 subscription 模块的 SubscriptionQuota / QuotaTier 类型。

use super::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};
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

    // 总体用量（周限额）
    if let Some(usage) = body.get("usage") {
        let used = usage.get("used").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let limit = usage.get("limit").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let resets_at = usage
            .get("reset_at")
            .or_else(|| usage.get("resetAt"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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

    // 会话限额（5 小时窗口）
    if let Some(limits) = body.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            if let Some(detail) = limit_item.get("detail") {
                let used = detail.get("used").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let limit = detail.get("limit").and_then(|v| v.as_f64()).unwrap_or(1.0);
                let resets_at = detail
                    .get("reset_at")
                    .or_else(|| detail.get("resetAt"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let utilization = if limit > 0.0 {
                    (used / limit) * 100.0
                } else {
                    0.0
                };
                tiers.push(QuotaTier {
                    name: "session_limit".to_string(),
                    utilization,
                    resets_at,
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

// ── 智谱 GLM ────────────────────────────────────────────────

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

    let mut tiers = Vec::new();

    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            let limit_type = limit_item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let percentage = limit_item
                .get("percentage")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let next_reset = limit_item
                .get("nextResetTime")
                .and_then(|v| v.as_i64())
                .and_then(millis_to_iso8601);

            let tier_name = match limit_type {
                "TOKENS_LIMIT" => "tokens_limit",
                "TIME_LIMIT" => "mcp_limit",
                _ => continue,
            };

            tiers.push(QuotaTier {
                name: tier_name.to_string(),
                utilization: percentage,
                resets_at: next_reset,
            });
        }
    }

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
        for item in model_remains {
            let model_name = item
                .get("model_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let total = item
                .get("current_interval_total_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            // 注意：current_interval_usage_count 名字有误导，实际是"剩余量"
            let remaining = item
                .get("current_interval_usage_count")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let end_time = item.get("end_time").and_then(|v| v.as_i64());

            let utilization = if total > 0.0 {
                ((total - remaining) / total) * 100.0
            } else {
                0.0
            };

            tiers.push(QuotaTier {
                name: model_name.to_string(),
                utilization,
                resets_at: end_time.and_then(millis_to_iso8601),
            });
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
