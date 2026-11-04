//! Cursor 官方订阅与额度查询服务
//!
//! 自动发现并读取 Cursor 的认证凭据：
//! 1. 优先读取 Cursor SQLite 数据库中的 `cursorAuth/accessToken`：
//!    - macOS: `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
//!    - Linux: `~/.config/Cursor/User/globalStorage/state.vscdb`
//!    - Windows: `%APPDATA%/Cursor/User/globalStorage/state.vscdb`
//! 2. 次选读取本地 CLI / 桌面配置：
//!    - `~/.config/cursor/auth.json` 或 `~/.cursor/auth.json`
//! 3. 环境变量覆盖（便于调试与测试）：
//!    - `CURSOR_COOKIE`、`CURSOR_ACCESS_TOKEN`、`CURSOR_AUTH_DB`
//!
//! 解码 JWT claims 获取 `sub`（提取 `user_id`）、`exp`（校验过期时间）和 `email`。
//! 拼接 `WorkosCursorSessionToken={user_id}%3A%3A{access_token}` Cookie，调用官方接口：
//! - `GET https://cursor.com/api/usage-summary`：会员类型（Pro/Plus/Free/Team 等）、计费周期、已用/上限、使用率百分比、On-demand 按量预算
//! - `GET https://cursor.com/api/usage?user={user_id}`：获取 fast requests (gpt-4) 用量与上限

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::get_home_dir;
use crate::services::subscription::{CredentialStatus, ExtraUsage, QuotaTier, SubscriptionQuota};

pub const TOOL: &str = "cursor";
pub const TIER_FAST_REQUESTS: &str = "fast_requests";

