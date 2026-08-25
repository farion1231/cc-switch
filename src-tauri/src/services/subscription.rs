//! 官方订阅额度查询服务
//!
//! 读取 CLI 工具的已有 OAuth 凭据，查询官方订阅额度。
//! 第一层：仅读取凭据，不实现登录/刷新。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashMap;

use crate::config;

// ── 数据类型 ──────────────────────────────────────────────

/// 凭据状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Valid,
    Expired,
    NotFound,
    ParseError,
}

/// 单个限速窗口（如 5小时会话、7天周期）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaTier {
    /// 窗口标识：five_hour, seven_day, seven_day_opus, seven_day_sonnet 等
    pub name: String,
    /// 使用百分比 0–100
    pub utilization: f64,
    /// ISO 8601 重置时间
    pub resets_at: Option<String>,
    /// ZenMux: 已用额度（USD）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_value_usd: Option<f64>,
    /// ZenMux: 窗口上限（USD）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_value_usd: Option<f64>,
}

/// 超额使用信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
    pub currency: Option<String>,
}

/// 一张可用的 Codex 限速重置卡（仅包含展示所需字段）。
///
/// 刻意不向前端暴露 credit id，避免只读额度界面意外演变为消费接口。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredit {
    #[serde(default)]
    pub granted_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Codex 限速重置卡摘要。
///
/// `credits = None` 表示明细接口不可用，但 `available_count` 仍来自额度接口；
/// `credits = Some([])` 表示明细接口成功且没有可展示的可用卡。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredits {
    pub available_count: i64,
    #[serde(default)]
    pub credits: Option<Vec<RateLimitResetCredit>>,
}

/// Codex / ChatGPT 当前订阅周期。
///
/// 这与 OAuth/JWT `exp`、额度窗口 `reset_at`、重置卡 `expires_at` 都是不同概念。
/// `active_until` 来自 OpenAI 签发的专用订阅 claim，或同账号的 ChatGPT 订阅元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMembership {
    pub active_until: String,
    #[serde(default)]
    pub will_renew: Option<bool>,
}

/// 订阅额度查询结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQuota {
    pub tool: String,
    pub credential_status: CredentialStatus,
    pub credential_message: Option<String>,
    pub success: bool,
    pub tiers: Vec<QuotaTier>,
    pub extra_usage: Option<ExtraUsage>,
    /// Codex / ChatGPT 当前套餐类型。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    /// Codex / ChatGPT 当前订阅周期；私有订阅元数据不可用时保持为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<CodexMembership>,
    /// Codex 限速重置卡；其他工具与旧响应保持无此字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_reset_credits: Option<RateLimitResetCredits>,
    pub error: Option<String>,
    pub queried_at: Option<i64>,
}

impl SubscriptionQuota {
    pub(crate) fn not_found(tool: &str) -> Self {
        Self {
            tool: tool.to_string(),
            credential_status: CredentialStatus::NotFound,
            credential_message: None,
            success: false,
            tiers: vec![],
            extra_usage: None,
            plan_type: None,
            membership: None,
            rate_limit_reset_credits: None,
            error: None,
            queried_at: None,
        }
    }

    pub(crate) fn error(tool: &str, status: CredentialStatus, message: String) -> Self {
        Self {
            tool: tool.to_string(),
            credential_status: status,
            credential_message: Some(message.clone()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            plan_type: None,
            membership: None,
            rate_limit_reset_credits: None,
            error: Some(message),
            queried_at: Some(now_millis()),
        }
    }
}

// ── Claude 凭据读取 ──────────────────────────────────────

/// Claude OAuth 凭据文件中的嵌套结构
#[derive(Deserialize)]
struct ClaudeOAuthEntry {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<serde_json::Value>,
}

/// 读取 Claude OAuth 凭据
///
/// 按优先级尝试以下来源：
/// 1. macOS Keychain (service: "Claude Code-credentials")
/// 2. 凭据文件 ~/.claude/.credentials.json
///
/// JSON 格式（两种 key 都兼容）：
/// {"claudeAiOauth": {"accessToken": "...", "expiresAt": ...}}
/// {"claude.ai_oauth": {"accessToken": "...", "expiresAt": ...}}
fn read_claude_credentials() -> (Option<String>, CredentialStatus, Option<String>) {
    // 来源 1: macOS Keychain
    #[cfg(target_os = "macos")]
    {
        if let Some(result) = read_claude_credentials_from_keychain() {
            return result;
        }
    }

    // 来源 2: 凭据文件
    read_claude_credentials_from_file()
}

/// 从 macOS Keychain 读取 Claude 凭据
#[cfg(target_os = "macos")]
fn read_claude_credentials_from_keychain(
) -> Option<(Option<String>, CredentialStatus, Option<String>)> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None; // Keychain 中无此条目，回退到文件
    }

    let json_str = String::from_utf8(output.stdout).ok()?;
    let json_str = json_str.trim();
    if json_str.is_empty() {
        return None;
    }

    Some(parse_claude_credentials_json(json_str))
}

/// 从文件读取 Claude 凭据
fn read_claude_credentials_from_file() -> (Option<String>, CredentialStatus, Option<String>) {
    let cred_path = config::get_claude_config_dir().join(".credentials.json");

    if !cred_path.exists() {
        return (None, CredentialStatus::NotFound, None);
    }

    let content = match std::fs::read_to_string(&cred_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to read credentials file: {e}")),
            );
        }
    };

    parse_claude_credentials_json(&content)
}

/// 解析 Claude 凭据 JSON（Keychain 和文件共用）
fn parse_claude_credentials_json(
    content: &str,
) -> (Option<String>, CredentialStatus, Option<String>) {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse credentials JSON: {e}")),
            );
        }
    };

    // 兼容两种 key 名
    let entry_value = parsed
        .get("claudeAiOauth")
        .or_else(|| parsed.get("claude.ai_oauth"));

    let entry_value = match entry_value {
        Some(v) => v,
        None => {
            return (
                None,
                CredentialStatus::ParseError,
                Some("No OAuth entry found in credentials".to_string()),
            );
        }
    };

    let entry: ClaudeOAuthEntry = match serde_json::from_value(entry_value.clone()) {
        Ok(e) => e,
        Err(e) => {
            return (
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse OAuth entry: {e}")),
            );
        }
    };

    let access_token = match entry.access_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return (
                None,
                CredentialStatus::ParseError,
                Some("accessToken is empty or missing".to_string()),
            );
        }
    };

    // 检查 token 是否过期
    if let Some(expires_at) = entry.expires_at {
        if is_token_expired(&expires_at) {
            return (
                Some(access_token),
                CredentialStatus::Expired,
                Some("OAuth token has expired".to_string()),
            );
        }
    }

    (Some(access_token), CredentialStatus::Valid, None)
}

/// 判断 token 是否过期，兼容 Unix 时间戳（秒/毫秒）和 ISO 字符串
fn is_token_expired(expires_at: &serde_json::Value) -> bool {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match expires_at {
        serde_json::Value::Number(n) => {
            if let Some(ts) = n.as_u64() {
                // 区分秒和毫秒（毫秒级时间戳大于 1e12）
                let ts_secs = if ts > 1_000_000_000_000 {
                    ts / 1000
                } else {
                    ts
                };
                ts_secs < now_secs
            } else {
                false
            }
        }
        serde_json::Value::String(s) => {
            // 尝试解析 ISO 8601 格式
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                (dt.timestamp() as u64) < now_secs
            } else if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
            {
                (dt.and_utc().timestamp() as u64) < now_secs
            } else {
                false // 无法解析时不视为过期
            }
        }
        _ => false,
    }
}

