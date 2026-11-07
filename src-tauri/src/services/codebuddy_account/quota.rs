//! CodeBuddy 账号的套餐与积分（只读查询）。
//!
//! CodeBuddy Code CLI 自己不显示额度；同一套账号体系下 WorkBuddy / CodeBuddy 桌面端读额度用的是
//! `POST {站点}/billing/meter/get-user-resource-summary`（个人）和
//! `POST {站点}/v2/billing/meter/get-enterprise-user-usage`（企业），两个都是空 body 的查询。
//! 站点按凭据里的 `auth.domain` 定（国内版 `www.codebuddy.cn`，国际版 `www.codebuddy.ai`），
//! 请求头和 CLI 一致：`Authorization: Bearer`、`X-User-Id`、`X-Domain`、`X-Product`
//! （不带 `X-Product` 会被拒成「请求不合法」）。
//!
//! 规矩：只用账号副本里现成的 accessToken 查，绝不续期（续期会让 CodeBuddy 手里的 refreshToken
//! 失效）；令牌过期或 401 时返回对应状态，等 CodeBuddy 自己续期或用户重新登录。令牌不进日志。

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::services::subscription::CredentialStatus;

/// 同一个账号查过之后这么久内直接用缓存（手动刷新除外）。
pub(super) const QUOTA_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 256 * 1024;

const PERSONAL_SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const ENTERPRISE_USAGE_PATH: &str = "/v2/billing/meter/get-enterprise-user-usage";

/// 只往这些站点发令牌（`auth.domain` 的后缀）。
const ALLOWED_DOMAIN_SUFFIXES: &[&str] = &[
    "codebuddy.cn",
    "codebuddy.ai",
    "workbuddy.cn",
    "copilot.tencent.com",
    "copilot-staging.tencent.com",
    "copilot.qq.com",
    "copilot-staging.qq.com",
];

/// 一个账号的套餐与积分。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodebuddyQuota {
    pub credential_status: CredentialStatus,
    pub success: bool,
    /// 套餐名（后端按 Accept-Language 给的，如「体验版」）；企业账号为 `None`
    pub plan_name: Option<String>,
    pub is_paid: bool,
    pub enterprise: bool,
    /// 企业额度不限量
    pub unlimited: bool,
    /// 本周期积分：总量 / 剩余 / 已用
    pub total: f64,
    pub remaining: f64,
    pub used: f64,
    pub error: Option<String>,
    pub queried_at: Option<i64>,
}

impl CodebuddyQuota {
    pub(super) fn failed(status: CredentialStatus, message: impl Into<String>) -> Self {
        Self {
            credential_status: status,
            success: false,
            plan_name: None,
            is_paid: false,
            enterprise: false,
            unlimited: false,
            total: 0.0,
            remaining: 0.0,
            used: 0.0,
            error: Some(message.into()),
            queried_at: Some(chrono::Utc::now().timestamp_millis()),
        }
    }

    fn ok(credits: Credits) -> Self {
        Self {
            credential_status: CredentialStatus::Valid,
            success: true,
            plan_name: credits.plan_name,
            is_paid: credits.is_paid,
            enterprise: credits.enterprise,
            unlimited: credits.unlimited,
            total: credits.total,
            remaining: credits.remaining,
            used: credits.used,
            error: None,
            queried_at: Some(chrono::Utc::now().timestamp_millis()),
        }
    }
}

/// 查额度要用到的凭据，从账号副本里取。
pub(super) struct QuotaCredentials {
    pub access_token: String,
    pub uid: String,
    pub domain: Option<String>,
    pub enterprise_id: Option<String>,
    pub access_expires_at: Option<i64>,
    pub refresh_expires_at: Option<i64>,
}

impl std::fmt::Debug for QuotaCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 令牌不打出来
        f.debug_struct("QuotaCredentials")
            .field("uid", &self.uid)
            .field("domain", &self.domain)
            .field("enterprise_id", &self.enterprise_id)
            .finish_non_exhaustive()
    }
}