/// Cursor 认证会话信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorSession {
    pub user_id: String,
    pub access_token: String,
    pub cookie: String,
    pub email: Option<String>,
    pub subject: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct UsageSummaryResponse {
    billing_cycle_start: Option<serde_json::Value>,
    billing_cycle_end: Option<serde_json::Value>,
    membership_type: Option<String>,
    individual_usage: Option<IndividualUsage>,
    team_usage: Option<TeamUsage>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct IndividualUsage {
    plan: Option<PlanUsage>,
    overall: Option<PlanUsage>,
    on_demand: Option<OnDemandUsage>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TeamUsage {
    pooled: Option<PlanUsage>,
    on_demand: Option<OnDemandUsage>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct PlanUsage {
    used: Option<f64>,
    limit: Option<f64>,
    total_percent_used: Option<f64>,
    auto_percent_used: Option<f64>,
    api_percent_used: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnDemandUsage {
    used: Option<f64>,
    limit: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RequestUsageResponse {
    #[serde(rename = "gpt-4")]
    gpt_4: Option<Gpt4Usage>,
    start_of_month: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Gpt4Usage {
    num_requests: Option<f64>,
    num_requests_total: Option<f64>,
    max_request_usage: Option<f64>,
}

// ── 凭据读取与 JWT 解析 ──────────────────────────────────────

/// 返回 Cursor 用户配置根目录
pub fn get_cursor_dir() -> PathBuf {
    let home = get_home_dir();
    #[cfg(target_os = "macos")]
    return home
        .join("Library")
        .join("Application Support")
        .join("Cursor");
    #[cfg(target_os = "windows")]
    return std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        .join("Cursor");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return home.join(".config").join("Cursor");
}

/// 候选的 `state.vscdb` 路径列表
pub fn cursor_auth_db_candidates() -> Vec<PathBuf> {
    if let Ok(custom) = std::env::var("CURSOR_AUTH_DB") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            return vec![p];
        }
    }

    let home = get_home_dir();
    vec![
        // macOS
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
        // Linux
        home.join(".config")
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
        // Windows
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    ]
}

/// 候选的 CLI / 独立 JSON 凭据路径列表
pub fn cursor_auth_json_candidates() -> Vec<PathBuf> {
    let home = get_home_dir();
    vec![
        home.join(".config").join("cursor").join("auth.json"),
        home.join(".cursor").join("auth.json"),
    ]
}

/// 从 SQLite 数据库读取 `cursorAuth/accessToken`
pub fn read_token_from_db(db_path: &Path) -> Result<String, String> {
    if !db_path.is_file() {
        return Err(format!(
            "Database file does not exist: {}",
            db_path.display()
        ));
    }

    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open Cursor SQLite database: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1")
        .map_err(|e| format!("Prepare query failed: {e}"))?;

    let token: String = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| format!("Failed to query cursorAuth/accessToken: {e}"))?;

    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty accessToken in database".to_string());
    }
    Ok(trimmed)
}

/// 从 JSON 文件读取 `accessToken`
pub fn read_token_from_json(json_path: &Path) -> Result<String, String> {
    if !json_path.is_file() {
        return Err(format!("File does not exist: {}", json_path.display()));
    }

    let content = std::fs::read_to_string(json_path)
        .map_err(|e| format!("Failed to read auth JSON file: {e}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse auth JSON: {e}"))?;

    let token = parsed
        .get("accessToken")
        .or_else(|| parsed.get("access_token"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "No accessToken field found in auth.json".to_string())?;

    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty accessToken in auth.json".to_string());
    }
    Ok(trimmed)
}

/// 解析 JWT Payload：提取 (user_id, email, subject, expiration)
pub fn parse_jwt_claims(token: &str) -> Result<(String, Option<String>, String, i64), String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Err("Invalid JWT format: fewer than 2 segments".to_string());
    }

    let payload_b64 = parts[1];
    let decoded = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| STANDARD_NO_PAD.decode(payload_b64))
        .or_else(|_| {
            let padded = match payload_b64.len() % 4 {
                2 => format!("{}==", payload_b64),
                3 => format!("{}=", payload_b64),
                _ => payload_b64.to_string(),
            };
            URL_SAFE
                .decode(&padded)
                .or_else(|_| STANDARD.decode(&padded))
        })
        .map_err(|e| format!("Base64 decode JWT payload failed: {e}"))?;

    let parsed: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|e| format!("Failed to deserialize JWT JSON payload: {e}"))?;

    let sub = parsed
        .get("sub")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "JWT missing 'sub' claim".to_string())?
        .trim();

    if sub.is_empty() {
        return Err("JWT 'sub' claim is empty".to_string());
    }

    let user_id = sub.rsplit('|').next().unwrap_or(sub).to_string();
    if user_id.is_empty() {
        return Err("Extracted user_id is empty".to_string());
    }

    let email = parsed
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let exp = parsed
        .get("exp")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(0);

    Ok((user_id, email, sub.to_string(), exp))
}

/// 从 Cookie 字符串解析 CursorSession
pub fn parse_cookie_session(cookie_str: &str) -> Option<CursorSession> {
    for part in cookie_str.split(';') {
        let trimmed = part.trim();
        let (name, value) = trimmed.split_once('=')?;
        if name.trim() == "WorkosCursorSessionToken" {
            let decoded = urlencoding::decode(value.trim()).ok()?.into_owned();
            let token = decoded.split("::").last()?.trim();
            let (user_id, email, subject, _) = parse_jwt_claims(token).ok()?;
            return Some(CursorSession {
                user_id,
                access_token: token.to_string(),
                cookie: format!("WorkosCursorSessionToken={}", value.trim()),
                email,
                subject: Some(subject),
            });
        }
    }
    None
}