// ── Claude API 查询 ──────────────────────────────────────

/// Claude OAuth 用量 API 响应中的单个窗口
#[derive(Deserialize)]
struct ApiUsageWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

/// Claude OAuth 用量 API 响应中的超额用量
#[derive(Deserialize)]
struct ApiExtraUsage {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
    currency: Option<String>,
}

/// 已知的 Claude 用量窗口名称。`QuotaTier::name` 会是其中之一。
pub const TIER_FIVE_HOUR: &str = "five_hour";
pub const TIER_SEVEN_DAY: &str = "seven_day";
pub const TIER_SEVEN_DAY_OPUS: &str = "seven_day_opus";
pub const TIER_SEVEN_DAY_SONNET: &str = "seven_day_sonnet";

/// Coding Plan（Kimi / MiniMax）的周窗口 tier 名。与 `coding_plan::query_*`
/// 写入、tray 渲染、commands::provider 扁平化三处共用同一标识。
pub const TIER_WEEKLY_LIMIT: &str = "weekly_limit";

/// 月窗口 tier 名。火山方舟 Agent Plan / Coding Plan 有 5h / 周 / 月 三个展示
/// 窗口（Kimi / MiniMax 只有 5h + 周），月窗口共用此标识；前端 `TIER_I18N_KEYS`
/// 映射到 `subscription.monthly`。
pub const TIER_MONTHLY: &str = "monthly";

/// Codex 免费方案的 30 天（月）滚动窗口 tier 名。付费方案的次要窗口是 7 天
/// (`seven_day`)，免费方案则是 30 天。由 `window_seconds_to_tier_name` 产出、
/// tray 的月分组渲染、前端 `TIER_I18N_KEYS` 映射到 `subscription.thirtyDay`
/// 三处共用同一标识。见 #3651。
pub const TIER_THIRTY_DAY: &str = "30_day";

/// Grok credit 额度窗口的兜底 tier 名。Grok 账单接口只返回一个 credit 用量
/// 窗口，`subscription_grok::tier_name_for_reset` 按重置距离优先映射到
/// `weekly_limit` / `monthly`，两者都不匹配时用此标识；前端 `TIER_I18N_KEYS`
/// 映射到 `subscription.credits`，tray 归入 "c" 分组。
pub const TIER_CREDITS: &str = "credits";

/// Gemini 用量分组名称（按模型而非时间窗口）。`classify_gemini_model` 输出。
pub const TIER_GEMINI_PRO: &str = "gemini_pro";
pub const TIER_GEMINI_FLASH: &str = "gemini_flash";
pub const TIER_GEMINI_FLASH_LITE: &str = "gemini_flash_lite";

const KNOWN_TIERS: &[&str] = &[
    TIER_FIVE_HOUR,
    TIER_SEVEN_DAY,
    TIER_SEVEN_DAY_OPUS,
    TIER_SEVEN_DAY_SONNET,
];

/// 查询 Claude 官方订阅额度
///
/// 瞬时传输失败（网络/超时/读体中断）返回 `Err`（前端 reject → retry + 保留上次
/// 成功值）；确定性失败（鉴权/非 2xx/响应体非法 JSON）返回 `Ok(success:false)`。
/// codex/gemini 两个查询函数遵守同一约定。
async fn query_claude_quota(access_token: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota::error(
            "claude",
            CredentialStatus::Expired,
            format!("Authentication failed (HTTP {status}). Please re-login with Claude CLI."),
        ));
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(SubscriptionQuota::error(
            "claude",
            CredentialStatus::Valid,
            format!("API error (HTTP {status}): {body}"),
        ));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read API response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "claude",
                CredentialStatus::Valid,
                format!("Failed to parse API response: {e}"),
            ));
        }
    };

    // 解析已知的 tier 窗口
    let mut tiers = Vec::new();
    for &tier_name in KNOWN_TIERS {
        if let Some(window) = body.get(tier_name) {
            if let Ok(w) = serde_json::from_value::<ApiUsageWindow>(window.clone()) {
                if let Some(util) = w.utilization {
                    tiers.push(QuotaTier {
                        name: tier_name.to_string(),
                        utilization: util,
                        resets_at: w.resets_at,
                        used_value_usd: None,
                        max_value_usd: None,
                    });
                }
            }
        }
    }

    // 也解析未知窗口（API 可能返回新的窗口类型）
    if let Some(obj) = body.as_object() {
        for (key, value) in obj {
            if key == "extra_usage" || KNOWN_TIERS.contains(&key.as_str()) {
                continue;
            }
            if let Ok(w) = serde_json::from_value::<ApiUsageWindow>(value.clone()) {
                if let Some(util) = w.utilization {
                    tiers.push(QuotaTier {
                        name: key.clone(),
                        utilization: util,
                        resets_at: w.resets_at,
                        used_value_usd: None,
                        max_value_usd: None,
                    });
                }
            }
        }
    }

    // 解析超额使用
    let extra_usage = body.get("extra_usage").and_then(|v| {
        serde_json::from_value::<ApiExtraUsage>(v.clone())
            .ok()
            .map(|e| ExtraUsage {
                is_enabled: e.is_enabled.unwrap_or(false),
                monthly_limit: e.monthly_limit,
                used_credits: e.used_credits,
                utilization: e.utilization,
                currency: e.currency,
            })
    });

    Ok(SubscriptionQuota {
        tool: "claude".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage,
        plan_type: None,
        membership: None,
        rate_limit_reset_credits: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── Codex 凭据读取 ──────────────────────────────────────

#[derive(Deserialize)]
struct CodexAuthJson {
    auth_mode: Option<String>,
    tokens: Option<CodexTokens>,
    last_refresh: Option<String>,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: Option<String>,
    id_token: Option<String>,
    account_id: Option<String>,
}

/// (access_token, id_token, account_id, status, message)
type CodexCredentials = (
    Option<String>,
    Option<String>,
    Option<String>,
    CredentialStatus,
    Option<String>,
);

/// 读取 Codex OAuth 凭据
///
/// 按优先级尝试以下来源：
/// 1. macOS Keychain (service: "Codex Auth")
/// 2. 凭据文件 ~/.codex/auth.json
///
/// 仅 auth_mode == "chatgpt" (OAuth) 时有效，API key 模式不支持用量查询。
fn read_codex_credentials() -> CodexCredentials {
    #[cfg(target_os = "macos")]
    {
        if let Some(result) = read_codex_credentials_from_keychain() {
            return result;
        }
    }

    read_codex_credentials_from_file()
}

/// 从 macOS Keychain 读取 Codex 凭据
#[cfg(target_os = "macos")]
fn read_codex_credentials_from_keychain() -> Option<CodexCredentials> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Codex Auth", "-w"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8(output.stdout).ok()?;
    let json_str = json_str.trim();
    if json_str.is_empty() {
        return None;
    }

    Some(parse_codex_credentials_json(json_str))
}

/// 从文件读取 Codex 凭据
fn read_codex_credentials_from_file() -> CodexCredentials {
    let auth_path = crate::codex_config::get_codex_auth_path();

    if !auth_path.exists() {
        return (None, None, None, CredentialStatus::NotFound, None);
    }

    let content = match std::fs::read_to_string(&auth_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                None,
                None,
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to read Codex auth file: {e}")),
            );
        }
    };

    parse_codex_credentials_json(&content)
}

