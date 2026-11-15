//! 供应商余额查询服务
//!
//! 支持 DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita AI 的账户余额查询；
//! 智谱 GLM 的余额只作为套餐查询的回落（见 `query_zhipu_balance`），不参与自动检测。
//! 返回 UsageResult 格式，与现有用量系统无缝对接。
//!
//! 错误通道语义（与 coding_plan / subscription 两个服务保持一致）：
//! - `Err(String)` = 瞬时传输失败（网络不可达/超时/读体中断）。前端 invoke reject，
//!   react-query 触发 retry 并保留上一次成功的 data（天然 keep-last-good）。
//! - `Ok(success:false)` = 确定性失败（空 key/未知供应商/鉴权/非 2xx/响应体非法 JSON），
//!   立即透出错误文案。判定按 reqwest 错误种类在折叠点完成，不依赖错误文案匹配。

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

async fn query_deepseek(api_key: &str) -> Result<UsageResult, String> {
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
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    Ok(parse_deepseek_balance(&body))
}

// `is_available` 只表示余额够不够调用，不代表 Key 失效（失效是 401/403，上面已处理），
// 所以不拿它判 is_valid，否则零余额 / 欠费会被显示成「套餐已过期」；交给 UI 按余额红字显示（#8812）。
fn parse_deepseek_balance(body: &serde_json::Value) -> UsageResult {
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
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            });
        }
    }

    // 同一账号会列出 CNY / USD 两个币种钱包，扣费走有钱的那个。只要还有币种有余额，
    // 就不再列出空钱包：否则空的那一行会显示一条红字余额，两行还会让卡片自动展开（#8170）。
    if data.iter().any(|d| d.remaining.is_some_and(|v| v > 0.0)) {
        data.retain(|d| d.remaining.is_none_or(|v| v > 0.0));
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

async fn query_stepfun(api_key: &str) -> Result<UsageResult, String> {
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
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    let balance = parse_f64_field(&body, "balance").unwrap_or(0.0);

    Ok(UsageResult {
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
    })
}

// ── SiliconFlow ─────────────────────────────────────────────
// GET https://api.siliconflow.cn/v1/user/info (or .com for EN)
// Response: { code, data: { balance, chargeBalance, totalBalance, status } }

async fn query_siliconflow(api_key: &str, is_cn: bool) -> Result<UsageResult, String> {
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
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    let data = match body.get("data") {
        Some(d) => d,
        None => return Ok(make_error("Missing 'data' field in response".to_string())),
    };

    let total_balance = parse_f64_field(data, "totalBalance").unwrap_or(0.0);

    let unit = if is_cn { "CNY" } else { "USD" };
    let plan_name = if is_cn {
        "SiliconFlow"
    } else {
        "SiliconFlow (EN)"
    };

    Ok(UsageResult {
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
    })
}

// ── OpenRouter ──────────────────────────────────────────────
// GET https://openrouter.ai/api/v1/credits
// Response: { data: { total_credits, total_usage } }
// /key's limit_remaining is a spending cap, not the account credit balance.

async fn query_openrouter(api_key: &str) -> Result<UsageResult, String> {
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
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    Ok(parse_openrouter_balance(&body))
}

fn parse_openrouter_balance(body: &serde_json::Value) -> UsageResult {
    let data = body.get("data").unwrap_or(body);
    let (Some(total_credits), Some(total_usage)) = (
        parse_f64_field(data, "total_credits").filter(|v| v.is_finite()),
        parse_f64_field(data, "total_usage").filter(|v| v.is_finite()),
    ) else {
        return make_error("Missing or invalid OpenRouter credit amounts".to_string());
    };
    let remaining = total_credits - total_usage;
    if !remaining.is_finite() {
        return make_error("Invalid OpenRouter credit balance".to_string());
    }

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("OpenRouter".to_string()),
            remaining: Some(remaining),
            total: Some(total_credits),
            used: Some(total_usage),
            unit: Some("USD".to_string()),
            // /credits reports purchased credits, not key expiration. A free
            // account can have no credits and still use free models; let the
            // UI show the balance without marking the key as expired.
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── Novita AI ───────────────────────────────────────────────
// GET https://api.novita.ai/v3/user/balance
// Response: { availableBalance, cashBalance, creditLimit, outstandingInvoices }
// 金额单位：0.0001 USD

async fn query_novita(api_key: &str) -> Result<UsageResult, String> {
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
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    Ok(parse_novita_balance(&body))
}

fn parse_novita_balance(body: &serde_json::Value) -> UsageResult {
    // Novita 金额单位为 0.0001 USD，需除以 10000 转为 USD
    let Some(available) = parse_f64_field(body, "availableBalance")
        .map(|v| v / 10000.0)
        .filter(|v| v.is_finite())
    else {
        return make_error("Missing or invalid Novita AI balance".to_string());
    };

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("Novita AI".to_string()),
            remaining: Some(available),
            total: None,
            used: None,
            unit: Some("USD".to_string()),
            // 余额为 0 或负数不代表 key 失效：交给 UI 按余额红字显示，
            // 而不是标成套餐已过期（同 OpenRouter，#8175）
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── 智谱 GLM ────────────────────────────────────────────────
// GET {open.bigmodel.cn | api.z.ai}/api/biz/account/query-customer-account-report
// 没有公开文档。2026-10-18 用国内站按量 Key 实测：接受推理用的 API Key（带不带 Bearer
// 都行；同在 /api/biz/ 下的其他接口回 403 "APIKey not allow access"，这个是有意开放的），
// Response: { code, msg, success, data: { balance, availableBalance, frozenBalance, ... } }，
// 单位人民币。国际站路由存在（无效 Key 回 401 而非 404），返回结构与币种未实测：
// 按同一结构解析，币种记 USD。
//
// 只给智谱 token_plan 卡「账户没有 Coding Plan」时回落用（commands::provider），
// 不进 detect_provider：智谱卡默认注入 token_plan 模板，余额模板再自动认智谱会和它抢。

pub(crate) async fn query_zhipu_balance(
    base_url: &str,
    api_key: &str,
) -> Result<UsageResult, String> {
    let is_cn = base_url.to_lowercase().contains("bigmodel.cn");
    let host = if is_cn {
        "https://open.bigmodel.cn"
    } else {
        "https://api.z.ai"
    };
    let client = crate::proxy::http_client::get();

    let resp = client
        .get(format!(
            "{host}/api/biz/account/query-customer-account-report"
        ))
        .header("Authorization", api_key) // 与套餐接口一致，不加 Bearer
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US,en")
        .timeout(Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_auth_error(status));
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

    Ok(parse_zhipu_balance(&body, is_cn))
}

fn parse_zhipu_balance(body: &serde_json::Value, is_cn: bool) -> UsageResult {
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return make_error(format!("API error: {msg}"));
    }
    let data = body.get("data").filter(|d| d.is_object()).unwrap_or(body);
    // availableBalance 是扣掉冻结后的可用余额；没有时退回 balance。
    let Some(available) = parse_f64_field(data, "availableBalance")
        .or_else(|| parse_f64_field(data, "balance"))
        .filter(|v| v.is_finite())
    else {
        return make_error("Missing or invalid Zhipu balance".to_string());
    };

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(if is_cn { "Zhipu GLM" } else { "Z.ai" }.to_string()),
            remaining: Some(available),
            total: None,
            used: None,
            unit: Some(if is_cn { "CNY" } else { "USD" }.to_string()),
            // 余额为 0 不代表 Key 失效，交给 UI 按余额红字显示（同 OpenRouter，#8175）
            is_valid: Some(true),
            invalid_message: None,
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

/// 查询余额。瞬时传输失败返回 `Err`（前端 reject → retry + 保留上次成功值），
/// 确定性失败返回 `Ok(success:false)`（见模块级文档）。
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

    match provider {
        BalanceProvider::DeepSeek => query_deepseek(api_key).await,
        BalanceProvider::StepFun => query_stepfun(api_key).await,
        BalanceProvider::SiliconFlow => query_siliconflow(api_key, true).await,
        BalanceProvider::SiliconFlowEn => query_siliconflow(api_key, false).await,
        BalanceProvider::OpenRouter => query_openrouter(api_key).await,
        BalanceProvider::NovitaAI => query_novita(api_key).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openrouter_balance_does_not_determine_key_validity() {
        // Free accounts, exhausted paid accounts, and overdrawn accounts do not
        // imply an expired key. Positive balances must retain their amounts too.
        for (credits, usage, remaining) in [
            (0.0, 0.0, 0.0),
            (100.0, 100.0, 0.0),
            (100.0, 105.0, -5.0),
            (100.0, 25.5, 74.5),
        ] {
            let result = parse_openrouter_balance(&json!({
                "data": { "total_credits": credits, "total_usage": usage }
            }));
            assert!(result.success);
            assert!(result.error.is_none());
            let data = result.data.unwrap();
            assert_eq!(data.len(), 1);
            let balance = &data[0];
            assert_eq!(balance.plan_name.as_deref(), Some("OpenRouter"));
            assert_eq!(balance.unit.as_deref(), Some("USD"));
            assert_eq!(balance.total, Some(credits));
            assert_eq!(balance.used, Some(usage));
            assert_eq!(balance.remaining, Some(remaining));
            assert_eq!(balance.is_valid, Some(true));
            assert!(balance.invalid_message.is_none());
        }
    }

    #[test]
    fn openrouter_balance_accepts_numeric_strings_and_unwrapped_response() {
        let result = parse_openrouter_balance(&json!({
            "total_credits": "100.00", "total_usage": "25.50"
        }));
        assert!(result.success);
        let balance = result.data.unwrap().remove(0);
        assert_eq!(balance.total, Some(100.0));
        assert_eq!(balance.used, Some(25.5));
        assert_eq!(balance.remaining, Some(74.5));
    }

    #[test]
    fn openrouter_balance_rejects_missing_or_invalid_amounts() {
        for body in [
            json!({}),
            json!({ "data": null }),
            json!({ "data": { "limit": 100, "limit_remaining": 100, "is_free_tier": true } }),
            json!({ "data": { "total_credits": 100 } }),
            json!({ "data": { "total_usage": 0 } }),
            json!({ "data": { "total_credits": null, "total_usage": 0 } }),
            json!({ "data": { "total_credits": 100, "total_usage": "invalid" } }),
            json!({ "data": { "total_credits": "NaN", "total_usage": 0 } }),
            json!({ "data": { "total_credits": 100, "total_usage": "inf" } }),
            json!({ "data": { "total_credits": 1e308, "total_usage": -1e308 } }),
        ] {
            let result = parse_openrouter_balance(&body);
            assert!(!result.success, "unexpected success for {body}");
            assert!(result.data.is_none());
            assert!(result.error.is_some());
        }
    }

    #[test]
    fn deepseek_balance_hides_empty_wallets_when_another_has_funds() {
        let result = parse_deepseek_balance(&json!({
            "is_available": true,
            "balance_infos": [
                { "currency": "CNY", "total_balance": "0.00" },
                { "currency": "USD", "total_balance": "12.34" }
            ]
        }));
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].unit.as_deref(), Some("USD"));
        assert_eq!(data[0].remaining, Some(12.34));
    }

    #[test]
    fn deepseek_balance_keeps_every_wallet_when_none_or_all_have_funds() {
        for (cny, usd, expected) in [("0.00", "0.00", 2), ("5.00", "1.00", 2)] {
            let result = parse_deepseek_balance(&json!({
                "is_available": cny != "0.00",
                "balance_infos": [
                    { "currency": "CNY", "total_balance": cny },
                    { "currency": "USD", "total_balance": usd }
                ]
            }));
            assert_eq!(result.data.unwrap().len(), expected);
        }

        let result = parse_deepseek_balance(&json!({
            "balance_infos": [{ "currency": "CNY", "total_balance": "0.00" }]
        }));
        assert_eq!(result.data.unwrap()[0].remaining, Some(0.0));
    }

    #[test]
    fn deepseek_unavailable_balance_does_not_determine_key_validity() {
        for (raw, expected) in [("-24.96", -24.96), ("-0.00", 0.0)] {
            let result = parse_deepseek_balance(&json!({
                "is_available": false,
                "balance_infos": [{ "currency": "CNY", "total_balance": raw }]
            }));
            assert!(result.success);
            let balance = result.data.unwrap().remove(0);
            assert_eq!(balance.remaining, Some(expected));
            assert_eq!(balance.is_valid, Some(true));
            assert!(balance.invalid_message.is_none());
        }
    }

    #[test]
    fn novita_balance_does_not_determine_key_validity() {
        for (raw, available) in [(0.0, 0.0), (-50000.0, -5.0), (255000.0, 25.5)] {
            let result = parse_novita_balance(&json!({ "availableBalance": raw }));
            assert!(result.success);
            let balance = result.data.unwrap().remove(0);
            assert_eq!(balance.remaining, Some(available));
            assert_eq!(balance.unit.as_deref(), Some("USD"));
            assert_eq!(balance.is_valid, Some(true));
            assert!(balance.invalid_message.is_none());
        }

        let result = parse_novita_balance(&json!({ "availableBalance": "10000" }));
        assert_eq!(result.data.unwrap()[0].remaining, Some(1.0));
    }

    #[test]
    fn novita_balance_rejects_missing_or_invalid_amount() {
        for body in [
            json!({}),
            json!({ "availableBalance": null }),
            json!({ "availableBalance": "invalid" }),
            json!({ "availableBalance": "inf" }),
            json!({ "cashBalance": 10000 }),
        ] {
            let result = parse_novita_balance(&body);
            assert!(!result.success, "unexpected success for {body}");
            assert!(result.data.is_none());
            assert!(result.error.is_some());
        }
    }

    /// 2026-10-18 国内站按量 Key 实测原文。
    const ZHIPU_BALANCE_BODY: &str = r#"{"code":200,"msg":"操作成功","data":{"balance":14.68002004,"rechargeAmount":15.0,"giveAmount":0.0,"totalSpendAmount":0.31997996,"todaySpendAmount":null,"availableBalance":14.68002004,"frozenBalance":0.0,"creditBalance":null,"availableCreditBalance":null,"creditStatus":"NOT_OPEN","modelSpendAmountList":null,"isKA":false},"success":true}"#;

    #[test]
    fn zhipu_balance_shows_available_balance_in_site_currency() {
        let body: serde_json::Value = serde_json::from_str(ZHIPU_BALANCE_BODY).unwrap();
        let result = parse_zhipu_balance(&body, true);
        assert!(result.success);
        let balance = result.data.unwrap().remove(0);
        assert_eq!(balance.remaining, Some(14.68002004));
        assert_eq!(balance.unit.as_deref(), Some("CNY"));
        assert_eq!(balance.plan_name.as_deref(), Some("Zhipu GLM"));
        assert_eq!(balance.is_valid, Some(true));

        // 国际站未实测：同结构、币种记 USD；可用余额缺失时退回 balance、数字串也认
        let result = parse_zhipu_balance(
            &json!({ "success": true, "data": { "balance": "3.5", "frozenBalance": 0 } }),
            false,
        );
        let balance = result.data.unwrap().remove(0);
        assert_eq!(balance.remaining, Some(3.5));
        assert_eq!(balance.unit.as_deref(), Some("USD"));

        // 用完不算 Key 失效
        let result = parse_zhipu_balance(&json!({ "data": { "availableBalance": 0 } }), true);
        assert_eq!(result.data.unwrap()[0].is_valid, Some(true));
    }

    #[test]
    fn zhipu_balance_rejects_business_errors_and_missing_amounts() {
        for body in [
            json!({ "code": 401, "msg": "令牌已过期或验证不正确", "success": false }),
            json!({ "code": 200, "success": true, "data": null }),
            json!({ "code": 200, "success": true, "data": { "availableBalance": "inf" } }),
            json!({}),
        ] {
            let result = parse_zhipu_balance(&body, true);
            assert!(!result.success, "unexpected success for {body}");
            assert!(result.data.is_none());
            assert!(result.error.is_some());
        }
        let msg = parse_zhipu_balance(
            &json!({ "msg": "令牌已过期或验证不正确", "success": false }),
            true,
        )
        .error
        .unwrap();
        assert!(msg.contains("令牌已过期"), "{msg}");
    }
}