/// 读取并解析本地 Cursor 凭据
pub fn read_cursor_credentials() -> (Option<CursorSession>, CredentialStatus, Option<String>) {
    // 优先：环境变量显式覆盖
    if let Ok(manual_cookie) = std::env::var("CURSOR_COOKIE") {
        if let Some(session) = parse_cookie_session(&manual_cookie) {
            return (Some(session), CredentialStatus::Valid, None);
        }
    }

    let raw_token = if let Ok(env_token) = std::env::var("CURSOR_ACCESS_TOKEN") {
        let t = env_token.trim().to_string();
        if !t.is_empty() {
            Some(t)
        } else {
            None
        }
    } else {
        None
    };

    let token = match raw_token {
        Some(t) => t,
        None => {
            // 候选 1: SQLite state.vscdb
            let mut found_token = None;
            for db_path in cursor_auth_db_candidates() {
                if db_path.is_file() {
                    if let Ok(t) = read_token_from_db(&db_path) {
                        found_token = Some(t);
                        break;
                    }
                }
            }

            // 候选 2: auth.json
            if found_token.is_none() {
                for json_path in cursor_auth_json_candidates() {
                    if json_path.is_file() {
                        if let Ok(t) = read_token_from_json(&json_path) {
                            found_token = Some(t);
                            break;
                        }
                    }
                }
            }

            match found_token {
                Some(t) => t,
                None => return (None, CredentialStatus::NotFound, None),
            }
        }
    };

    let (user_id, email, subject, exp) = match parse_jwt_claims(&token) {
        Ok(res) => res,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Invalid Cursor token: {e}")),
            );
        }
    };

    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // 若 exp 存在且已过期（留 60s 宽限）
    if exp > 0 && exp <= now_epoch + 60 {
        return (
            None,
            CredentialStatus::Expired,
            Some("Cursor session token has expired. Please sign in again in Cursor.".to_string()),
        );
    }

    let cookie = format!("WorkosCursorSessionToken={user_id}%3A%3A{token}");
    let session = CursorSession {
        user_id,
        access_token: token,
        cookie,
        email,
        subject: Some(subject),
    };

    (Some(session), CredentialStatus::Valid, None)
}

// ── 格式化与解析辅助 ──────────────────────────────────────

/// 格式化 Cursor 套餐名称
pub fn format_cursor_plan_name(membership_type: Option<&str>) -> String {
    let name = match membership_type.unwrap_or("").to_lowercase().as_str() {
        "enterprise" => "Enterprise",
        "express" => "Start",
        "free" => "Free",
        "free_trial" => "Pro Trial",
        "hobby" => "Hobby",
        "pro" => "Pro",
        "plus" | "pro_plus" => "Pro+",
        "pro_student" => "Pro (Student)",
        "team" => "Team",
        "ultra" => "Ultra",
        other if !other.is_empty() => other,
        _ => "Pro",
    };
    format!("Cursor {name}")
}

/// 解析重置时间（支持 ISO 8601 字符串或毫秒时间戳）
pub fn parse_reset_time(val: Option<&serde_json::Value>) -> Option<String> {
    let v = val?;
    if let Some(s) = v.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(millis) = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)) {
        if millis > 0 {
            if let Some(dt) = DateTime::<Utc>::from_timestamp_millis(millis) {
                return Some(dt.to_rfc3339());
            }
        }
    }
    None
}

// ── 额度查询入口 ──────────────────────────────────────────

