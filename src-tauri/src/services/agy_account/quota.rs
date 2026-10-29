//! 直接向 Google 查某个 Antigravity 账号的剩余额度（授权中心逐个账号显示）。
//!
//! 和 agy 的 `/usage` 是同一个接口、同一组额度桶，区别是用 CC Switch 存的那份令牌查，
//! 不用先把 agy 切到这个号。服务端按 User-Agent 决定回什么：不带 `antigravity` 一律
//! 403「没有有效许可」；带了但不是 `antigravity/cli/<版本>`（或版本低于 1.2.0）时只回
//! Claude 和 GPT 那组，Gemini 那组整个省掉。所以 UA 写成
//! `cc-switch/<版本> (antigravity/cli/<本机 agy 版本>)`：照实报 CC Switch，括号里补上
//! 服务端要的客户端标识，版本取本机装的 agy，读不到时用 `FALLBACK_AGY_VERSION`。
//! 只查额度，不调模型、不做预热，也不据额度自动换号。
//!
//! 没有使用资格的号（如 Google 判定未满 18 岁）同样回 403「没有有效许可」，这时再问一次
//! `loadCodeAssist`，把 Google 给的不合格原因原样显示，免得看着像 CC Switch 的问题。

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::OnceCell;

use crate::services::subscription::QuotaTier;
use crate::services::subscription_agy::collect_tiers;

const API_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: usize = 256 * 1024;
/// 本机读不到 agy 版本时写进 UA 的版本（实测过的发行版；服务端要 1.2.0 起才回 Gemini 那组）
const FALLBACK_AGY_VERSION: &str = "1.4.6";
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum QuotaError {
    /// 令牌被拒（401）：续期也救不回来时要重新登录
    #[error("Google rejected the sign-in (401)")]
    Unauthorized,
    /// 账号没有 Antigravity 使用资格，带 Google 给的原因
    #[error("{0}")]
    Ineligible(String),
    #[error("Google returned {status}: {message}")]
    Http { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected quota response")]
    Parse,
}

impl From<reqwest::Error> for QuotaError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url().to_string())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryResponse {
    #[serde(default)]
    groups: Vec<SummaryGroup>,
}