/// 解析 Codex 凭据 JSON（Keychain 和文件共用）
fn parse_codex_credentials_json(content: &str) -> CodexCredentials {
    let auth: CodexAuthJson = match serde_json::from_str(content) {
        Ok(a) => a,
        Err(e) => {
            return (
                None,
                None,
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse Codex auth JSON: {e}")),
            );
        }
    };

    // 仅 OAuth 模式有用量数据
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return (
            None,
            None,
            None,
            CredentialStatus::NotFound,
            Some("Codex not using OAuth mode".to_string()),
        );
    }

    let tokens = match auth.tokens {
        Some(t) => t,
        None => {
            return (
                None,
                None,
                None,
                CredentialStatus::ParseError,
                Some("No tokens in Codex auth".to_string()),
            );
        }
    };

    let access_token = match tokens.access_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return (
                None,
                None,
                None,
                CredentialStatus::ParseError,
                Some("access_token is empty or missing".to_string()),
            );
        }
    };

    // 检查 token 是否可能过期（距上次刷新 > 8 天）
    if let Some(ref last_refresh) = auth.last_refresh {
        if is_codex_token_stale(last_refresh) {
            return (
                Some(access_token),
                tokens.id_token,
                tokens.account_id,
                CredentialStatus::Expired,
                Some("Codex token may be stale (>8 days since last refresh)".to_string()),
            );
        }
    }

    (
        Some(access_token),
        tokens.id_token,
        tokens.account_id,
        CredentialStatus::Valid,
        None,
    )
}

/// 判断 Codex token 是否可能过期（Codex CLI 在 >8 天时自动刷新）
fn is_codex_token_stale(last_refresh: &str) -> bool {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last_refresh) {
        let age_secs = now_secs.saturating_sub(dt.timestamp() as u64);
        age_secs > 8 * 24 * 3600
    } else {
        false
    }
}

// ── Codex API 查询 ──────────────────────────────────────

const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_RESET_CREDITS_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CODEX_SUBSCRIPTIONS_URL: &str = "https://chatgpt.com/backend-api/subscriptions";

fn codex_authenticated_get(
    client: &reqwest::Client,
    url: &str,
    access_token: &str,
    account_id: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = client
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json");
    if let Some(id) = account_id {
        request = request.header("ChatGPT-Account-Id", id);
    }
    request
}

#[derive(Deserialize)]
struct CodexRateLimitWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<i64>,
    reset_at: Option<i64>,
}

#[derive(Deserialize)]
struct CodexRateLimit {
    primary_window: Option<CodexRateLimitWindow>,
    secondary_window: Option<CodexRateLimitWindow>,
}

#[derive(Deserialize)]
struct CodexRateLimitResetCreditsSummary {
    available_count: i64,
}

#[derive(Deserialize)]
struct CodexRateLimitResetCreditsDetails {
    #[serde(default)]
    credits: Vec<CodexRateLimitResetCreditDetails>,
    #[serde(default)]
    available_count: Option<i64>,
}

#[derive(Deserialize)]
struct CodexRateLimitResetCreditDetails {
    #[serde(default)]
    status: Option<String>,
    #[serde(default, alias = "type")]
    reset_type: Option<String>,
    granted_at: Option<serde_json::Value>,
    expires_at: Option<serde_json::Value>,
    title: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct CodexUsageResponse {
    rate_limit: Option<CodexRateLimit>,
}

fn codex_timestamp_to_iso(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
                return Some(parsed.to_rfc3339());
            }
            let timestamp = trimmed.parse::<i64>().ok()?;
            unix_timestamp_value_to_iso(timestamp)
        }
        serde_json::Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|raw| i64::try_from(raw).ok()))
            .and_then(unix_timestamp_value_to_iso),
        _ => None,
    }
}

fn unix_timestamp_value_to_iso(mut timestamp: i64) -> Option<String> {
    if timestamp > 1_000_000_000_000 {
        timestamp /= 1000;
    }
    unix_ts_to_iso(timestamp)
}

fn decode_codex_jwt_payload(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// 只读取明确命名的订阅 claim；绝不将 JWT `exp` 当作会员期限。
fn codex_membership_from_jwt(
    token: Option<&str>,
    expected_account_id: Option<&str>,
) -> Option<CodexMembership> {
    let payload = decode_codex_jwt_payload(token?)?;
    let auth = payload.get("https://api.openai.com/auth")?;
    if let Some(expected) = expected_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let token_account_id = auth
            .get("chatgpt_account_id")
            .or_else(|| auth.get("account_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        if token_account_id != expected {
            return None;
        }
    }
    let active_until = codex_timestamp_to_iso(auth.get("chatgpt_subscription_active_until"))?;
    Some(CodexMembership {
        active_until,
        will_renew: None,
    })
}

fn codex_membership_from_tokens(
    id_token: Option<&str>,
    access_token: &str,
    expected_account_id: Option<&str>,
) -> Option<CodexMembership> {
    codex_membership_from_jwt(id_token, expected_account_id)
        .or_else(|| codex_membership_from_jwt(Some(access_token), expected_account_id))
}

fn codex_membership_from_subscriptions(raw: &serde_json::Value) -> Option<CodexMembership> {
    let active_until = codex_timestamp_to_iso(
        raw.get("active_until")
            .or_else(|| raw.get("subscription_active_until"))
            .or_else(|| raw.get("expires_at")),
    )?;
    Some(CodexMembership {
        active_until,
        will_renew: raw.get("will_renew").and_then(serde_json::Value::as_bool),
    })
}

fn merge_codex_membership(
    preferred: Option<CodexMembership>,
    fallback: Option<CodexMembership>,
) -> Option<CodexMembership> {
    preferred.or(fallback)
}

/// 可选增强字段单独从宽松 JSON 中读取。即使后端临时改变这些字段的类型，
/// 原有 5h / 7d 核心额度仍能正常反序列化和显示。
fn codex_plan_type(raw: &serde_json::Value) -> Option<String> {
    raw.get("plan_type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .and_then(|value| trimmed_optional_text(Some(value), 64))
}

fn codex_reset_credits_summary(
    raw: &serde_json::Value,
) -> Option<CodexRateLimitResetCreditsSummary> {
    raw.get("rate_limit_reset_credits")
        .and_then(|value| value.get("available_count"))
        .and_then(serde_json::Value::as_i64)
        .map(|available_count| CodexRateLimitResetCreditsSummary { available_count })
}

fn trimmed_optional_text(value: Option<String>, max_chars: usize) -> Option<String> {
    value
        .map(|value| value.trim().chars().take(max_chars).collect::<String>())
        .filter(|value| !value.is_empty())
}

fn reset_credit_expiry_sort_key(expires_at: Option<&str>) -> i64 {
    expires_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
        .unwrap_or(i64::MAX)
}

fn is_available_codex_reset_credit(credit: &CodexRateLimitResetCreditDetails) -> bool {
    let status_matches = credit
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.eq_ignore_ascii_case("available"))
        .unwrap_or(true);
    let type_matches = credit
        .reset_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.eq_ignore_ascii_case("codex_rate_limits")
                || value.eq_ignore_ascii_case("rate_limit_reset")
        })
        .unwrap_or(true);
    status_matches && type_matches
}

