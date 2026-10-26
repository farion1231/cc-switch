//! Antigravity CLI 官方登录的剩余额度：问 agy 自己的 `/usage` 命令
//! （`agy -p /usage --output-format json`），这条命令不调模型、不新建会话。
//!
//! 这是后备路径：能切号的平台（macOS）先用 CC Switch 存的令牌直接问 Google
//! （`commands::agy_account::agy_quota`，约 1 秒），认不出 agy 的登录时才走这里；
//! 启动 agy 要七八秒。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};

const TOOL: &str = "agy";
const TIMEOUT: Duration = Duration::from_secs(30);

pub const TIER_AGY_GEMINI_FIVE_HOUR: &str = "agy_gemini_five_hour";
pub const TIER_AGY_GEMINI_WEEKLY: &str = "agy_gemini_weekly";
pub const TIER_AGY_THIRD_PARTY_FIVE_HOUR: &str = "agy_third_party_five_hour";
pub const TIER_AGY_THIRD_PARTY_WEEKLY: &str = "agy_third_party_weekly";

#[derive(Deserialize)]
struct UsageOutput {
    status: Option<String>,
    command: Option<UsageCommand>,
}

#[derive(Deserialize)]
struct UsageCommand {
    data: Option<UsageData>,
}

#[derive(Deserialize)]
struct UsageData {
    #[serde(default)]
    groups: Vec<UsageGroup>,
}

#[derive(Deserialize)]
struct UsageGroup {
    #[serde(default)]
    buckets: Vec<UsageBucket>,
}

#[derive(Deserialize)]
struct UsageBucket {
    id: Option<String>,
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

/// agy 的额度分组：Gemini 一组，Claude 和 GPT 共用一组，每组各有 5 小时和每周两档。
/// 不认识的桶（以后新增的组）跳过，免得显示成没有文案的档。
fn tier_name(bucket_id: &str) -> Option<&'static str> {
    match bucket_id {
        "gemini-5h" => Some(TIER_AGY_GEMINI_FIVE_HOUR),
        "gemini-weekly" => Some(TIER_AGY_GEMINI_WEEKLY),
        "3p-5h" => Some(TIER_AGY_THIRD_PARTY_FIVE_HOUR),
        "3p-weekly" => Some(TIER_AGY_THIRD_PARTY_WEEKLY),
        _ => None,
    }
}

fn sort_order(name: &str) -> usize {
    match name {
        TIER_AGY_GEMINI_FIVE_HOUR => 0,
        TIER_AGY_GEMINI_WEEKLY => 1,
        TIER_AGY_THIRD_PARTY_FIVE_HOUR => 2,
        _ => 3,
    }
}

fn parse_usage(stdout: &[u8]) -> Option<Vec<QuotaTier>> {
    let output: UsageOutput = serde_json::from_slice(stdout).ok()?;
    if output.status.as_deref() != Some("SUCCESS") {
        return None;
    }
    let groups = output.command?.data?.groups;
    collect_tiers(
        groups
            .into_iter()
            .flat_map(|group| group.buckets)
            .map(|bucket| (bucket.id, bucket.remaining_fraction, bucket.reset_time)),
    )
}

/// 一组额度桶（id、剩余比例、重置时间）→ 排好序的档；一档都认不出时返回 `None`。
/// agy 的 `/usage` 和直接查 Google 额度接口（`agy_account::quota`）返回的是同一组桶。
pub(crate) fn collect_tiers(
    buckets: impl IntoIterator<Item = (Option<String>, Option<f64>, Option<String>)>,
) -> Option<Vec<QuotaTier>> {
    let mut tiers: Vec<QuotaTier> = buckets
        .into_iter()
        .filter_map(|(id, remaining, reset_time)| {
            let name = tier_name(id.as_deref()?)?;
            let remaining = remaining?.clamp(0.0, 1.0);
            Some(QuotaTier {
                name: name.to_string(),
                utilization: (1.0 - remaining) * 100.0,
                resets_at: reset_time,
                used_value_usd: None,
                max_value_usd: None,
            })
        })
        .collect();
    if tiers.is_empty() {
        return None;
    }
    tiers.sort_by_key(|tier| sort_order(&tier.name));
    Some(tiers)
}

/// 由 `subscription::get_subscription_quota("agy")` 分发。没装 agy 时按「没有凭据」处理（卡片不显示）；
/// 超时、没登录或输出看不懂都写成查询失败，错误里不带 agy 的原始输出。
pub(crate) async fn get_agy_subscription_quota() -> Result<SubscriptionQuota, String> {
    let Some(bin) = tokio::task::spawn_blocking(crate::agy_config::real_agy_bin)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(SubscriptionQuota::not_found(TOOL));
    };
    let mut command = crate::agy_config::background_command(&bin);
    command
        .args(["--print", "/usage", "--output-format", "json"])
        // 只问官方登录：GUI 进程里残留的 Key 会让 agy 改走 Key 模式。
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_GEMINI_BASE_URL")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return Ok(SubscriptionQuota::not_found(TOOL)),
        Err(_) => return Err("Antigravity quota query timed out".to_string()),
    };
    let tiers = output
        .status
        .success()
        .then(|| parse_usage(&output.stdout))
        .flatten();
    let Some(tiers) = tiers else {
        return Ok(SubscriptionQuota::error(
            TOOL,
            CredentialStatus::Valid,
            "Antigravity did not return quota. Sign in with agy and try again.".to_string(),
        ));
    };
    Ok(SubscriptionQuota {
        tool: TOOL.to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        reset_credits: None,
        credits_balance: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    // agy 1.4.3 `/usage --output-format json` 的真实结构（数值改过）
    const SAMPLE: &str = r#"{
      "conversation_id": "", "status": "SUCCESS", "response": "...",
      "usage": {"input_tokens": 0, "output_tokens": 0},
      "command": {"name": "usage", "data": {"description": "...", "groups": [
        {"name": "Gemini Models", "buckets": [
          {"id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.75, "reset_time": "2026-10-27T15:13:36Z"},
          {"id": "gemini-5h", "window": "5h", "remaining_fraction": 1, "reset_time": "2026-10-21T04:16:37Z"}
        ]},
        {"name": "Claude and GPT models", "buckets": [
          {"id": "3p-weekly", "window": "weekly", "remaining_fraction": 0.1, "reset_time": "2026-10-27T23:16:37Z"},
          {"id": "3p-5h", "window": "5h", "remaining_fraction": 0, "reset_time": "2026-10-21T04:16:37Z"},
          {"id": "future-bucket", "remaining_fraction": 0.5}
        ]}
      ]}}
    }"#;

    #[test]
    fn keeps_each_group_and_window_as_its_own_tier() {
        let tiers = parse_usage(SAMPLE.as_bytes()).unwrap();
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
        assert_eq!(used, [0.0, 25.0, 100.0, 90.0]);
        assert_eq!(tiers[1].resets_at.as_deref(), Some("2026-10-27T15:13:36Z"));
    }

    #[test]
    fn failed_or_empty_output_is_not_quota() {
        assert!(parse_usage(b"not json").is_none());
        assert!(parse_usage(br#"{"status":"ERROR","command":null}"#).is_none());
        assert!(parse_usage(br#"{"status":"SUCCESS","command":{"data":{"groups":[]}}}"#).is_none());
        // 缺比例的桶不当成满额
        assert!(parse_usage(
            br#"{"status":"SUCCESS","command":{"data":{"groups":[{"buckets":[{"id":"gemini-5h"}]}]}}}"#
        )
        .is_none());
    }
}
