//! 供应商余额查询服务
//!
//! 支持 DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita AI 的账户余额查询。
//! 返回 UsageResult 格式，与现有用量系统无缝对接。

use crate::provider::{UsageData, UsageResult};
use std::time::Duration;

// ── 供应商检测 ──────────────────────────────────────────────

enum BalanceProvider {
    DeepSeek,
    StepFun,
    SiliconFlow,
    SiliconFlowEn,
    OpenRouter,
    NovitaAI,
}

fn detect_provider(base_url: &str) -> Option<BalanceProvider> {
    let url = base_url.to_lowercase();
    if url.contains("api.deepseek.com") {
        Some(BalanceProvider::DeepSeek)
    } else if url.contains("api.stepfun.ai") || url.contains("api.stepfun.com") {
        Some(BalanceProvider::StepFun)
    } else if url.contains("api.siliconflow.cn") {
        Some(BalanceProvider::SiliconFlow)
    } else if url.contains("api.siliconflow.com") {
        Some(BalanceProvider::SiliconFlowEn)
    } else if url.contains("openrouter.ai") {
        Some(BalanceProvider::OpenRouter)
    } else if url.contains("api.novita.ai") {
        Some(BalanceProvider::NovitaAI)
    } else {
        None
    }
}

fn make_error(msg: String) -> UsageResult {
    UsageResult {
        success: false,
        data: None,
        error: Some(msg),
    }
}

fn make_auth_error(status: reqwest::StatusCode) -> UsageResult {
    UsageResult {
        success: false,
        data: Some(vec![UsageData {
            plan_name: None,
            remaining: None,
            total: None,
            used: None,
            unit: None,
            is_valid: Some(false),
            invalid_message: Some(format!("Authentication failed (HTTP {status})")),
            extra: None,
        }]),
        error: Some(format!("Authentication failed (HTTP {status})")),
    }
}

// ── DeepSeek ────────────────────────────────────────────────
// GET https://api.deepseek.com/user/balance
// Response: { balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }], is_available }

async fn query_deepseek(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.deepseek.com/user/balance")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let is_available = body
        .get("is_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut data = Vec::new();

    if let Some(infos) = body.get("balance_infos").and_then(|v| v.as_array()) {
        for info in infos {
            let currency = info
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("CNY");
            let total = parse_f64_field(info, "total_balance");

            data.push(UsageData {
                plan_name: Some(currency.to_string()),
                remaining: total,
                total: None,
                used: None,
                unit: Some(currency.to_string()),
                is_valid: Some(is_available),
                invalid_message: if !is_available {
                    Some("Insufficient balance".to_string())
                } else {
                    None
                },
                extra: None,
            });
        }
    }

    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

// ── StepFun ─────────────────────────────────────────────────
// GET https://api.stepfun.com/v1/accounts
// Response: { object, type, balance, total_cash_balance, total_voucher_balance }

async fn query_stepfun(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.stepfun.com/v1/accounts")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let balance = parse_f64_field(&body, "balance").unwrap_or(0.0);

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("StepFun".to_string()),
            remaining: Some(balance),
            total: None,
            used: None,
            unit: Some("CNY".to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── SiliconFlow ─────────────────────────────────────────────
// GET https://api.siliconflow.cn/v1/user/info (or .com for EN)
// Response: { code, data: { balance, chargeBalance, totalBalance, status } }

async fn query_siliconflow(api_key: &str, is_cn: bool) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let domain = if is_cn {
        "api.siliconflow.cn"
    } else {
        "api.siliconflow.com"
    };
    let url = format!("https://{domain}/v1/user/info");

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
    };

    let total_balance = parse_f64_field(data, "totalBalance").unwrap_or(0.0);

    let unit = if is_cn { "CNY" } else { "USD" };
    let plan_name = if is_cn {
        "SiliconFlow"
    } else {
        "SiliconFlow (EN)"
    };

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(plan_name.to_string()),
            remaining: Some(total_balance),
            total: None,
            used: None,
            unit: Some(unit.to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── OpenRouter ──────────────────────────────────────────────
// GET https://openrouter.ai/api/v1/credits
// Response: { data: { total_credits, total_usage } }

async fn query_openrouter(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://openrouter.ai/api/v1/credits")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let data = body.get("data").unwrap_or(&body);
    let total_credits = parse_f64_field(data, "total_credits").unwrap_or(0.0);
    let total_usage = parse_f64_field(data, "total_usage").unwrap_or(0.0);
    let remaining = total_credits - total_usage;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("OpenRouter".to_string()),
            remaining: Some(remaining),
            total: Some(total_credits),
            used: Some(total_usage),
            unit: Some("USD".to_string()),
            is_valid: Some(remaining > 0.0),
            invalid_message: if remaining <= 0.0 {
                Some("No credits remaining".to_string())
            } else {
                None
            },
            extra: None,
        }]),
        error: None,
    }
}

// ── Novita AI ───────────────────────────────────────────────
// GET https://api.novita.ai/v3/user/balance
// Response: { availableBalance, cashBalance, creditLimit, outstandingInvoices }
// 金额单位：0.0001 USD

async fn query_novita(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.novita.ai/v3/user/balance")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    // Novita 金额单位为 0.0001 USD，需除以 10000 转为 USD
    let available = parse_f64_field(&body, "availableBalance").unwrap_or(0.0) / 10000.0;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("Novita AI".to_string()),
            remaining: Some(available),
            total: None,
            used: None,
            unit: Some("USD".to_string()),
            is_valid: Some(available > 0.0),
            invalid_message: if available <= 0.0 {
                Some("No balance remaining".to_string())
            } else {
                None
            },
            extra: None,
        }]),
        error: None,
    }
}

// ── 工具函数 ────────────────────────────────────────────────

/// 解析 JSON 字段为 f64，兼容数字和字符串格式
fn parse_f64_field(obj: &serde_json::Value, field: &str) -> Option<f64> {
    obj.get(field).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

// ── 公开入口 ────────────────────────────────────────────────

pub async fn get_balance(base_url: &str, api_key: &str) -> Result<UsageResult, String> {
    if api_key.trim().is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some("API key is empty".to_string()),
        });
    }

    let provider = match detect_provider(base_url) {
        Some(p) => p,
        None => {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("Unknown balance provider".to_string()),
            })
        }
    };

    let result = match provider {
        BalanceProvider::DeepSeek => query_deepseek(api_key).await,
        BalanceProvider::StepFun => query_stepfun(api_key).await,
        BalanceProvider::SiliconFlow => query_siliconflow(api_key, true).await,
        BalanceProvider::SiliconFlowEn => query_siliconflow(api_key, false).await,
        BalanceProvider::OpenRouter => query_openrouter(api_key).await,
        BalanceProvider::NovitaAI => query_novita(api_key).await,
    };

    Ok(result)
}
