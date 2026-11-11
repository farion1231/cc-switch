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
//! 顺序和 agy 1.5.5 一致：先调一次 `loadCodeAssist` 拿套餐、`cloudaicompanionProject` 和使用资格，
//! 再把 `project` 带给 `retrieveUserQuotaSummary`（免费普通号的托管项目如 `aicode-consumers`
//! 不带时会被判成 403「没有有效许可」）。没有资格或锁区（如 `UNSUPPORTED_LOCATION`）的号在第一步
//! 就直接把 Google 给的原因原样显示，免得看着像 CC Switch 的问题，也不用再发第二次请求。
//!
//! 接口地址必须和 agy 用的一致：agy（1.5 起，见 `~/.gemini/antigravity-cli/log`）所有请求都发往
//! `daily-cloudcode-pa`，Gemini 那组的用量也记在那边；`cloudcode-pa` 回的 Gemini 桶是另一份
//! （重置时间、剩余比例都对不上，11-07 实测同一个号 0.94 对 1.0），Claude/GPT 那组两边相同。

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::OnceCell;

use crate::services::subscription::QuotaTier;
use crate::services::subscription_agy::collect_tiers;

/// agy 自己用的地址（不是 Gemini CLI 用的 `cloudcode-pa`），额度才和 agy 的 `/usage` 一致
const API_BASE: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
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
    cloudaicompanion_project: Option<Value>,
    current_tier: Option<Value>,
    /// 订阅（Google AI Pro / Ultra）在这里；有时 Pro 号也缺，缺了看 currentTier
    paid_tier: Option<Value>,
    #[serde(default)]
    allowed_tiers: Option<Vec<AllowedTier>>,
    #[serde(default)]
    ineligible_tiers: Option<Vec<IneligibleTier>>,
    gcp_managed: Option<bool>,
}

/// 一个账号的额度和套餐
pub struct AccountQuota {
    pub tiers: Vec<QuotaTier>,
    pub plan: Option<String>,
}