/// 查询 Cursor 订阅额度
pub async fn get_cursor_subscription_quota() -> Result<SubscriptionQuota, String> {
    let (session, status, message) = read_cursor_credentials();
    let session = match session {
        Some(s) => s,
        None => {
            return match status {
                CredentialStatus::NotFound => Ok(SubscriptionQuota::not_found(TOOL)),
                other => Ok(SubscriptionQuota::error(
                    TOOL,
                    other,
                    message.unwrap_or_else(|| {
                        "Cursor authentication credentials not found".to_string()
                    }),
                )),
            };
        }
    };

    let client = crate::proxy::http_client::get();

    // 1. 获取 usage-summary（套餐类型、周期、计划用量、按量预算）
    let summary_resp = client
        .get("https://cursor.com/api/usage-summary")
        .header("Cookie", &session.cookie)
        .header("Origin", "https://cursor.com")
        .header("User-Agent", "Mozilla/5.0 (compatible; cc-switch)")
        .send()
        .await;

    let summary_resp = match summary_resp {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                TOOL,
                CredentialStatus::Valid,
                format!("Failed to connect to Cursor usage API: {e}"),
            ));
        }
    };

    let http_status = summary_resp.status();
    if http_status == reqwest::StatusCode::UNAUTHORIZED
        || http_status == reqwest::StatusCode::FORBIDDEN
    {
        return Ok(SubscriptionQuota::error(
            TOOL,
            CredentialStatus::Expired,
            "Cursor authentication session expired. Please open Cursor and log in again."
                .to_string(),
        ));
    }

    if !http_status.is_success() {
        return Ok(SubscriptionQuota::error(
            TOOL,
            CredentialStatus::Valid,
            format!("Cursor API returned status code: {http_status}"),
        ));
    }

    let summary_text = summary_resp
        .text()
        .await
        .map_err(|e| format!("Failed to read Cursor response body: {e}"))?;

    let summary: UsageSummaryResponse = serde_json::from_str(&summary_text)
        .map_err(|e| format!("Failed to parse Cursor usage-summary JSON: {e}"))?;

    // 2. 尝试获取 request usage (gpt-4 请求计数与上限)
    let request_url = format!(
        "https://cursor.com/api/usage?user={}",
        urlencoding::encode(&session.user_id)
    );
    let request_resp = client
        .get(&request_url)
        .header("Cookie", &session.cookie)
        .header("Origin", "https://cursor.com")
        .header("User-Agent", "Mozilla/5.0 (compatible; cc-switch)")
        .send()
        .await;

    let request_usage: Option<RequestUsageResponse> = match request_resp {
        Ok(resp) if resp.status().is_success() => resp.json().await.ok(),
        _ => None,
    };

    // 3. 计算使用率与配额
    let mut utilization: Option<f64> = None;

    // 优先：从 /api/usage 提取 gpt-4 fast requests
    if let Some(req_data) = &request_usage {
        if let Some(gpt4) = &req_data.gpt_4 {
            let used = gpt4.num_requests_total.or(gpt4.num_requests).unwrap_or(0.0);
            if let Some(max) = gpt4.max_request_usage {
                if max > 0.0 {
                    utilization = Some((used / max * 100.0).clamp(0.0, 100.0));
                }
            }
        }
    }

    // 次选：从 usage-summary 的 plan.totalPercentUsed
    let plan_opt = summary
        .individual_usage
        .as_ref()
        .and_then(|u| u.plan.as_ref())
        .or_else(|| summary.team_usage.as_ref().and_then(|t| t.pooled.as_ref()));

    if utilization.is_none() {
        if let Some(plan) = plan_opt {
            if let Some(pct) = plan.total_percent_used {
                utilization = Some(pct.clamp(0.0, 100.0));
            } else if let (Some(used), Some(limit)) = (plan.used, plan.limit) {
                if limit > 0.0 {
                    utilization = Some((used / limit * 100.0).clamp(0.0, 100.0));
                }
            }
        }
    }

    let utilization = utilization.unwrap_or(0.0);
    let reset_time = parse_reset_time(summary.billing_cycle_end.as_ref()).or_else(|| {
        request_usage
            .as_ref()
            .and_then(|r| r.start_of_month.clone())
    });

    let plan_name = format_cursor_plan_name(summary.membership_type.as_deref());

    let tiers = vec![QuotaTier {
        name: TIER_FAST_REQUESTS.to_string(),
        utilization,
        resets_at: reset_time,
        used_value_usd: None,
        max_value_usd: None,
    }];

    // On-demand 按量预算（如果有开启）
    let on_demand_opt = summary
        .individual_usage
        .as_ref()
        .and_then(|u| u.on_demand.as_ref())
        .or_else(|| {
            summary
                .team_usage
                .as_ref()
                .and_then(|t| t.on_demand.as_ref())
        });

    let extra_usage = on_demand_opt.and_then(|od| {
        let limit = od.limit.unwrap_or(0.0);
        if limit > 0.0 {
            let used = od.used.unwrap_or(0.0);
            Some(ExtraUsage {
                is_enabled: true,
                monthly_limit: Some(limit / 100.0), // cents to USD
                used_credits: Some(used / 100.0),
                utilization: Some((used / limit * 100.0).clamp(0.0, 100.0)),
                currency: Some("USD".to_string()),
            })
        } else {
            None
        }
    });

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    Ok(SubscriptionQuota {
        tool: TOOL.to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: Some(plan_name),
        success: true,
        tiers,
        extra_usage,
        reset_credits: None,
        credits_balance: None,
        error: None,
        queried_at: Some(now_ms),
    })
}