pub(super) fn credentials_of(raw: &str) -> Option<QuotaCredentials> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let auth = value.get("auth")?.as_object()?;
    let account = value.get("account")?.as_object()?;
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let uid = match account.get("uid")? {
        Value::String(uid) if !uid.trim().is_empty() => uid.trim().to_string(),
        Value::Number(uid) => uid.to_string(),
        _ => return None,
    };
    let access_expires_at = super::timestamp_ms(auth.get("expiresAt")).or_else(|| {
        let refreshed = super::timestamp_ms(auth.get("lastRefreshTime"))?;
        let seconds = super::number(auth.get("expiresIn"))?;
        Some(refreshed + (seconds * 1000.0) as i64)
    });
    let refresh_expires_at = super::timestamp_ms(auth.get("refreshExpiresAt")).or_else(|| {
        let refreshed = super::timestamp_ms(auth.get("lastRefreshTime"))?;
        let seconds = super::number(auth.get("refreshExpiresIn"))?;
        Some(refreshed + (seconds * 1000.0) as i64)
    });
    Some(QuotaCredentials {
        access_token: text(auth.get("accessToken"))?,
        uid,
        domain: text(auth.get("domain")),
        enterprise_id: text(account.get("enterpriseId")),
        access_expires_at,
        refresh_expires_at,
    })
}

/// 令牌已经过期时不发请求：refresh 也过期了就要重新登录；只是 access 过期，CodeBuddy 下次
/// 用这个号时会自己续期。
pub(super) fn expired_status(creds: &QuotaCredentials, now_ms: i64) -> Option<CredentialStatus> {
    if super::is_expired(creds.refresh_expires_at, now_ms) {
        return Some(CredentialStatus::Expired);
    }
    if super::is_expired(creds.access_expires_at, now_ms) {
        return Some(CredentialStatus::RefreshPending);
    }
    None
}

/// `auth.domain` → 站点根地址。不认识的域名不发（令牌不往外送）。
pub(super) fn base_url(domain: Option<&str>) -> Option<String> {
    let host = domain?.trim().trim_end_matches('.').to_ascii_lowercase();
    let valid_chars = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    if host.is_empty() || !valid_chars {
        return None;
    }
    let allowed = ALLOWED_DOMAIN_SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
    allowed.then(|| format!("https://{host}"))
}

#[derive(Debug, PartialEq)]
pub(super) struct Credits {
    pub plan_name: Option<String>,
    pub is_paid: bool,
    pub enterprise: bool,
    pub unlimited: bool,
    pub total: f64,
    pub remaining: f64,
    pub used: f64,
}

/// 数字或数字字符串（后端积分给的是 `"123.45"` 这样的字符串）；认不出、负数都按 0。
fn amount(value: Option<&Value>) -> f64 {
    let parsed = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.filter(|n| n.is_finite() && *n > 0.0).unwrap_or(0.0)
}

#[derive(Debug, PartialEq)]
pub(super) enum ParseOutcome {
    Ok(Credits),
    /// 业务错误（`code != 0` 或没有 data），带后端的 msg
    Rejected(String),
}

/// 拆开 `{code, msg, data}`：`code != 0` 时返回后端的 msg；`data` 不是对象时为 `None`。
fn envelope(body: &[u8]) -> Result<Option<Value>, String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "CodeBuddy 返回的内容认不出".to_string())?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code != 0 {
        let message = value
            .get("msg")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(|msg| msg.chars().take(200).collect::<String>())
            .filter(|msg| !msg.is_empty())
            .unwrap_or_else(|| format!("code {code}"));
        return Err(message);
    }
    Ok(value.get("data").filter(|data| data.is_object()).cloned())
}

/// 个人账号：`Packages[]` 各包本周期总量 / 剩余 / 已用相加（和 WorkBuddy 的 sumSummaryCapacity
/// 一致），套餐名取 `SubscriptionPackageName`。
pub(super) fn parse_personal(body: &[u8]) -> ParseOutcome {
    let data = match envelope(body) {
        Ok(Some(data)) => data,
        Ok(None) => return ParseOutcome::Rejected("CodeBuddy 没有返回额度".into()),
        Err(message) => return ParseOutcome::Rejected(message),
    };
    let (mut total, mut remaining, mut used) = (0.0, 0.0, 0.0);
    for package in data
        .get("Packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        total += amount(package.get("CycleTotalCapacity"));
        remaining += amount(package.get("CycleRemainCapacity"));
        used += amount(package.get("CycleUsedCapacity"));
    }
    let plan_name = data
        .get("SubscriptionPackageName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    ParseOutcome::Ok(Credits {
        plan_name,
        is_paid: data
            .get("IsPaidUser")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        enterprise: false,
        unlimited: false,
        total,
        remaining,
        used,
    })
}

