//! 供应商余额查询服务
//!
//! 支持 DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita AI、阿里云 BSS 的账户余额查询。
//! 返回 UsageResult 格式，与现有用量系统无缝对接。

use crate::provider::{UsageData, UsageResult};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::BTreeMap;
use std::time::Duration;

// ── 供应商检测 ──────────────────────────────────────────────

enum BalanceProvider {
    DeepSeek,
    StepFun,
    SiliconFlow,
    SiliconFlowEn,
    OpenRouter,
    NovitaAI,
    Aliyun,
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
    } else if url.contains("business.aliyuncs.com") {
        Some(BalanceProvider::Aliyun)
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

fn make_single_result(
    plan_name: &str,
    remaining: f64,
    total: Option<f64>,
    used: Option<f64>,
    unit: &str,
    extra: Option<String>,
) -> UsageResult {
    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(plan_name.to_string()),
            remaining: Some(remaining),
            total,
            used,
            unit: Some(unit.to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra,
        }]),
        error: None,
    }
}

fn aliyun_percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

fn aliyun_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn aliyun_signature_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn build_aliyun_canonical_query(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                aliyun_percent_encode(key),
                aliyun_percent_encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn sign_aliyun_request(secret_access_key: &str, canonical_query: &str) -> Result<String, String> {
    type HmacSha1 = Hmac<Sha1>;

    let string_to_sign = format!("GET&%2F&{}", aliyun_percent_encode(canonical_query));
    let key = format!("{secret_access_key}&");
    let mut mac =
        HmacSha1::new_from_slice(key.as_bytes()).map_err(|e| format!("Invalid key: {e}"))?;
    mac.update(string_to_sign.as_bytes());
    let digest = mac.finalize().into_bytes();
    Ok(base64::engine::general_purpose::STANDARD.encode(digest))
}

fn is_live_aliyun_request_enabled() -> bool {
    std::env::var_os("CC_SWITCH_LIVE_ALIYUN_REQUESTS").is_some()
}

fn in_test_harness() -> bool {
    std::env::var_os("RUST_TEST_THREADS").is_some()
}

fn should_mock_aliyun_balance_request(in_test_harness: bool, live_enabled: bool) -> bool {
    in_test_harness && !live_enabled
}

fn mocked_aliyun_balance_result() -> UsageResult {
    make_single_result(
        "Alibaba Cloud",
        123.45,
        Some(200.0),
        Some(76.55),
        "CNY",
        Some("CreditAmount=0; AvailableCashAmount=10000; QuotaLimit=200".to_string()),
    )
}

async fn query_aliyun_balance(access_key_id: &str, secret_access_key: &str) -> UsageResult {
    if should_mock_aliyun_balance_request(in_test_harness(), is_live_aliyun_request_enabled()) {
        return mocked_aliyun_balance_result();
    }

    let mut params = BTreeMap::new();
    params.insert("Action".to_string(), "QueryAccountBalance".to_string());
    params.insert("AccessKeyId".to_string(), access_key_id.to_string());
    params.insert("Format".to_string(), "JSON".to_string());
    params.insert("SignatureMethod".to_string(), "HMAC-SHA1".to_string());
    params.insert("SignatureNonce".to_string(), aliyun_signature_nonce());
    params.insert("SignatureVersion".to_string(), "1.0".to_string());
    params.insert("Timestamp".to_string(), aliyun_timestamp());
    params.insert("Version".to_string(), "2017-12-14".to_string());

    let canonical_query = build_aliyun_canonical_query(&params);
    let signature = match sign_aliyun_request(secret_access_key, &canonical_query) {
        Ok(sig) => sig,
        Err(e) => return make_error(e),
    };

    let full_url = format!(
        "https://business.aliyuncs.com/?{}&Signature={}",
        canonical_query,
        aliyun_percent_encode(&signature)
    );

    let client = crate::proxy::http_client::get();
    let resp = client
        .get(&full_url)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
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

    parse_aliyun_balance_response(body)
}

fn parse_aliyun_balance_response(body: serde_json::Value) -> UsageResult {
    let success = body
        .get("Success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        let msg = body
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("Aliyun balance query failed");
        return make_error(msg.to_string());
    }

    let data = match body.get("Data") {
        Some(v) => v,
        None => return make_error("Missing 'Data' field in response".to_string()),
    };

    let remaining = parse_f64_field(data, "AvailableCashAmount").unwrap_or(0.0);
    let credit = parse_f64_field(data, "CreditAmount");
    let cash = parse_f64_field(data, "AvailableCashAmount");
    let quota = parse_f64_field(data, "QuotaLimit");
    let currency = data
        .get("Currency")
        .and_then(|v| v.as_str())
        .unwrap_or("CNY");
    let extra = Some(format!(
        "CreditAmount={}; AvailableCashAmount={}; QuotaLimit={}",
        credit
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        cash.map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        quota
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    ));

    make_single_result("Alibaba Cloud", remaining, quota, None, currency, extra)
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
        .timeout(Duration::from_secs(15))
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
        .timeout(Duration::from_secs(15))
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
        .timeout(Duration::from_secs(15))
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
        .timeout(Duration::from_secs(15))
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
        .timeout(Duration::from_secs(15))
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

pub async fn get_balance(
    base_url: &str,
    api_key: &str,
    secret_access_key: Option<&str>,
) -> Result<UsageResult, String> {
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
        BalanceProvider::Aliyun => {
            let Some(secret_access_key) =
                secret_access_key.map(str::trim).filter(|v| !v.is_empty())
            else {
                return Ok(UsageResult {
                    success: false,
                    data: None,
                    error: Some("AccessKey Secret is empty".to_string()),
                });
            };
            query_aliyun_balance(api_key, secret_access_key).await
        }
    };

    Ok(result)
}