// ── 单元测试 ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 辅助生成无签名的测试 JWT（header.payload.signature）
    fn make_test_jwt(sub: &str, exp: i64, email: Option<&str>) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let mut map = serde_json::Map::new();
        map.insert(
            "sub".to_string(),
            serde_json::Value::String(sub.to_string()),
        );
        map.insert("exp".to_string(), serde_json::Value::Number(exp.into()));
        if let Some(em) = email {
            map.insert(
                "email".to_string(),
                serde_json::Value::String(em.to_string()),
            );
        }
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&map).unwrap());
        format!("{header}.{payload}.signature")
    }

    #[test]
    fn test_parse_jwt_claims_valid() {
        let exp = 1900000000;
        let jwt = make_test_jwt("auth0|user_123456", exp, Some("user@example.com"));
        let (user_id, email, subject, parsed_exp) = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(user_id, "user_123456");
        assert_eq!(email.as_deref(), Some("user@example.com"));
        assert_eq!(subject, "auth0|user_123456");
        assert_eq!(parsed_exp, exp);
    }

    #[test]
    fn test_parse_jwt_claims_plain_sub() {
        let exp = 1900000000;
        let jwt = make_test_jwt("user_plain_789", exp, None);
        let (user_id, email, subject, _) = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(user_id, "user_plain_789");
        assert_eq!(email, None);
        assert_eq!(subject, "user_plain_789");
    }

    #[test]
    fn test_parse_jwt_claims_invalid() {
        assert!(parse_jwt_claims("not-a-jwt").is_err());
        assert!(parse_jwt_claims("only.one").is_err());
    }

    #[test]
    fn test_parse_cookie_session() {
        let exp = 1900000000;
        let jwt = make_test_jwt("auth0|user_abc", exp, Some("abc@example.com"));
        let cookie_str =
            format!("other_cookie=123; WorkosCursorSessionToken=user_abc%3A%3A{jwt}; test=true");
        let session = parse_cookie_session(&cookie_str).unwrap();
        assert_eq!(session.user_id, "user_abc");
        assert_eq!(session.access_token, jwt);
        assert_eq!(session.email.as_deref(), Some("abc@example.com"));
    }

    #[test]
    fn test_format_cursor_plan_name() {
        assert_eq!(format_cursor_plan_name(Some("pro")), "Cursor Pro");
        assert_eq!(format_cursor_plan_name(Some("plus")), "Cursor Pro+");
        assert_eq!(format_cursor_plan_name(Some("pro_plus")), "Cursor Pro+");
        assert_eq!(format_cursor_plan_name(Some("free")), "Cursor Free");
        assert_eq!(
            format_cursor_plan_name(Some("enterprise")),
            "Cursor Enterprise"
        );
        assert_eq!(format_cursor_plan_name(Some("team")), "Cursor Team");
        assert_eq!(format_cursor_plan_name(None), "Cursor Pro");
    }

    #[test]
    fn test_parse_reset_time() {
        let iso_val = serde_json::json!("2026-12-01T00:00:00.000Z");
        assert_eq!(
            parse_reset_time(Some(&iso_val)),
            Some("2026-12-01T00:00:00.000Z".to_string())
        );

        let millis_val = serde_json::json!(1764547200000i64);
        assert!(parse_reset_time(Some(&millis_val)).is_some());

        assert_eq!(parse_reset_time(None), None);
    }

    #[test]
    fn test_parse_usage_summary_json() {
        let raw = r#"{
            "billingCycleStart": "2026-11-01T00:00:00.000Z",
            "billingCycleEnd": "2026-12-01T00:00:00.000Z",
            "membershipType": "plus",
            "individualUsage": {
                "plan": {
                    "used": 142,
                    "limit": 500,
                    "totalPercentUsed": 28.4
                },
                "onDemand": {
                    "used": 250,
                    "limit": 2000
                }
            }
        }"#;

        let summary: UsageSummaryResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(summary.membership_type.as_deref(), Some("plus"));
        let plan = summary.individual_usage.unwrap().plan.unwrap();
        assert_eq!(plan.total_percent_used, Some(28.4));
    }
}