/// 优先采用明细接口的权威快照；明细不可用时降级为 `/wham/usage` 的数量。
fn merge_codex_reset_credits(
    summary: Option<CodexRateLimitResetCreditsSummary>,
    details: Option<CodexRateLimitResetCreditsDetails>,
) -> Option<RateLimitResetCredits> {
    if let Some(details) = details {
        let mut credits = details
            .credits
            .into_iter()
            .filter(is_available_codex_reset_credit)
            .map(|credit| RateLimitResetCredit {
                granted_at: codex_timestamp_to_iso(credit.granted_at.as_ref()),
                expires_at: codex_timestamp_to_iso(credit.expires_at.as_ref()),
                title: trimmed_optional_text(credit.title, 160),
                description: trimmed_optional_text(credit.description, 500),
            })
            .collect::<Vec<_>>();
        let available_count = details
            .available_count
            .or_else(|| summary.map(|value| value.available_count))
            .unwrap_or_else(|| i64::try_from(credits.len()).unwrap_or(i64::MAX))
            .max(0);
        let detail_limit = usize::try_from(available_count).unwrap_or(usize::MAX);
        credits.sort_by_key(|credit| reset_credit_expiry_sort_key(credit.expires_at.as_deref()));
        credits.truncate(detail_limit);
        return Some(RateLimitResetCredits {
            available_count,
            credits: Some(credits),
        });
    }

    summary.map(|summary| RateLimitResetCredits {
        available_count: summary.available_count.max(0),
        credits: None,
    })
}

fn decode_codex_reset_credit_details(
    status: reqwest::StatusCode,
    raw: &[u8],
) -> Option<CodexRateLimitResetCreditsDetails> {
    if !status.is_success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let payload = value.get("data").unwrap_or(&value).clone();
    serde_json::from_value(payload).ok()
}

fn codex_reset_credits_request(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
) -> reqwest::RequestBuilder {
    codex_authenticated_get(client, CODEX_RESET_CREDITS_URL, access_token, account_id)
        .header("OpenAI-Beta", "codex-1")
        .header("originator", "Codex Desktop")
}

async fn fetch_codex_reset_credit_details(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    should_fetch: bool,
) -> Option<CodexRateLimitResetCreditsDetails> {
    if !should_fetch {
        return None;
    }
    let response = codex_reset_credits_request(client, access_token, account_id)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let status = response.status();
    let raw = response.bytes().await.ok()?;
    decode_codex_reset_credit_details(status, &raw)
}

fn decode_codex_membership_response(
    status: reqwest::StatusCode,
    raw: &[u8],
) -> Option<CodexMembership> {
    if !status.is_success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    codex_membership_from_subscriptions(&value)
}

fn codex_membership_request(
    client: &reqwest::Client,
    access_token: &str,
    account_id: &str,
) -> reqwest::RequestBuilder {
    codex_authenticated_get(
        client,
        CODEX_SUBSCRIPTIONS_URL,
        access_token,
        Some(account_id),
    )
    .query(&[("account_id", account_id)])
    .header("OpenAI-Beta", "codex-1")
    .header("originator", "Codex Desktop")
    .header("Referer", "https://chatgpt.com/")
    .header("x-openai-target-path", "/backend-api/subscriptions")
    .header("x-openai-target-route", "/backend-api/subscriptions")
}

/// ChatGPT Web 的订阅元数据是未公开接口，只作为同账号、只读、尽力而为的兜底。
/// 任何失败都返回 `None`，不能拖垮原有额度查询。
async fn fetch_codex_membership(
    client: &reqwest::Client,
    access_token: &str,
    account_id: Option<&str>,
    should_fetch: bool,
) -> Option<CodexMembership> {
    if !should_fetch {
        return None;
    }
    let account_id = account_id?.trim();
    if account_id.is_empty() {
        return None;
    }

    let response = codex_membership_request(client, access_token, account_id)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let status = response.status();
    let raw = response.bytes().await.ok()?;
    decode_codex_membership_response(status, &raw)
}

/// 根据窗口秒数映射到 tier 名称（与 Claude 的命名兼容以复用前端 i18n）
fn window_seconds_to_tier_name(secs: i64) -> String {
    match secs {
        18000 => TIER_FIVE_HOUR.to_string(),
        604800 => TIER_SEVEN_DAY.to_string(),
        // Codex 免费方案的 30 天窗口。显式映射到常量，与 tray 月分组、前端
        // TIER_I18N_KEYS 保持同一标识（否则动态回退虽也得到 "30_day"，但字符串
        // 分散在多处、易和托盘/前端白名单脱节）。见 #3651。
        2_592_000 => TIER_THIRTY_DAY.to_string(),
        s => {
            let hours = s / 3600;
            if hours >= 24 {
                format!("{}_day", hours / 24)
            } else {
                format!("{}_hour", hours)
            }
        }
    }
}

/// Unix 时间戳（秒）转 ISO 8601 字符串
fn unix_ts_to_iso(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
}