/// 企业账号：`limit_num`（-1 不限量）和 `used_num`；老字段 `limitNum` / `credit` 也认。
pub(super) fn parse_enterprise(body: &[u8]) -> ParseOutcome {
    let data = match envelope(body) {
        Ok(Some(data)) => data,
        Ok(None) => return ParseOutcome::Rejected("CodeBuddy 没有返回额度".into()),
        Err(message) => return ParseOutcome::Rejected(message),
    };
    let raw_limit = data
        .get("limit_num")
        .or_else(|| data.get("limitNum"))
        .and_then(|value| match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        });
    let unlimited = raw_limit.is_some_and(|limit| limit < 0.0);
    let total = if unlimited {
        0.0
    } else {
        amount(data.get("limit_num").or_else(|| data.get("limitNum")))
    };
    let used = amount(data.get("used_num").or_else(|| data.get("credit")));
    ParseOutcome::Ok(Credits {
        plan_name: None,
        is_paid: true,
        enterprise: true,
        unlimited,
        total,
        remaining: if unlimited {
            0.0
        } else {
            (total - used).max(0.0)
        },
        used,
    })
}

/// 发一次只读查询。不续期、不重试。
pub(super) async fn fetch(creds: &QuotaCredentials, lang: &str) -> CodebuddyQuota {
    let Some(base) = base_url(creds.domain.as_deref()) else {
        return CodebuddyQuota::failed(
            CredentialStatus::Valid,
            "认不出这个账号的站点，没有查询额度",
        );
    };
    let enterprise = creds.enterprise_id.is_some();
    let path = if enterprise {
        ENTERPRISE_USAGE_PATH
    } else {
        PERSONAL_SUMMARY_PATH
    };
    let mut request = crate::proxy::http_client::get()
        .post(format!("{base}{path}"))
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(&creds.access_token)
        .header("X-User-Id", &creds.uid)
        .header("X-Product", "SaaS")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_LANGUAGE, lang)
        .header(
            reqwest::header::USER_AGENT,
            format!("cc-switch/{}", env!("CARGO_PKG_VERSION")),
        )
        .json(&serde_json::json!({}));
    if let Some(domain) = creds.domain.as_deref() {
        request = request.header("X-Domain", domain);
    }
    if let Some(enterprise_id) = creds.enterprise_id.as_deref() {
        request = request
            .header("X-Enterprise-Id", enterprise_id)
            .header("X-Tenant-Id", enterprise_id);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            // reqwest 的错误里只有地址，没有请求头
            log::warn!("[CodebuddyAccount] 查询额度失败: {error}");
            return CodebuddyQuota::failed(
                CredentialStatus::Valid,
                if error.is_timeout() {
                    "查询 CodeBuddy 额度超时".to_string()
                } else {
                    "连不上 CodeBuddy，没查到额度".to_string()
                },
            );
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return CodebuddyQuota::failed(CredentialStatus::Expired, "CodeBuddy 登录已失效");
    }
    let body = match response.bytes().await {
        Ok(body) if body.len() <= MAX_BODY => body,
        Ok(_) => {
            return CodebuddyQuota::failed(CredentialStatus::Valid, "CodeBuddy 返回的内容太大")
        }
        Err(error) => {
            log::warn!("[CodebuddyAccount] 读取额度响应失败: {error}");
            return CodebuddyQuota::failed(CredentialStatus::Valid, "没读到 CodeBuddy 的响应");
        }
    };
    let outcome = if enterprise {
        parse_enterprise(&body)
    } else {
        parse_personal(&body)
    };
    match outcome {
        ParseOutcome::Ok(credits) if status.is_success() => CodebuddyQuota::ok(credits),
        ParseOutcome::Ok(_) => {
            CodebuddyQuota::failed(CredentialStatus::Valid, format!("HTTP {}", status.as_u16()))
        }
        ParseOutcome::Rejected(message) => {
            log::warn!(
                "[CodebuddyAccount] 额度查询被拒: HTTP {} {message}",
                status.as_u16()
            );
            CodebuddyQuota::failed(CredentialStatus::Valid, message)
        }
    }
}