#[derive(Deserialize)]
struct SummaryGroup {
    #[serde(default)]
    buckets: Vec<SummaryBucket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryBucket {
    bucket_id: Option<String>,
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssistResponse {
    current_tier: Option<Value>,
    #[serde(default)]
    ineligible_tiers: Vec<IneligibleTier>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IneligibleTier {
    reason_message: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<ErrorDetail>,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: Option<String>,
}

fn user_agent(agy_version: &str) -> String {
    format!(
        "cc-switch/{} (antigravity/cli/{agy_version})",
        env!("CARGO_PKG_VERSION")
    )
}

/// 本机 agy 的版本，每个进程只探测一次（定位 agy 可能要问登录 shell）；读不到用固定值。
async fn agy_version() -> &'static str {
    static VERSION: OnceCell<String> = OnceCell::const_new();
    VERSION
        .get_or_init(|| async {
            probe_agy_version()
                .await
                .unwrap_or_else(|| FALLBACK_AGY_VERSION.to_string())
        })
        .await
}

async fn probe_agy_version() -> Option<String> {
    let bin = tokio::task::spawn_blocking(crate::agy_config::real_agy_bin)
        .await
        .ok()??;
    let mut command = tokio::process::Command::new(bin);
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(VERSION_PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_version(&output.stdout))
        .flatten()
}

/// `agy --version` 的输出里第一个形如 `1.4.6` 的词（允许带 `v` 前缀）。
fn parse_version(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .split_whitespace()
        .map(|word| word.trim_start_matches('v'))
        .find(|word| {
            word.contains('.')
                && word.starts_with(|c: char| c.is_ascii_digit())
                && word.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(str::to_string)
}

pub async fn fetch(access_token: &str) -> Result<Vec<QuotaTier>, QuotaError> {
    let (status, body) = post(access_token, "retrieveUserQuotaSummary", json!({})).await?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(QuotaError::Unauthorized);
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        if let Some(reason) = ineligible_reason(access_token).await {
            return Err(QuotaError::Ineligible(reason));
        }
    }
    if !status.is_success() {
        return Err(QuotaError::Http {
            status: status.as_u16(),
            message: error_message(&body),
        });
    }
    parse(&body).ok_or(QuotaError::Parse)
}

/// 账号没有使用资格时返回 Google 给的原因；有资格或问不到时返回 `None`。
async fn ineligible_reason(access_token: &str) -> Option<String> {
    let (status, body) = post(
        access_token,
        "loadCodeAssist",
        json!({"metadata": {"ideType": "ANTIGRAVITY"}}),
    )
    .await
    .ok()?;
    status
        .is_success()
        .then(|| parse_ineligible_reason(&body))
        .flatten()
}

async fn post(
    access_token: &str,
    method: &str,
    body: Value,
) -> Result<(reqwest::StatusCode, bytes::Bytes), QuotaError> {
    let response = crate::proxy::http_client::get()
        .post(format!("{API_BASE}:{method}"))
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, user_agent(agy_version().await))
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let body = response.bytes().await?;
    if body.len() > MAX_BODY {
        return Err(QuotaError::Parse);
    }
    Ok((status, body))
}

fn parse(body: &[u8]) -> Option<Vec<QuotaTier>> {
    let response: SummaryResponse = serde_json::from_slice(body).ok()?;
    collect_tiers(
        response
            .groups
            .into_iter()
            .flat_map(|group| group.buckets)
            .map(|bucket| {
                (
                    bucket.bucket_id,
                    bucket.remaining_fraction,
                    bucket.reset_time,
                )
            }),
    )
}

/// 只有不合格档、没有当前档时才算没资格。
fn parse_ineligible_reason(body: &[u8]) -> Option<String> {
    let response: LoadCodeAssistResponse = serde_json::from_slice(body).ok()?;
    if response.current_tier.is_some() {
        return None;
    }
    response
        .ineligible_tiers
        .into_iter()
        .find_map(|tier| tier.reason_message.filter(|message| !message.is_empty()))
}

/// Google 错误体里的 `error.message`，截短；拿不到时为空串。
fn error_message(body: &[u8]) -> String {
    serde_json::from_slice::<ErrorBody>(body)
        .ok()
        .and_then(|body| body.error?.message)
        .map(|message| message.chars().take(200).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription_agy::{
        TIER_AGY_GEMINI_FIVE_HOUR, TIER_AGY_GEMINI_WEEKLY, TIER_AGY_THIRD_PARTY_FIVE_HOUR,
        TIER_AGY_THIRD_PARTY_WEEKLY,
    };

    // retrieveUserQuotaSummary 的真实结构（数值改过）
    const SAMPLE: &str = r#"{"groups": [
      {"displayName": "Gemini Models", "buckets": [
        {"bucketId": "gemini-weekly", "window": "weekly", "resetTime": "2026-10-28T07:41:54Z", "remainingFraction": 1},
        {"bucketId": "gemini-5h", "window": "5h", "resetTime": "2026-10-21T12:41:54Z", "remainingFraction": 0.5}
      ]},
      {"displayName": "Claude and GPT models", "buckets": [
        {"bucketId": "3p-weekly", "window": "weekly", "resetTime": "2026-10-28T04:42:02Z", "remainingFraction": 0.988746},
        {"bucketId": "3p-5h", "window": "5h", "resetTime": "2026-10-21T09:42:02Z", "remainingFraction": 0},
        {"bucketId": "future-bucket", "remainingFraction": 0.3}
      ]}
    ], "description": "..."}"#;

    #[test]
    fn parses_the_same_tiers_as_agy_usage() {
        let tiers = parse(SAMPLE.as_bytes()).unwrap();
        let names: Vec<_> = tiers.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                TIER_AGY_GEMINI_FIVE_HOUR,
                TIER_AGY_GEMINI_WEEKLY,
                TIER_AGY_THIRD_PARTY_FIVE_HOUR,
                TIER_AGY_THIRD_PARTY_WEEKLY,
            ]
        );
        let used: Vec<_> = tiers.iter().map(|t| t.utilization.round()).collect();
        assert_eq!(used, [50.0, 0.0, 100.0, 1.0]);
        assert_eq!(tiers[0].resets_at.as_deref(), Some("2026-10-21T12:41:54Z"));
    }

    #[test]
    fn empty_or_unknown_responses_are_not_quota() {
        assert!(parse(b"{}").is_none());
        assert!(
            parse(br#"{"groups":[{"buckets":[{"bucketId":"x","remainingFraction":1}]}]}"#)
                .is_none()
        );
        assert!(parse(b"<html>").is_none());
    }

    #[test]
    fn error_message_is_taken_from_google_error_body() {
        let body = br#"{"error":{"code":403,"message":"You do not have a valid license"}}"#;
        assert_eq!(error_message(body), "You do not have a valid license");
        assert_eq!(error_message(b"oops"), "");
    }

    #[test]
    fn ineligibility_reason_comes_from_load_code_assist() {
        let ineligible = br#"{"ineligibleTiers":[{"reasonCode":"RESTRICTED_AGE","reasonMessage":"Your current account is not eligible for Antigravity.","tierId":"free-tier"}],"allowedTiers":[{"id":"standard-tier"}]}"#;
        assert_eq!(
            parse_ineligible_reason(ineligible).as_deref(),
            Some("Your current account is not eligible for Antigravity.")
        );
        // 有当前档就是有资格（403 另有原因）
        let eligible =
            br#"{"currentTier":{"id":"free-tier"},"ineligibleTiers":[{"reasonMessage":"x"}]}"#;
        assert_eq!(parse_ineligible_reason(eligible), None);
        assert_eq!(parse_ineligible_reason(b"{}"), None);
    }

    #[test]
    fn user_agent_names_cc_switch_and_the_agy_cli_version() {
        // 服务端只在看到 `antigravity/cli/<版本>` 时才回 Gemini 那组，写成 `(antigravity)` 会漏
        let ua = user_agent("1.4.6");
        assert!(ua.starts_with("cc-switch/"));
        assert!(ua.ends_with(" (antigravity/cli/1.4.6)"));
        assert!(user_agent(FALLBACK_AGY_VERSION).contains("antigravity/cli/"));
    }

    #[test]
    fn version_is_read_from_agy_version_output() {
        assert_eq!(parse_version(b"1.4.6\n").as_deref(), Some("1.4.6"));
        assert_eq!(
            parse_version(b"agy version v1.5.0 (abc123)").as_deref(),
            Some("1.5.0")
        );
        assert_eq!(parse_version(b""), None);
        assert_eq!(parse_version(b"unknown"), None);
        assert_eq!(parse_version(b"build 42"), None);
    }
}