/// 查询 Codex / ChatGPT 反代订阅额度
///
/// 参数化 `tool_label` 和 `expired_message` 让该函数可被两个调用点共用：
/// - `"codex"` + "Please re-login with Codex CLI."（CLI 凭据路径）
/// - `"codex_oauth"` + "Please re-login via cc-switch."（cc-switch 自管 OAuth 路径）
pub(crate) async fn query_codex_quota(
    access_token: &str,
    id_token: Option<&str>,
    account_id: Option<&str>,
    tool_label: &str,
    expired_message: &str,
) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let usage_req = codex_authenticated_get(&client, CODEX_USAGE_URL, access_token, account_id);

    let resp = match usage_req
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Expired,
            format!("{expired_message} (HTTP {status})"),
        ));
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(SubscriptionQuota::error(
            tool_label,
            CredentialStatus::Valid,
            format!("API error (HTTP {status}): {body}"),
        ));
    }

    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read API response: {e}")),
    };
    let raw_value: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                tool_label,
                CredentialStatus::Valid,
                format!("Failed to parse API response: {e}"),
            ));
        }
    };
    let body: CodexUsageResponse = match serde_json::from_value(raw_value.clone()) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                tool_label,
                CredentialStatus::Valid,
                format!("Failed to parse API response: {e}"),
            ));
        }
    };
    let plan_type = codex_plan_type(&raw_value);
    let rate_limit_reset_credits = codex_reset_credits_summary(&raw_value);
    // Some Codex credential versions put the dedicated claim in the ID token,
    // others in the access token. Opaque access tokens simply fail JWT decoding.
    let token_membership = codex_membership_from_tokens(id_token, access_token, account_id);

    // 会员元数据与重置卡明细彼此独立，并行获取，额外等待上限仍为 5 秒。
    // 两者都是 best-effort：任何 HTTP、超时或 JSON 错误都不能影响核心额度。
    let should_fetch_reset_credit_details = rate_limit_reset_credits
        .as_ref()
        .is_some_and(|summary| summary.available_count > 0);
    let (reset_credit_details, live_membership) = tokio::join!(
        fetch_codex_reset_credit_details(
            &client,
            access_token,
            account_id,
            should_fetch_reset_credit_details,
        ),
        fetch_codex_membership(
            &client,
            access_token,
            account_id,
            token_membership.is_none(),
        ),
    );

    // OpenAI 签发的专用订阅 claim 优先；claim 缺失时才请求同一稳定 account_id
    // 的订阅元数据。不接受默认账号、付费账号或首条记录回退。订阅端点的
    // `expires_at` 只在该上下文中读取；JWT 路径绝不把通用 token `exp` 当期限。
    let membership = merge_codex_membership(token_membership, live_membership);

    let rate_limit = body.rate_limit;
    let mut tiers = Vec::new();

    if let Some(rate_limit) = rate_limit {
        for window in [rate_limit.primary_window, rate_limit.secondary_window]
            .into_iter()
            .flatten()
        {
            if let Some(used) = window.used_percent {
                tiers.push(QuotaTier {
                    name: window
                        .limit_window_seconds
                        .map(window_seconds_to_tier_name)
                        .unwrap_or_else(|| "unknown".to_string()),
                    utilization: used,
                    resets_at: window.reset_at.and_then(unix_ts_to_iso),
                    used_value_usd: None,
                    max_value_usd: None,
                });
            }
        }
    }

    Ok(SubscriptionQuota {
        tool: tool_label.to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        plan_type,
        membership,
        rate_limit_reset_credits: merge_codex_reset_credits(
            rate_limit_reset_credits,
            reset_credit_details,
        ),
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── Gemini 凭据读取 ──────────────────────────────────────

/// Gemini OAuth 凭据文件格式（~/.gemini/oauth_creds.json）
#[derive(Deserialize)]
struct GeminiOAuthCredsFile {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry_date: Option<i64>, // 毫秒时间戳
}

/// (access_token, refresh_token, status, message)
type GeminiCredentials = (
    Option<String>,
    Option<String>,
    CredentialStatus,
    Option<String>,
);

/// 读取 Gemini OAuth 凭据
///
/// 按优先级尝试以下来源：
/// 1. macOS Keychain (service: "gemini-cli-oauth", account: "main-account")
/// 2. 凭据文件 ~/.gemini/oauth_creds.json（遗留格式）
///
/// 仅 OAuth 认证模式（`oauth-personal`）有效；API key 模式无法查询官方用量。
fn read_gemini_credentials() -> GeminiCredentials {
    #[cfg(target_os = "macos")]
    {
        if let Some(result) = read_gemini_credentials_from_keychain() {
            return result;
        }
    }

    read_gemini_credentials_from_file()
}

/// 从 macOS Keychain 读取 Gemini 凭据
#[cfg(target_os = "macos")]
fn read_gemini_credentials_from_keychain() -> Option<GeminiCredentials> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "gemini-cli-oauth",
            "-a",
            "main-account",
            "-w",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8(output.stdout).ok()?;
    let json_str = json_str.trim();
    if json_str.is_empty() {
        return None;
    }

    Some(parse_gemini_keychain_json(json_str))
}

/// 解析 Keychain 格式的 Gemini 凭据
///
/// Keychain 格式（keytar）：
/// ```json
/// { "token": { "accessToken": "...", "refreshToken": "...", "expiresAt": 1234 }, "updatedAt": ... }
/// ```
#[cfg(target_os = "macos")]
fn parse_gemini_keychain_json(content: &str) -> GeminiCredentials {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            return (
                None,
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse Gemini keychain JSON: {e}")),
            )
        }
    };

    let token = match parsed.get("token") {
        Some(t) => t,
        None => {
            // Keychain 中可能是扁平格式，尝试文件格式解析
            return parse_gemini_file_json(content);
        }
    };

    let access_token = token
        .get("accessToken")
        .and_then(|v| v.as_str())
        .map(String::from);
    let refresh_token = token
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_at = token.get("expiresAt").and_then(|v| v.as_i64());

    match access_token {
        Some(at) if !at.is_empty() => {
            // expiresAt 是毫秒时间戳
            if let Some(exp_ms) = expires_at {
                if exp_ms < now_millis() {
                    return (
                        Some(at),
                        refresh_token,
                        CredentialStatus::Expired,
                        Some("Gemini access token has expired".to_string()),
                    );
                }
            }
            (Some(at), refresh_token, CredentialStatus::Valid, None)
        }
        _ => (
            None,
            refresh_token,
            CredentialStatus::ParseError,
            Some("accessToken is empty or missing".to_string()),
        ),
    }
}

/// 从文件读取 Gemini 凭据
fn read_gemini_credentials_from_file() -> GeminiCredentials {
    let cred_path = crate::gemini_config::get_gemini_dir().join("oauth_creds.json");
    if !cred_path.exists() {
        return (None, None, CredentialStatus::NotFound, None);
    }

    let content = match std::fs::read_to_string(&cred_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                None,
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to read Gemini credentials: {e}")),
            )
        }
    };

    parse_gemini_file_json(&content)
}

/// 解析文件格式的 Gemini 凭据
///
/// 文件格式（oauth_creds.json）：
/// ```json
/// { "access_token": "...", "refresh_token": "...", "expiry_date": 1234 }
/// ```
fn parse_gemini_file_json(content: &str) -> GeminiCredentials {
    let creds: GeminiOAuthCredsFile = match serde_json::from_str(content) {
        Ok(c) => c,
        Err(e) => {
            return (
                None,
                None,
                CredentialStatus::ParseError,
                Some(format!("Failed to parse Gemini credentials: {e}")),
            )
        }
    };

    let access_token = match creds.access_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return (
                None,
                creds.refresh_token,
                CredentialStatus::ParseError,
                Some("access_token is empty or missing".to_string()),
            )
        }
    };

    // expiry_date 是毫秒时间戳
    if let Some(exp_ms) = creds.expiry_date {
        if exp_ms < now_millis() {
            return (
                Some(access_token),
                creds.refresh_token,
                CredentialStatus::Expired,
                Some("Gemini access token has expired".to_string()),
            );
        }
    }

    (
        Some(access_token),
        creds.refresh_token,
        CredentialStatus::Valid,
        None,
    )
}

// ── Gemini Token 刷新 ──────────────────────────────────────

/// Gemini OAuth Client 凭据（公开值，来自 Gemini CLI 源码 google-gemini/gemini-cli）
const GEMINI_OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GEMINI_OAUTH_CLIENT_SECRET: &str = "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";

/// 使用 refresh_token 刷新 Gemini access token
///
/// Google OAuth access_token 仅有 ~1h 有效期，需要定期用 refresh_token 刷新。
/// refresh_token 本身不过期（除非用户撤销授权）。
async fn refresh_gemini_token(refresh_token: &str) -> Option<String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", GEMINI_OAUTH_CLIENT_ID),
            ("client_secret", GEMINI_OAUTH_CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    body.get("access_token")?.as_str().map(String::from)
}

// ── Gemini API 查询 ──────────────────────────────────────

/// loadCodeAssist 响应
#[derive(Deserialize)]
struct GeminiLoadCodeAssistResponse {
    #[serde(rename = "cloudaicompanionProject")]
    cloudaicompanion_project: Option<serde_json::Value>,
}

/// 配额 bucket
#[derive(Deserialize)]
struct GeminiBucketInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
}

/// retrieveUserQuota 响应
#[derive(Deserialize)]
struct GeminiQuotaResponse {
    buckets: Option<Vec<GeminiBucketInfo>>,
}

