//! 直接向 Google 查某个 Antigravity 账号的剩余额度（授权中心逐个账号显示）。
//!
//! 和 agy 的 `/usage` 是同一个接口、同一组额度桶，区别是用 CC Switch 存的那份令牌查，
//! 不用先把 agy 切到这个号。这个接口只回应自报是 Antigravity 的客户端（User-Agent
//! 里不带 `antigravity` 一律 403「没有有效许可」），所以 UA 写成
//! `cc-switch/<版本> (antigravity)`：照实报 CC Switch，只补上服务端要的那个词。
//! 只查额度，不调模型、不做预热，也不据额度自动换号。
//!
//! 没有使用资格的号（如 Google 判定未满 18 岁）同样回 403「没有有效许可」，这时再问一次
//! `loadCodeAssist`，把 Google 给的不合格原因原样显示，免得看着像 CC Switch 的问题。

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::services::subscription::QuotaTier;
use crate::services::subscription_agy::collect_tiers;

const API_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: usize = 256 * 1024;

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

fn user_agent() -> String {
    format!("cc-switch/{} (antigravity)", env!("CARGO_PKG_VERSION"))
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
        .header(reqwest::header::USER_AGENT, user_agent())
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
    fn user_agent_names_cc_switch_and_the_client_it_queries_for() {
        let ua = user_agent();
        assert!(ua.starts_with("cc-switch/"));
        assert!(ua.contains("antigravity"));
    }
}