#[derive(Deserialize)]
struct AllowedTier {
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IneligibleTier {
    tier_id: Option<String>,
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

/// 先问 `loadCodeAssist` 拿套餐、`cloudaicompanionProject` 和使用资格，再带 `project` 查额度
/// （和 agy 1.5.5 一致）；`loadCodeAssist` 因非 401 失败时降级为不带项目直查额度（套餐问不到
/// 只是不显示，不影响额度）。
pub async fn fetch(access_token: &str) -> Result<AccountQuota, QuotaError> {
    fetch_with(|method, body| post(access_token, method, body)).await
}

async fn fetch_with<F, Fut>(mut send: F) -> Result<AccountQuota, QuotaError>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<(reqwest::StatusCode, bytes::Bytes), QuotaError>>,
{
    match load_code_assist(&mut send).await {
        Ok((reqwest::StatusCode::UNAUTHORIZED, _)) => Err(QuotaError::Unauthorized),
        Ok((status, body)) if status.is_success() => {
            if let Some(reason) = parse_ineligible_reason(&body) {
                return Err(QuotaError::Ineligible(reason));
            }
            let plan = code_assist_plan(&body);
            let project = parse_project(&body);
            let tiers = fetch_tiers(&mut send, project.as_deref()).await?;
            Ok(AccountQuota { tiers, plan })
        }
        Ok(_) | Err(_) => match fetch_tiers(&mut send, None).await {
            Ok(tiers) => Ok(AccountQuota { tiers, plan: None }),
            Err(QuotaError::Http {
                status: 403,
                message,
            }) => {
                if let Some(reason) = ineligible_reason(&mut send).await {
                    return Err(QuotaError::Ineligible(reason));
                }
                Err(QuotaError::Http {
                    status: 403,
                    message,
                })
            }
            Err(error) => Err(error),
        },
    }
}

async fn fetch_tiers<F, Fut>(
    send: &mut F,
    project: Option<&str>,
) -> Result<Vec<QuotaTier>, QuotaError>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<(reqwest::StatusCode, bytes::Bytes), QuotaError>>,
{
    let request = match project {
        Some(project) => json!({ "project": project }),
        None => json!({}),
    };
    let (status, body) = send("retrieveUserQuotaSummary", request).await?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(QuotaError::Unauthorized);
    }
    if !status.is_success() {
        return Err(QuotaError::Http {
            status: status.as_u16(),
            message: error_message(&body),
        });
    }
    parse(&body).ok_or(QuotaError::Parse)
}

async fn load_code_assist<F, Fut>(
    send: &mut F,
) -> Result<(reqwest::StatusCode, bytes::Bytes), QuotaError>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<(reqwest::StatusCode, bytes::Bytes), QuotaError>>,
{
    send(
        "loadCodeAssist",
        json!({"metadata": {"ideType": "ANTIGRAVITY"}}),
    )
    .await
}

/// 账号没有使用资格时返回 Google 给的原因；有资格或问不到时返回 `None`。
async fn ineligible_reason<F, Fut>(send: &mut F) -> Option<String>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<(reqwest::StatusCode, bytes::Bytes), QuotaError>>,
{
    let (status, body) = load_code_assist(send).await.ok()?;
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

/// `loadCodeAssist` 响应里的套餐名：先看 `paidTier.name`（「Google AI Pro」），没有时按
/// `currentTier.id` 写 Free / Paid（gemini-cli 和 CodexBar 都这样取）。Gemini CLI 的额度也用它
pub(crate) fn code_assist_plan(body: &[u8]) -> Option<String> {
    let response: LoadCodeAssistResponse = serde_json::from_slice(body).ok()?;
    let name = |tier: &Option<Value>| {
        tier.as_ref()?
            .get("name")?
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    };
    if let Some(name) = name(&response.paid_tier) {
        let id = response
            .paid_tier
            .as_ref()
            .and_then(|tier| tier.get("id")?.as_str());
        return Some(with_ultra_variant(name, id));
    }
    let id = response.current_tier.as_ref()?.get("id")?.as_str()?;
    match id {
        "free-tier" => Some("Free".to_string()),
        "standard-tier" => Some("Paid".to_string()),
        _ => name(&response.current_tier).map(|name| with_ultra_variant(name, Some(id))),
    }
}

/// Google AI Ultra 分 5x、20x 两档，`paidTier.name` 都只写「Google AI Ultra」，靠 id 区分
/// （11-07 实测）：`g1-ultra-lite-tier` 是 5x（升级提示写「可升级到更高的 Ultra 方案」），
/// `g1-ultra-tier` 是 20x（「已是最好的 Google AI 方案」）。名字里已带档位时不重复补。
fn with_ultra_variant(name: String, tier_id: Option<&str>) -> String {
    let variant = match tier_id {
        Some("g1-ultra-lite-tier") => "5x",
        Some("g1-ultra-tier") => "20x",
        _ => return name,
    };
    let lower = name.to_ascii_lowercase();
    if !lower.contains("ultra") || lower.contains("5x") || lower.contains("20x") {
        return name;
    }
    format!("{name} {variant}")
}

/// `loadCodeAssist` 返回的 `cloudaicompanionProject`（如免费普通号的 `"aicode-consumers"`），
/// 兼容字符串或 `{"id": ...}` / `{"projectId": ...}` 对象；去空白，不拼 `"projects/"` 前缀。
fn parse_project(body: &[u8]) -> Option<String> {
    let response: LoadCodeAssistResponse = serde_json::from_slice(body).ok()?;
    match response.cloudaicompanion_project.as_ref()? {
        Value::String(project) => non_empty(Some(project.as_str())),
        Value::Object(map) => non_empty(map.get("id").and_then(Value::as_str))
            .or_else(|| non_empty(map.get("projectId").and_then(Value::as_str))),
        _ => None,
    }
}

/// 账号没有 Antigravity 使用资格时提取 Google 给的原因：
/// - 非企业托管账号（`gcpManaged != true`）用 Antigravity 必须具备 `free-tier` 资格（和 agy /
///   桌面端 `ValidateAndOnboardAccount` 一致）；锁区（如 `UNSUPPORTED_LOCATION`）的订阅号虽带
///   `standard-tier` 或 `paidTier`，但 `free-tier` 落在 `ineligibleTiers` 里，照样用不了。
/// - 其余情况沿用原规则：没有 `currentTier` 时从 `ineligibleTiers` 取第一条非空 `reasonMessage`。
fn parse_ineligible_reason(body: &[u8]) -> Option<String> {
    let response: LoadCodeAssistResponse = serde_json::from_slice(body).ok()?;
    let allowed_tiers = response.allowed_tiers.as_deref().unwrap_or_default();
    let ineligible_tiers = response.ineligible_tiers.as_deref().unwrap_or_default();

    let current_id = response
        .current_tier
        .as_ref()
        .and_then(|tier| tier.get("id")?.as_str());
    let has_free_tier = current_id == Some("free-tier")
        || allowed_tiers
            .iter()
            .any(|tier| tier.id.as_deref() == Some("free-tier"));

    if response.gcp_managed != Some(true) && !has_free_tier {
        if let Some(reason) = ineligible_tiers
            .iter()
            .filter(|tier| tier.tier_id.as_deref() == Some("free-tier"))
            .find_map(|tier| non_empty(tier.reason_message.as_deref()))
        {
            return Some(reason);
        }
    }

    if response.current_tier.is_some() {
        return None;
    }
    ineligible_tiers
        .iter()
        .find_map(|tier| non_empty(tier.reason_message.as_deref()))
}

fn non_empty(text: Option<&str>) -> Option<String> {
    let trimmed = text?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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
        TIER_AGY_GEMINI_ARGON_WEEKLY, TIER_AGY_GEMINI_FIVE_HOUR, TIER_AGY_GEMINI_WEEKLY,
        TIER_AGY_THIRD_PARTY_FIVE_HOUR, TIER_AGY_THIRD_PARTY_WEEKLY,
    };

    // retrieveUserQuotaSummary 的真实结构（数值改过）
    const SAMPLE: &str = r#"{"groups": [
      {"displayName": "Gemini Models", "buckets": [
        {"bucketId": "gemini-weekly", "window": "weekly", "resetTime": "2026-10-28T07:41:54Z", "remainingFraction": 1},
        {"bucketId": "gemini-5h", "window": "5h", "resetTime": "2026-10-21T12:41:54Z", "remainingFraction": 0.5},
        {"bucketId": "gemini-argon-wtu-weekly", "window": "weekly", "resetTime": "2026-10-28T09:00:00Z", "remainingFraction": 0.6}
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
                TIER_AGY_GEMINI_ARGON_WEEKLY,
                TIER_AGY_THIRD_PARTY_FIVE_HOUR,
                TIER_AGY_THIRD_PARTY_WEEKLY,
            ]
        );
        let used: Vec<_> = tiers.iter().map(|t| t.utilization.round()).collect();
        assert_eq!(used, [50.0, 0.0, 40.0, 100.0, 1.0]);
        assert_eq!(tiers[0].resets_at.as_deref(), Some("2026-10-21T12:41:54Z"));
        assert_eq!(tiers[2].resets_at.as_deref(), Some("2026-10-28T09:00:00Z"));
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
    fn plan_prefers_the_paid_tier_name() {
        let plan = |body: &str| code_assist_plan(body.as_bytes());
        assert_eq!(
            plan(r#"{"currentTier":{"id":"standard-tier","name":"Gemini Code Assist"},"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#)
                .as_deref(),
            Some("Google AI Pro")
        );
        // paidTier 缺了：按当前档写
        assert_eq!(
            plan(r#"{"currentTier":{"id":"standard-tier","name":"Gemini Code Assist"}}"#)
                .as_deref(),
            Some("Paid")
        );
        assert_eq!(
            plan(
                r#"{"currentTier":{"id":"free-tier","name":"Gemini Code Assist for individuals"}}"#
            )
            .as_deref(),
            Some("Free")
        );
        assert_eq!(plan(r#"{"ineligibleTiers":[]}"#), None);
    }

    #[test]
    fn ultra_plans_show_their_5x_or_20x_variant() {
        let plan = |body: &str| code_assist_plan(body.as_bytes());
        // loadCodeAssist 的真实 paidTier（11-07），两档名字一样，只有 id 不同
        assert_eq!(
            plan(r#"{"currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("Google AI Ultra 5x")
        );
        assert_eq!(
            plan(r#"{"currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"g1-ultra-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("Google AI Ultra 20x")
        );
        // 以后 Google 自己把档位写进名字时不重复补；别的套餐不动
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-ultra-tier","name":"Google AI Ultra 20x"}}"#).as_deref(),
            Some("Google AI Ultra 20x")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra 5X"}}"#)
                .as_deref(),
            Some("Google AI Ultra 5X")
        );
        assert_eq!(
            plan(r#"{"currentTier":{"id":"g1-ultra-lite-tier","name":"Google AI Ultra"}}"#)
                .as_deref(),
            Some("Google AI Ultra 5x")
        );
        assert_eq!(
            plan(r#"{"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#).as_deref(),
            Some("Google AI Pro")
        );
    }

    #[test]
    fn quota_is_read_from_the_endpoint_agy_uses() {
        assert!(API_BASE.starts_with("https://daily-cloudcode-pa.googleapis.com/"));
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

    #[test]
    fn region_locked_paid_account_reports_google_ineligibility_reason() {
        let locked_ultra = br#"{
            "currentTier": {"id": "standard-tier", "name": "Gemini Code Assist"},
            "paidTier": {"id": "g1-ultra-tier", "name": "Google AI Ultra"},
            "allowedTiers": [{"id": "standard-tier"}],
            "ineligibleTiers": [{
                "reasonCode": "UNSUPPORTED_LOCATION",
                "reasonMessage": "Your current account is not eligible for Antigravity, because it is not currently available in your location.",
                "tierId": "free-tier"
            }]
        }"#;
        assert_eq!(
            parse_ineligible_reason(locked_ultra).as_deref(),
            Some("Your current account is not eligible for Antigravity, because it is not currently available in your location.")
        );

        // 改区通过后 allowedTiers 带上 free-tier，恢复正常可用
        let unlocked = br#"{
            "currentTier": {"id": "standard-tier", "name": "Gemini Code Assist"},
            "paidTier": {"id": "g1-ultra-tier", "name": "Google AI Ultra"},
            "allowedTiers": [{"id": "standard-tier"}, {"id": "free-tier"}],
            "ineligibleTiers": [{
                "reasonCode": "UNSUPPORTED_LOCATION",
                "reasonMessage": "stale",
                "tierId": "free-tier"
            }]
        }"#;
        assert_eq!(parse_ineligible_reason(unlocked), None);

        // 企业托管号（gcpManaged = true）不走个人 free-tier 准入校验
        let enterprise = br#"{
            "gcpManaged": true,
            "currentTier": {"id": "standard-tier", "name": "Gemini Code Assist"},
            "allowedTiers": [{"id": "standard-tier"}],
            "ineligibleTiers": [{
                "reasonMessage": "Not eligible for consumer free tier.",
                "tierId": "free-tier"
            }]
        }"#;
        assert_eq!(parse_ineligible_reason(enterprise), None);

        // gcpManaged = false 时仍按非企业号拦下；非 free-tier 的不合格项不误伤已开通账号
        let explicit_non_gcp = br#"{
            "gcpManaged": false,
            "currentTier": {"id": "standard-tier"},
            "ineligibleTiers": [{"reasonMessage": "Unsupported region", "tierId": "free-tier"}]
        }"#;
        assert_eq!(
            parse_ineligible_reason(explicit_non_gcp).as_deref(),
            Some("Unsupported region")
        );

        let other_ineligible_tier = br#"{
            "currentTier": {"id": "standard-tier"},
            "ineligibleTiers": [{"reasonMessage": "Enterprise only", "tierId": "enterprise-tier"}]
        }"#;
        assert_eq!(parse_ineligible_reason(other_ineligible_tier), None);

        // null 数组不崩，且 currentTier 缺失时仍兼容无 tierId 的错误原因
        let generic_ineligible =
            br#"{"allowedTiers":null,"ineligibleTiers":[{"reasonMessage":"  Account disabled  "}]}"#;
        assert_eq!(
            parse_ineligible_reason(generic_ineligible).as_deref(),
            Some("Account disabled")
        );
    }

    #[test]
    fn project_is_extracted_from_string_or_object_without_prefix() {
        let project = |body: &str| parse_project(body.as_bytes());
        assert_eq!(
            project(r#"{"cloudaicompanionProject":"aicode-consumers"}"#).as_deref(),
            Some("aicode-consumers")
        );
        assert_eq!(
            project(r#"{"cloudaicompanionProject":"  aicode-consumers \n"}"#).as_deref(),
            Some("aicode-consumers")
        );
        assert_eq!(
            project(r#"{"cloudaicompanionProject":{"id":"aicode-consumers"}}"#).as_deref(),
            Some("aicode-consumers")
        );
        assert_eq!(
            project(r#"{"cloudaicompanionProject":{"id":"  ","projectId":"my-project"}}"#)
                .as_deref(),
            Some("my-project")
        );
        assert_eq!(project(r#"{"cloudaicompanionProject":""}"#), None);
        assert_eq!(project(r#"{"cloudaicompanionProject":"   "}"#), None);
        assert_eq!(project(r#"{"cloudaicompanionProject":null}"#), None);
        assert_eq!(project(r#"{"cloudaicompanionProject":{}}"#), None);
        assert_eq!(project(r#"{}"#), None);
    }

    #[tokio::test]
    async fn fetch_passes_project_to_quota_summary_for_starter_accounts() {
        use std::collections::VecDeque;

        let mut calls = Vec::new();
        let mut responses = VecDeque::from([
            Ok((
                reqwest::StatusCode::OK,
                bytes::Bytes::from_static(
                    br#"{"cloudaicompanionProject":"aicode-consumers","currentTier":{"id":"free-tier","name":"Antigravity"},"paidTier":{"id":"free-tier","name":"Antigravity Starter Quota"},"allowedTiers":[{"id":"free-tier"}]}"#,
                ),
            )),
            Ok((reqwest::StatusCode::OK, bytes::Bytes::from_static(SAMPLE.as_bytes()))),
        ]);

        let quota = fetch_with(|method, body| {
            calls.push((method, body));
            let next = responses.pop_front().unwrap();
            async move { next }
        })
        .await
        .unwrap();

        assert_eq!(
            calls,
            vec![
                (
                    "loadCodeAssist",
                    json!({"metadata": {"ideType": "ANTIGRAVITY"}})
                ),
                (
                    "retrieveUserQuotaSummary",
                    json!({"project": "aicode-consumers"})
                ),
            ]
        );
        assert_eq!(quota.plan.as_deref(), Some("Antigravity Starter Quota"));
        assert_eq!(quota.tiers.len(), 5);

        // 没有 cloudaicompanionProject 时仍发空对象 `{}`
        let mut calls = Vec::new();
        let mut responses = VecDeque::from([
            Ok((
                reqwest::StatusCode::OK,
                bytes::Bytes::from_static(
                    br#"{"currentTier":{"id":"free-tier"},"paidTier":{"id":"g1-pro-tier","name":"Google AI Pro"}}"#,
                ),
            )),
            Ok((reqwest::StatusCode::OK, bytes::Bytes::from_static(SAMPLE.as_bytes()))),
        ]);
        let quota = fetch_with(|method, body| {
            calls.push((method, body));
            let next = responses.pop_front().unwrap();
            async move { next }
        })
        .await
        .unwrap();
        assert_eq!(calls[1], ("retrieveUserQuotaSummary", json!({})));
        assert_eq!(quota.plan.as_deref(), Some("Google AI Pro"));
    }

    #[tokio::test]
    async fn fetch_stops_early_on_unauthorized_or_ineligible_without_duplicate_requests() {
        use std::collections::VecDeque;

        // 1. loadCodeAssist 401：直接返回 Unauthorized，不再调 retrieveUserQuotaSummary
        let mut count = 0;
        let err = fetch_with(|_, _| {
            count += 1;
            async { Ok((reqwest::StatusCode::UNAUTHORIZED, bytes::Bytes::new())) }
        })
        .await
        .err()
        .unwrap();
        assert!(matches!(err, QuotaError::Unauthorized));
        assert_eq!(count, 1);

        // 2. 锁区或无资格：loadCodeAssist 一步即返回 Ineligible，不发 retrieveUserQuotaSummary
        let mut count = 0;
        let err = fetch_with(|_, _| {
            count += 1;
            async {
                Ok((
                    reqwest::StatusCode::OK,
                    bytes::Bytes::from_static(
                        br#"{"currentTier":{"id":"standard-tier"},"paidTier":{"id":"g1-ultra-tier","name":"Google AI Ultra"},"ineligibleTiers":[{"tierId":"free-tier","reasonCode":"UNSUPPORTED_LOCATION","reasonMessage":"Not available in your location."}]}"#,
                    ),
                ))
            }
        })
        .await
        .err()
        .unwrap();
        assert!(matches!(
            err,
            QuotaError::Ineligible(ref msg) if msg == "Not available in your location."
        ));
        assert_eq!(count, 1);

        // 3. loadCodeAssist 已成功确认有资格时，retrieveUserQuotaSummary 回 403 不再二次调 loadCodeAssist
        let mut count = 0;
        let mut responses = VecDeque::from([
            Ok((
                reqwest::StatusCode::OK,
                bytes::Bytes::from_static(br#"{"currentTier":{"id":"free-tier"}}"#),
            )),
            Ok((
                reqwest::StatusCode::FORBIDDEN,
                bytes::Bytes::from_static(
                    br#"{"error":{"code":403,"message":"You do not have a valid license"}}"#,
                ),
            )),
        ]);
        let err = fetch_with(|_, _| {
            count += 1;
            let next = responses.pop_front().unwrap();
            async move { next }
        })
        .await
        .err()
        .unwrap();
        assert!(matches!(
            err,
            QuotaError::Http { status: 403, ref message } if message == "You do not have a valid license"
        ));
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn fetch_degrades_gracefully_when_load_code_assist_fails_with_non_401() {
        use std::collections::VecDeque;

        // 套餐接口非 401 失败：不影响无项目直查额度
        let mut responses = VecDeque::from([
            Err(QuotaError::Network("timeout".to_string())),
            Ok((
                reqwest::StatusCode::OK,
                bytes::Bytes::from_static(SAMPLE.as_bytes()),
            )),
        ]);
        let quota = fetch_with(|_, _| {
            let next = responses.pop_front().unwrap();
            async move { next }
        })
        .await
        .unwrap();
        assert_eq!(quota.plan, None);
        assert_eq!(quota.tiers.len(), 5);

        // 降级路径下 retrieveUserQuotaSummary 返回 403 时，补问一次 loadCodeAssist 拿原因
        let mut responses = VecDeque::from([
            Ok((
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                bytes::Bytes::from_static(b"oops"),
            )),
            Ok((
                reqwest::StatusCode::FORBIDDEN,
                bytes::Bytes::from_static(br#"{"error":{"message":"Forbidden"}}"#),
            )),
            Ok((
                reqwest::StatusCode::OK,
                bytes::Bytes::from_static(
                    br#"{"ineligibleTiers":[{"tierId":"free-tier","reasonMessage":"Restricted age"}]}"#,
                ),
            )),
        ]);
        let err = fetch_with(|_, _| {
            let next = responses.pop_front().unwrap();
            async move { next }
        })
        .await
        .err()
        .unwrap();
        assert!(matches!(err, QuotaError::Ineligible(ref msg) if msg == "Restricted age"));
    }
}