/// 从 loadCodeAssist 响应中提取项目 ID
fn extract_project_id(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(obj) => obj
            .get("id")
            .or_else(|| obj.get("projectId"))
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    }
}

/// 将 Gemini 模型 ID 分类为 Pro / Flash / Flash Lite
fn classify_gemini_model(model_id: &str) -> &str {
    if model_id.contains("flash-lite") {
        TIER_GEMINI_FLASH_LITE
    } else if model_id.contains("flash") {
        TIER_GEMINI_FLASH
    } else if model_id.contains("pro") {
        TIER_GEMINI_PRO
    } else {
        model_id
    }
}

/// 查询 Gemini 官方订阅额度
///
/// 两步 API 调用：
/// 1. loadCodeAssist → 获取 cloudaicompanionProject
/// 2. retrieveUserQuota → 获取按模型分桶的配额数据
async fn query_gemini_quota(access_token: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    // ── Step 1: loadCodeAssist 获取项目 ID ──
    let load_resp = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "metadata": {
                "ideType": "GEMINI_CLI",
                "pluginType": "GEMINI"
            }
        }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let load_resp = match load_resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error (loadCodeAssist): {e}")),
    };

    let load_status = load_resp.status();
    if load_status == reqwest::StatusCode::UNAUTHORIZED
        || load_status == reqwest::StatusCode::FORBIDDEN
    {
        return Ok(SubscriptionQuota::error(
            "gemini",
            CredentialStatus::Expired,
            format!("Authentication failed (HTTP {load_status}). Please re-login with Gemini CLI."),
        ));
    }
    if !load_status.is_success() {
        let body = load_resp.text().await.unwrap_or_default();
        return Ok(SubscriptionQuota::error(
            "gemini",
            CredentialStatus::Valid,
            format!("loadCodeAssist failed (HTTP {load_status}): {body}"),
        ));
    }

    let load_raw = match load_resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read loadCodeAssist response: {e}")),
    };
    let load_body: GeminiLoadCodeAssistResponse = match serde_json::from_slice(&load_raw) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "gemini",
                CredentialStatus::Valid,
                format!("Failed to parse loadCodeAssist response: {e}"),
            ));
        }
    };

    let project_id = load_body
        .cloudaicompanion_project
        .as_ref()
        .and_then(extract_project_id);

    // ── Step 2: retrieveUserQuota 获取配额 ──
    let mut quota_body = serde_json::json!({});
    if let Some(ref pid) = project_id {
        quota_body["project"] = serde_json::Value::String(pid.clone());
    }

    let quota_resp = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .json(&quota_body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let quota_resp = match quota_resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error (retrieveUserQuota): {e}")),
    };

    let quota_status = quota_resp.status();
    if quota_status == reqwest::StatusCode::UNAUTHORIZED
        || quota_status == reqwest::StatusCode::FORBIDDEN
    {
        return Ok(SubscriptionQuota::error(
            "gemini",
            CredentialStatus::Expired,
            format!("Authentication failed (HTTP {quota_status})."),
        ));
    }
    if !quota_status.is_success() {
        let body = quota_resp.text().await.unwrap_or_default();
        return Ok(SubscriptionQuota::error(
            "gemini",
            CredentialStatus::Valid,
            format!("retrieveUserQuota failed (HTTP {quota_status}): {body}"),
        ));
    }

    let quota_raw = match quota_resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read quota response: {e}")),
    };
    let quota_data: GeminiQuotaResponse = match serde_json::from_slice(&quota_raw) {
        Ok(v) => v,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "gemini",
                CredentialStatus::Valid,
                format!("Failed to parse quota response: {e}"),
            ));
        }
    };

    // ── 按模型分类汇总，每类取最低 remainingFraction ──
    let mut category_map: HashMap<String, (f64, Option<String>)> = HashMap::new();

    if let Some(buckets) = quota_data.buckets {
        for bucket in buckets {
            let model_id = bucket.model_id.as_deref().unwrap_or("unknown");
            let category = classify_gemini_model(model_id).to_string();
            let remaining = bucket.remaining_fraction.unwrap_or(1.0).clamp(0.0, 1.0);

            let entry = category_map
                .entry(category)
                .or_insert((remaining, bucket.reset_time.clone()));
            if remaining < entry.0 {
                entry.0 = remaining;
                if bucket.reset_time.is_some() {
                    entry.1.clone_from(&bucket.reset_time);
                }
            }
        }
    }

    // 转换为 tiers（remainingFraction → utilization: 已用百分比）
    let sort_order = |name: &str| -> usize {
        match name {
            TIER_GEMINI_PRO => 0,
            TIER_GEMINI_FLASH => 1,
            TIER_GEMINI_FLASH_LITE => 2,
            _ => 3,
        }
    };

    let mut tiers: Vec<QuotaTier> = category_map
        .into_iter()
        .map(|(name, (remaining, reset_time))| QuotaTier {
            name,
            utilization: (1.0 - remaining) * 100.0,
            resets_at: reset_time,
            used_value_usd: None,
            max_value_usd: None,
        })
        .collect();

    tiers.sort_by_key(|t| sort_order(&t.name));

    Ok(SubscriptionQuota {
        tool: "gemini".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        plan_type: None,
        membership: None,
        rate_limit_reset_credits: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── 入口函数 ──────────────────────────────────────────────

/// 查询指定 CLI 工具的官方订阅额度
///
/// 瞬时传输失败以 `Err` 传播（前端 reject → retry + 保留上次成功值）。Expired
/// 分支的"过期也试一把"重试同样用 `?` 传播瞬时错误——不能折叠成"已过期"，
/// 否则一次网络抖动会被误报成确定性的凭据过期。
pub async fn get_subscription_quota(tool: &str) -> Result<SubscriptionQuota, String> {
    match tool {
        "claude" => {
            let (token, status, message) = read_claude_credentials();

            match status {
                CredentialStatus::NotFound => Ok(SubscriptionQuota::not_found("claude")),
                CredentialStatus::ParseError => Ok(SubscriptionQuota::error(
                    "claude",
                    CredentialStatus::ParseError,
                    message.unwrap_or_else(|| "Failed to parse credentials".to_string()),
                )),
                CredentialStatus::Expired => {
                    // 即使过期也尝试调用 API（token 可能实际上仍有效）
                    if let Some(token) = token {
                        let result = query_claude_quota(&token).await?;
                        if result.success {
                            return Ok(result);
                        }
                    }
                    Ok(SubscriptionQuota::error(
                        "claude",
                        CredentialStatus::Expired,
                        message.unwrap_or_else(|| "OAuth token has expired".to_string()),
                    ))
                }
                CredentialStatus::Valid => {
                    let token = token.expect("token must be Some when status is Valid");
                    query_claude_quota(&token).await
                }
            }
        }
        "codex" => {
            let (token, id_token, account_id, status, message) = read_codex_credentials();

            match status {
                CredentialStatus::NotFound => Ok(SubscriptionQuota::not_found("codex")),
                CredentialStatus::ParseError => Ok(SubscriptionQuota::error(
                    "codex",
                    CredentialStatus::ParseError,
                    message.unwrap_or_else(|| "Failed to parse credentials".to_string()),
                )),
                CredentialStatus::Expired => {
                    // 即使可能过期也尝试调用 API
                    if let Some(token) = token {
                        let result = query_codex_quota(
                            &token,
                            id_token.as_deref(),
                            account_id.as_deref(),
                            "codex",
                            "Authentication failed. Please re-login with Codex CLI.",
                        )
                        .await?;
                        if result.success {
                            return Ok(result);
                        }
                    }
                    Ok(SubscriptionQuota::error(
                        "codex",
                        CredentialStatus::Expired,
                        message.unwrap_or_else(|| "Codex OAuth token may be stale".to_string()),
                    ))
                }
                CredentialStatus::Valid => {
                    let token = token.expect("token must be Some when status is Valid");
                    query_codex_quota(
                        &token,
                        id_token.as_deref(),
                        account_id.as_deref(),
                        "codex",
                        "Authentication failed. Please re-login with Codex CLI.",
                    )
                    .await
                }
            }
        }
        "gemini" => {
            let (token, refresh_token, status, message) = read_gemini_credentials();

            match status {
                CredentialStatus::NotFound => Ok(SubscriptionQuota::not_found("gemini")),
                CredentialStatus::ParseError => Ok(SubscriptionQuota::error(
                    "gemini",
                    CredentialStatus::ParseError,
                    message.unwrap_or_else(|| "Failed to parse credentials".to_string()),
                )),
                CredentialStatus::Expired => {
                    // Gemini access_token 仅 ~1h 有效，尝试用 refresh_token 刷新
                    if let Some(ref rt) = refresh_token {
                        if let Some(new_token) = refresh_gemini_token(rt).await {
                            return query_gemini_quota(&new_token).await;
                        }
                    }
                    // 刷新失败，尝试用旧 token
                    if let Some(ref token) = token {
                        let result = query_gemini_quota(token).await?;
                        if result.success {
                            return Ok(result);
                        }
                    }
                    Ok(SubscriptionQuota::error(
                        "gemini",
                        CredentialStatus::Expired,
                        message.unwrap_or_else(|| "Gemini OAuth token has expired".to_string()),
                    ))
                }
                CredentialStatus::Valid => {
                    let token = token.expect("token must be Some when status is Valid");
                    query_gemini_quota(&token).await
                }
            }
        }
        "grokbuild" => crate::services::subscription_grok::get_grok_subscription_quota().await,
        _ => Ok(SubscriptionQuota::not_found(tool)),
    }
}

// ── 辅助函数 ──────────────────────────────────────────────

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_codex_jwt(payload: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{header}.{payload}.signature")
    }

    #[test]
    fn window_seconds_map_to_expected_tier_names() {
        // 官方特例窗口
        assert_eq!(window_seconds_to_tier_name(18000), TIER_FIVE_HOUR);
        assert_eq!(window_seconds_to_tier_name(604800), TIER_SEVEN_DAY);
        // Codex 免费方案的次要窗口是 30 天（30 * 24 * 3600 = 2_592_000 秒）。
        // 前端 TIER_I18N_KEYS 与 tray 月分组都需要认得 "30_day"，见 #3651。
        assert_eq!(window_seconds_to_tier_name(2_592_000), TIER_THIRTY_DAY);
        // 其他窗口按小时/天回退命名
        assert_eq!(window_seconds_to_tier_name(3600), "1_hour");
        assert_eq!(window_seconds_to_tier_name(86400), "1_day");
    }

    #[test]
    fn codex_optional_enrichments_are_tolerant_and_keep_future_plan_names() {
        let raw = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12.5,
                    "limit_window_seconds": 18000,
                    "reset_at": 1_800_000_000
                }
            },
            "plan_type": "prolite",
            "rate_limit_reset_credits": { "available_count": 2 }
        });

        let core: CodexUsageResponse = serde_json::from_value(raw.clone()).unwrap();
        assert!(core.rate_limit.is_some());
        assert_eq!(codex_plan_type(&raw).as_deref(), Some("prolite"));
        assert_eq!(
            codex_reset_credits_summary(&raw).map(|summary| summary.available_count),
            Some(2)
        );

        // 可选增强字段的临时类型漂移不能拖垮原有 rate_limit 解析。
        let malformed_enrichments = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12.5,
                    "limit_window_seconds": 18000
                }
            },
            "plan_type": { "name": "plus" },
            "rate_limit_reset_credits": { "available_count": "2" }
        });
        let core: CodexUsageResponse =
            serde_json::from_value(malformed_enrichments.clone()).unwrap();
        assert!(core.rate_limit.is_some());
        assert_eq!(codex_plan_type(&malformed_enrichments), None);
        assert!(codex_reset_credits_summary(&malformed_enrichments).is_none());
    }

    #[test]
    fn codex_membership_uses_only_the_named_claim_and_checks_account_scope() {
        let token_exp_only = unsigned_codex_jwt(serde_json::json!({
            "exp": 1_900_000_000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-a"
            }
        }));
        assert!(codex_membership_from_jwt(Some(&token_exp_only), Some("account-a")).is_none());

        let subscription_token = unsigned_codex_jwt(serde_json::json!({
            "exp": 1_700_000_000,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-a",
                "chatgpt_subscription_active_until": "2030-01-02T03:04:05Z"
            }
        }));
        let membership =
            codex_membership_from_jwt(Some(&subscription_token), Some("account-a")).unwrap();
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&membership.active_until)
                .unwrap()
                .timestamp(),
            chrono::DateTime::parse_from_rfc3339("2030-01-02T03:04:05Z")
                .unwrap()
                .timestamp()
        );
        assert_eq!(membership.will_renew, None);
        assert!(codex_membership_from_jwt(Some(&subscription_token), Some("account-b")).is_none());

        let unscoped_subscription_token = unsigned_codex_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_subscription_active_until": "2030-01-02T03:04:05Z"
            }
        }));
        assert!(
            codex_membership_from_jwt(Some(&unscoped_subscription_token), Some("account-a"))
                .is_none()
        );
    }

    #[test]
    fn codex_membership_token_fallback_checks_id_then_access_token() {
        let id_token_without_membership = unsigned_codex_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-a"
            }
        }));
        let access_token_with_membership = unsigned_codex_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "account-a",
                "chatgpt_subscription_active_until": "2032-03-04T05:06:07Z"
            }
        }));

        let membership = codex_membership_from_tokens(
            Some(&id_token_without_membership),
            &access_token_with_membership,
            Some("account-a"),
        )
        .unwrap();
        assert!(membership.active_until.starts_with("2032-03-04T05:06:07"));
    }

    #[test]
    fn codex_membership_claim_accepts_seconds_and_milliseconds() {
        for raw in [
            serde_json::json!(1_900_000_000_i64),
            serde_json::json!(1_900_000_000_000_i64),
            serde_json::json!("1900000000"),
        ] {
            let token = unsigned_codex_jwt(serde_json::json!({
                "https://api.openai.com/auth": {
                    "chatgpt_subscription_active_until": raw
                }
            }));
            let membership = codex_membership_from_jwt(Some(&token), None).unwrap();
            assert_eq!(
                chrono::DateTime::parse_from_rfc3339(&membership.active_until)
                    .unwrap()
                    .timestamp(),
                1_900_000_000
            );
        }
    }

    #[test]
    fn subscriptions_membership_accepts_endpoint_expiry_and_claim_wins() {
        let endpoint_expiry = codex_membership_from_subscriptions(&serde_json::json!({
            "expires_at": "2099-01-01T00:00:00Z",
            "will_renew": true
        }))
        .unwrap();
        assert!(endpoint_expiry
            .active_until
            .starts_with("2099-01-01T00:00:00"));

        let live = codex_membership_from_subscriptions(&serde_json::json!({
            "active_until": "2031-02-03T04:05:06Z",
            "will_renew": false
        }))
        .unwrap();
        let claim = CodexMembership {
            active_until: "2030-01-02T03:04:05Z".to_string(),
            will_renew: None,
        };
        let merged = merge_codex_membership(Some(claim), Some(live)).unwrap();
        assert_eq!(merged.will_renew, None);
        assert!(merged.active_until.starts_with("2030-01-02T03:04:05"));
    }

    #[test]
    fn membership_http_and_json_failures_are_best_effort() {
        let valid = br#"{
            "active_until": "2030-01-02T03:04:05Z",
            "will_renew": true
        }"#;
        assert!(decode_codex_membership_response(reqwest::StatusCode::OK, valid).is_some());
        assert!(decode_codex_membership_response(reqwest::StatusCode::OK, b"not-json").is_none());
        for status in [401, 403, 404, 429, 500, 503] {
            assert!(decode_codex_membership_response(
                reqwest::StatusCode::from_u16(status).unwrap(),
                valid
            )
            .is_none());
        }
    }

    #[test]
    fn reset_credit_details_filter_to_available_codex_cards_without_ids() {
        let details: CodexRateLimitResetCreditsDetails =
            serde_json::from_value(serde_json::json!({
                "available_count": 2,
                "total_earned_count": 9,
                "credits": [
                    {
                        "id": "must-not-leave-rust",
                        "reset_type": "codex_rate_limits",
                        "status": "available",
                        "granted_at": 1900000000,
                        "expires_at": 1900003600000_i64,
                        "title": "  Full reset  ",
                        "description": "  Ready to redeem  ",
                        "profile_user_id": "must-not-leave-rust"
                    },
                    {
                        "id": "redeemed",
                        "reset_type": "codex_rate_limits",
                        "status": "redeemed",
                        "granted_at": "2026-06-01T00:00:00Z",
                        "expires_at": null
                    },
                    {
                        "id": "legacy-without-status-or-type",
                        "granted_at": "2098-01-01T00:00:00Z",
                        "expires_at": "2099-01-01T00:00:00Z",
                        "title": "Legacy reset"
                    },
                    {
                        "id": "different-product",
                        "reset_type": "future_reset_type",
                        "status": "available",
                        "granted_at": "2026-06-02T00:00:00Z",
                        "expires_at": null
                    }
                ]
            }))
            .unwrap();
        let merged = merge_codex_reset_credits(None, Some(details)).unwrap();

        // 数量来自明细响应，而不是过滤后的列表长度。
        assert_eq!(merged.available_count, 2);
        let credits = merged.credits.unwrap();
        assert_eq!(credits.len(), 2);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(credits[0].granted_at.as_deref().unwrap())
                .unwrap()
                .timestamp(),
            1_900_000_000
        );
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(credits[0].expires_at.as_deref().unwrap())
                .unwrap()
                .timestamp(),
            1_900_003_600
        );
        assert_eq!(credits[0].title.as_deref(), Some("Full reset"));
        assert_eq!(credits[0].description.as_deref(), Some("Ready to redeem"));

        let serialized = serde_json::to_value(&credits[0]).unwrap();
        assert!(serialized.get("id").is_none());
        assert!(serialized.get("status").is_none());
        assert!(serialized.get("profileUserId").is_none());
    }

    #[test]
    fn reset_credit_summary_survives_missing_details_and_distinguishes_zero() {
        let count_only = merge_codex_reset_credits(
            Some(CodexRateLimitResetCreditsSummary { available_count: 3 }),
            None,
        )
        .unwrap();
        assert_eq!(count_only.available_count, 3);
        assert!(count_only.credits.is_none());

        let zero = merge_codex_reset_credits(
            Some(CodexRateLimitResetCreditsSummary { available_count: 0 }),
            None,
        )
        .unwrap();
        assert_eq!(zero.available_count, 0);
        assert!(merge_codex_reset_credits(None, None).is_none());
    }

    #[test]
    fn reset_credit_detail_http_and_json_failures_are_best_effort() {
        let valid = br#"{
            "available_count": 1,
            "credits": [{
                "reset_type": "codex_rate_limits",
                "status": "available",
                "granted_at": "2026-07-01T00:00:00Z",
                "expires_at": null
            }]
        }"#;
        assert!(decode_codex_reset_credit_details(reqwest::StatusCode::OK, valid).is_some());
        let nested = br#"{
            "data": {
                "credits": [{"status": "available", "expires_at": 1900000000}]
            }
        }"#;
        let nested = decode_codex_reset_credit_details(reqwest::StatusCode::OK, nested).unwrap();
        assert_eq!(nested.available_count, None);
        assert_eq!(nested.credits.len(), 1);
        assert!(decode_codex_reset_credit_details(reqwest::StatusCode::OK, b"not-json").is_none());

        for status in [401, 403, 404, 429, 500, 503] {
            assert!(decode_codex_reset_credit_details(
                reqwest::StatusCode::from_u16(status).unwrap(),
                valid,
            )
            .is_none());
        }
    }

    #[test]
    fn legacy_subscription_quota_deserializes_without_new_fields() {
        let quota: SubscriptionQuota = serde_json::from_value(serde_json::json!({
            "tool": "codex",
            "credentialStatus": "valid",
            "credentialMessage": null,
            "success": true,
            "tiers": [],
            "extraUsage": null,
            "error": null,
            "queriedAt": 0
        }))
        .unwrap();

        assert!(quota.plan_type.is_none());
        assert!(quota.membership.is_none());
        assert!(quota.rate_limit_reset_credits.is_none());
    }

    #[test]
    fn usage_and_reset_detail_requests_share_the_same_account_scope() {
        let client = reqwest::Client::new();
        let usage =
            codex_authenticated_get(&client, CODEX_USAGE_URL, "test-token", Some("account-a"))
                .build()
                .unwrap();
        let details = codex_reset_credits_request(&client, "test-token", Some("account-a"))
            .build()
            .unwrap();
        let membership = codex_membership_request(&client, "test-token", "account-a")
            .build()
            .unwrap();

        assert_eq!(usage.url().as_str(), CODEX_USAGE_URL);
        assert_eq!(details.url().as_str(), CODEX_RESET_CREDITS_URL);
        assert_eq!(
            details
                .headers()
                .get("OpenAI-Beta")
                .and_then(|value| value.to_str().ok()),
            Some("codex-1")
        );
        assert_eq!(
            details
                .headers()
                .get("originator")
                .and_then(|value| value.to_str().ok()),
            Some("Codex Desktop")
        );
        assert_eq!(membership.url().path(), "/backend-api/subscriptions");
        assert!(membership
            .url()
            .query_pairs()
            .any(|(key, value)| key == "account_id" && value == "account-a"));
        for request in [&usage, &details, &membership] {
            assert_eq!(
                request
                    .headers()
                    .get("ChatGPT-Account-Id")
                    .and_then(|value| value.to_str().ok()),
                Some("account-a")
            );
            assert_eq!(
                request
                    .headers()
                    .get("Authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer test-token")
            );
        }
    }
}
