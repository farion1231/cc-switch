//! Codex OAuth Authentication Module
//!
//! 实现 OpenAI ChatGPT Plus/Pro 订阅的 OAuth Device Code 流程。
//! 支持多账号管理，每个 Provider 可关联不同的 ChatGPT 账号。
//!
//! ## 认证流程
//! 1. 启动 Device Code 流程，获取 device_auth_id 和 user_code
//! 2. 用户在浏览器中完成 ChatGPT 授权
//! 3. 轮询获取 authorization_code 和 code_verifier（注意：verifier 由服务端返回）
//! 4. 使用 code + verifier 换取 access_token + refresh_token + id_token
//! 5. 自动刷新 access_token（到期前 60 秒）
//!
//! ## 多账号支持
//! - 每个 ChatGPT 账号独立存储 refresh_token
//! - Provider 通过 meta.authBinding 关联账号（auth_provider = "codex_oauth"）
//! - 通过 JWT id_token 提取 chatgpt_account_id 作为账号唯一标识

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};

/// OpenAI OAuth 客户端 ID（OpenCode 使用，与官方 Codex CLI 相同）
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Device Code 启动 URL
const DEVICE_AUTH_USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";

/// Device Code 轮询 URL
const DEVICE_AUTH_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";

/// OAuth Token URL（用于 code 换 token 和 refresh token）
const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// Device Code 验证 URL（向用户展示）
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";

/// Device Code 流程的 redirect_uri（OpenAI 服务端约定）
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";

/// Token 刷新提前量（毫秒）
const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;

/// OAuth token/device 端点的单请求超时。共享 HTTP client 默认 600s 超时是给
/// 大模型流式响应用的，对认证请求过长；网络卡住时应尽快失败而非长时间阻塞。
const OAUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Device Code 默认有效时长（秒），OpenAI 文档约定 15 分钟
const DEVICE_CODE_DEFAULT_EXPIRES_IN: u64 = 900;

/// 轮询间隔安全余量（秒）
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;

/// User-Agent
const CODEX_USER_AGENT: &str = "cc-switch-codex-oauth";

/// Codex OAuth 错误
#[derive(Debug, thiserror::Error)]
pub enum CodexOAuthError {
    #[error("等待用户授权中")]
    AuthorizationPending,

    #[error("用户拒绝授权")]
    AccessDenied,

    #[error("Device Code 已过期")]
    ExpiredToken,

    #[error("OAuth Token 获取失败: {0}")]
    TokenFetchFailed(String),

    #[error("Refresh Token 失效或已过期")]
    RefreshTokenInvalid,

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("账号不存在: {0}")]
    AccountNotFound(String),
}

impl From<reqwest::Error> for CodexOAuthError {
    fn from(err: reqwest::Error) -> Self {
        CodexOAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CodexOAuthError {
    fn from(err: std::io::Error) -> Self {
        CodexOAuthError::IoError(err.to_string())
    }
}

/// OpenAI Device Code 响应
#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// OpenAI Device Code 轮询响应（成功）
#[derive(Debug, Clone, Deserialize)]
struct DevicePollSuccess {
    authorization_code: String,
    code_verifier: String,
}

/// OAuth Token 响应
#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// 解析后的 JWT claims（仅关心 chatgpt_account_id、email、user_id、sub 等字段）
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct IdTokenClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    organizations: Vec<OrgClaim>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaim>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct OrgClaim {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct OpenAiAuthClaim {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
}

impl IdTokenClaims {
    fn extract_chatgpt_account_id(&self) -> Option<String> {
        self.chatgpt_account_id
            .as_deref()
            .or_else(|| {
                self.openai_auth
                    .as_ref()
                    .and_then(|a| a.chatgpt_account_id.as_deref())
            })
            .or_else(|| self.organizations.first().and_then(|o| o.id.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn email_claim(&self) -> Option<&str> {
        non_empty(self.email.as_deref())
    }

    fn user_id_claim(&self) -> Option<&str> {
        non_empty(
            self.openai_auth
                .as_ref()
                .and_then(|auth| auth.user_id.as_deref()),
        )
    }

    fn sub_claim(&self) -> Option<&str> {
        non_empty(self.sub.as_deref())
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 一个账号从 token claims 里能拿到的全部身份信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AccountIdentity {
    /// 账号主键使用的个人身份。
    ///
    /// 只有 id_token 完全没有个人 claim 时才退到 access_token：access_token 会被
    /// Codex CLI 自刷新轮换，拿它参与主键会让同一个账号在换代后算出不同的 key，
    /// 于是 `~/.codex/auth.json` 再也认不回账号、CLI 轮换出的 refresh_token 被孤儿化。
    primary: Option<String>,
    /// 两个 token 里出现过的全部个人标识，用于「是否同一个人」的宽容比对。
    candidates: Vec<String>,
    /// 展示用 email，跨两个 token 收集（主键不一定用它）。
    email: Option<String>,
}

impl AccountIdentity {
    pub(crate) fn from_claims(
        id_claims: Option<&IdTokenClaims>,
        access_claims: Option<&IdTokenClaims>,
    ) -> Self {
        let mut candidates = Vec::new();
        let mut collect = |claims: Option<&IdTokenClaims>| -> Option<String> {
            let claims = claims?;
            // 优先级 user_id > sub > email：前两者是 OpenAI 侧不可变的不透明标识，
            // email 可以被用户改掉，用它当主键会让同一个账号改名后变成两条记录。
            let mut best = None;
            for value in [
                claims.user_id_claim().map(str::to_string),
                claims.sub_claim().map(str::to_string),
                claims.email_claim().map(normalize_email),
            ] {
                let Some(value) = value else { continue };
                if !candidates.contains(&value) {
                    candidates.push(value.clone());
                }
                best.get_or_insert(value);
            }
            best
        };

        // 两个 token 都要走一遍，否则 access_token 的候选不会被收集。
        let from_id_token = collect(id_claims);
        let from_access_token = collect(access_claims);
        let email = id_claims
            .and_then(IdTokenClaims::email_claim)
            .or_else(|| access_claims.and_then(IdTokenClaims::email_claim))
            .map(str::to_string);

        Self {
            primary: from_id_token.or(from_access_token),
            candidates,
            email,
        }
    }

    /// `user` 是否是这份 claims 描述的同一个人。
    ///
    /// 比对全部候选而不是只比 `primary()`：token 换代后 claim 集合可能变化
    /// （例如新 id_token 不再带 `user_id`），只要原来那个标识仍在就应当认得出来。
    fn matches_user(&self, user: &str) -> bool {
        self.candidates.iter().any(|candidate| candidate == user)
    }
}

/// email 统一小写：同一个账号在不同登录里回传 `Alice@x` / `alice@x` 时不能变成两条记录。
fn normalize_email(email: &str) -> String {
    email.to_lowercase()
}

/// 账号主键 `用户身份::工作区` 的分隔符。`::` 不会出现在 email / OpenAI 的
/// user_id / sub 里，因此可用它判断一个 key 是否已是复合键。
pub(crate) const ACCOUNT_ID_SEPARATOR: &str = "::";

/// 拆开复合主键。返回 `None` 表示这是升级前存下的裸工作区 key。
pub(crate) fn split_composite_account_id(account_id: &str) -> Option<(&str, &str)> {
    account_id.split_once(ACCOUNT_ID_SEPARATOR)
}

/// 这份 claims 是否属于 `account_id` 指向的账号。
///
/// 复合键必须个人身份与工作区都命中；裸工作区 key（升级前存量，用户尚未在 UI
/// 重新绑定）只能比到工作区一层——这正是它在同工作区多成员下会误判的原因。
pub(crate) fn identity_owns_account_id(
    identity: &AccountIdentity,
    claims_workspace_id: Option<&str>,
    tokens_account_id: Option<&str>,
    account_id: &str,
) -> bool {
    match split_composite_account_id(account_id) {
        Some((user, workspace)) => {
            identity.matches_user(user) && claims_workspace_id == Some(workspace)
        }
        // 裸 key 有两种：升级前存下的工作区 ID，以及没有工作区 claim 的个人账号。
        // 字符串本身分不出来，两种都要试。
        None => {
            claims_workspace_id == Some(account_id)
                || (claims_workspace_id.is_none()
                    && (identity.matches_user(account_id) || tokens_account_id == Some(account_id)))
        }
    }
}

/// 写进原生 `~/.codex/auth.json` 的 `tokens.account_id`。
///
/// 复合主键绝不能写出去：它含个人身份（可能是 email），而 Codex CLI 会把这个字段当
/// `ChatGPT-Account-Id` 发给 OpenAI。裸 key 则保持既有行为原样写入——它要么就是工作区
/// ID，要么是没有工作区的个人账号，后者本来就没有别的值可写。
pub(crate) fn native_tokens_account_id<'a>(
    chatgpt_account_id: Option<&'a str>,
    account_id: &'a str,
) -> &'a str {
    chatgpt_account_id.unwrap_or(match split_composite_account_id(account_id) {
        Some(_) => "",
        None => account_id,
    })
}

/// 由已解析的 identity 组合出 (account_id, email, workspace)。
pub(crate) fn resolve_account_identity_from(
    identity: &AccountIdentity,
    chatgpt_account_id: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let account_id =
        compose_primary_account_id(identity.primary.clone(), chatgpt_account_id.clone());

    (account_id, identity.email.clone(), chatgpt_account_id)
}

pub(crate) fn resolve_identity_and_workspace(
    id_claims: Option<&IdTokenClaims>,
    access_claims: Option<&IdTokenClaims>,
) -> (AccountIdentity, Option<String>) {
    let identity = AccountIdentity::from_claims(id_claims, access_claims);
    let chatgpt_account_id = id_claims
        .and_then(IdTokenClaims::extract_chatgpt_account_id)
        .or_else(|| access_claims.and_then(IdTokenClaims::extract_chatgpt_account_id));

    (identity, chatgpt_account_id)
}

/// 缓存的 access_token（含过期时间）
#[derive(Debug, Clone)]
struct CachedAccessToken {
    token: String,
    /// 过期时间戳（毫秒）
    expires_at_ms: i64,
    /// 获取（刷新）时间戳（毫秒）。用于写入托管 auth.json 的 `last_refresh`，
    /// 使其如实反映 access_token 的真实获取时间，而非写盘时刻——否则 Codex CLI
    /// 会误判一个旧 token 是刚刷新的。
    obtained_at_ms: i64,
}

impl CachedAccessToken {
    fn is_expiring_soon(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expires_at_ms - now < TOKEN_REFRESH_BUFFER_MS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshTokenAdoptionMode {
    /// Normal CLI synchronization: different token material must carry a
    /// strictly newer live timestamp before it can replace manager state.
    TimestampChecked,
    /// The OAuth server has just rejected the manager refresh token. A
    /// different same-account token observed on disk is therefore the only
    /// viable recovery generation and may bypass timestamp ambiguity.
    RejectedManagerToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshTokenAdoptionOutcome {
    /// The live and manager token material already describe the same
    /// generation. `state_changed` only reflects timestamp bookkeeping.
    Synchronized { state_changed: bool },
    /// Different live token material was accepted as the newer generation.
    Adopted,
    /// Different live token material carries a timestamp strictly older than
    /// the manager generation and may therefore be overwritten or removed.
    ProvablyOlder,
    /// Different token material could not be ordered safely. Callers that are
    /// about to overwrite/delete auth.json must abort instead of guessing.
    Ambiguous,
    /// The account is not owned by this manager.
    NotManaged,
}

impl RefreshTokenAdoptionOutcome {
    fn state_changed(self) -> bool {
        matches!(
            self,
            Self::Synchronized {
                state_changed: true
            } | Self::Adopted
        )
    }
}

/// 进行中的 Device Code 条目，带过期时间以便清理放弃的登录流程
#[derive(Debug, Clone)]
struct PendingDeviceCode {
    user_code: String,
    /// Unix 毫秒时间戳，超时后可清理
    expires_at_ms: i64,
}

/// 持久化的账号数据
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAccountData {
    /// 账号唯一标识（同时作为 HashMap 的 key，优先使用 email / user_id / sub）
    pub account_id: String,
    /// 账号邮箱（如果可获取）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// OpenAI chatgpt_account_id / workspace ID（如果可获取）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_account_id: Option<String>,
    /// Refresh Token（持久化）
    pub refresh_token: String,
    /// 认证时间戳（秒）
    pub authenticated_at: i64,
    /// ChatGPT id_token（JWT，持久化）。用于让托管写入的 Codex auth.json
    /// 与原生浏览器登录保持一致的 tokens 字段形状；刷新时若返回新值则更新。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// 最近一次取得或采纳这组 OAuth token 的时间。用于在 Codex CLI 与
    /// cc-switch 都可能轮换 refresh_token 时拒绝从 live 采纳更旧的一代。
    #[serde(default)]
    pub token_updated_at_ms: i64,
}

impl CodexAccountData {
    /// 已存下的工作区 ID。
    fn stored_workspace_id(&self) -> Option<String> {
        non_empty(self.chatgpt_account_id.as_deref()).map(str::to_string)
    }

    /// 当前 id_token claims 里的工作区 ID。
    fn claimed_workspace_id(&self) -> Option<String> {
        self.id_token
            .as_deref()
            .and_then(parse_jwt_claims)
            .and_then(|c| c.extract_chatgpt_account_id())
    }

    /// 日常读取：已存的字段优先，存量账号没有该字段时才回落到 claims。
    pub fn effective_chatgpt_account_id(&self) -> Option<String> {
        self.stored_workspace_id()
            .or_else(|| self.claimed_workspace_id())
    }

    /// 刷新 / 采纳新 token 后重新解析工作区 ID。
    ///
    /// 与 `effective_chatgpt_account_id()` 的取值顺序相反：先认 claims，工作区变更时
    /// 才比对得出差异。否则陈旧的工作区会继续被注入 `chatgpt-account-id` 和托管
    /// auth.json。新 token 不带工作区 claim 时保留旧值。
    pub fn synced_chatgpt_account_id(&self) -> Option<String> {
        self.claimed_workspace_id()
            .or_else(|| self.stored_workspace_id())
    }
}

/// 公开的账号信息（返回给前端，复用 GitHubAccount 结构）
impl From<&CodexAccountData> for GitHubAccount {
    fn from(data: &CodexAccountData) -> Self {
        let login = match (&data.email, &data.chatgpt_account_id) {
            (Some(email), Some(ws)) if !ws.trim().is_empty() => {
                // 按字符截断：workspace ID 来自 JWT claim，不保证是 ASCII UUID，
                // 按字节切片会在多字节字符中间 panic（这里在 list_accounts 热路径上）。
                let short_ws: String = ws.trim().chars().take(8).collect();
                format!("{email} ({short_ws})")
            }
            (Some(email), _) => email.clone(),
            (None, Some(ws)) => format!("ChatGPT ({ws})"),
            (None, None) => format!("ChatGPT ({})", data.account_id),
        };

        GitHubAccount {
            id: data.account_id.clone(),
            login,
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: "github.com".to_string(),
            // 旧账号（升级前登录）没有持久化 id_token，需重新登录补全
            reauth_required: data.id_token.is_none(),
        }
    }
}

/// 持久化存储结构（v1）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexOAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, CodexAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

/// 写入托管 Codex `auth.json` 所需的完整可刷新 token 束。
#[derive(Debug, Clone)]
pub(crate) struct ManagedTokenBundle {
    pub access_token: String,
    pub id_token: Option<String>,
    pub refresh_token: String,
    /// OpenAI Workspace / Team ID（chatgpt_account_id，写入 auth.json 里的 tokens.account_id）
    pub chatgpt_account_id: Option<String>,
    /// access_token 的真实获取时间，RFC3339 纳秒精度 + `Z`（与原生 auth.json 的
    /// `last_refresh` 形状一致）。反映 token 何时真正刷新，而非写盘时刻。
    pub last_refresh: String,
}

/// Codex OAuth 认证管理器（多账号）
pub struct CodexOAuthManager {
    accounts: Arc<RwLock<HashMap<String, CodexAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    /// 内存缓存的 access_token（不持久化）
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    /// 每个账号的刷新锁
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// 普通 token 解析/采纳持读锁，账号删除/清空持写锁。删除因此会等待
    /// 已在飞 refresh 完成，也不会因过早清理 refresh_locks 产生第二把账号锁。
    lifecycle_lock: Arc<RwLock<()>>,
    /// 进行中的 Device Code 流程：device_auth_id -> {user_code, expires_at_ms}
    /// 过期条目会在 start_device_flow 时被清理，防止放弃的登录流程导致无界增长
    pending_device_codes: Arc<RwLock<HashMap<String, PendingDeviceCode>>>,
    /// 清除全部认证时递增，使已经在网络请求中的登录流程无法重新登记。
    login_epoch: AtomicU64,
    storage_path: PathBuf,
    /// 持久化串行锁：`save_to_disk` 与 `clear_auth` 的「快照+写盘/删文件」都在此锁内
    /// 完成。此前由外层 `RwLock<CodexOAuthManager>` 的写锁隐式串行化；去掉外层锁后
    /// 需要它防止并发保存/清除交错，导致已删账号被旧快照复活。
    storage_lock: Arc<Mutex<()>>,
}

impl CodexOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("codex_oauth_auth.json");

        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_lock: Arc::new(RwLock::new(())),
            pending_device_codes: Arc::new(RwLock::new(HashMap::new())),
            login_epoch: AtomicU64::new(0),
            storage_path,
            storage_lock: Arc::new(Mutex::new(())),
        };

        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[CodexOAuth] 加载存储失败: {e}");
        }

        manager
    }

    // ==================== 设备码流程 ====================

    /// 启动 Device Code 流程
    ///
    /// 返回 GitHubDeviceCodeResponse 复用现有前端结构，但字段含义对应 OpenAI 的字段：
    /// - device_code = device_auth_id
    /// - user_code = user_code
    /// - verification_uri = https://auth.openai.com/codex/device
    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, CodexOAuthError> {
        log::info!("[CodexOAuth] 启动 Device Code 流程");
        let login_epoch = self.login_epoch.load(Ordering::Acquire);

        let response = crate::proxy::http_client::get()
            .post(DEVICE_AUTH_USERCODE_URL)
            .timeout(OAUTH_HTTP_TIMEOUT)
            .header("Content-Type", "application/json")
            .header("User-Agent", CODEX_USER_AGENT)
            .json(&serde_json::json!({ "client_id": CODEX_CLIENT_ID }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::NetworkError(format!(
                "Device Code 请求失败: {status} - {text}"
            )));
        }

        let device: DeviceCodeResponse = response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        let interval = parse_interval(device.interval.as_ref());
        let expires_in = device.expires_in.unwrap_or(DEVICE_CODE_DEFAULT_EXPIRES_IN);
        let expires_at_ms = chrono::Utc::now().timestamp_millis() + (expires_in as i64) * 1000;

        self.register_pending_device_code(
            device.device_auth_id.clone(),
            device.user_code.clone(),
            expires_at_ms,
            login_epoch,
        )
        .await?;

        log::info!(
            "[CodexOAuth] 获取 Device Code 成功，user_code: {}",
            device.user_code
        );

        Ok(GitHubDeviceCodeResponse {
            device_code: device.device_auth_id,
            user_code: device.user_code,
            verification_uri: DEVICE_VERIFICATION_URL.to_string(),
            expires_in,
            interval,
        })
    }

    async fn register_pending_device_code(
        &self,
        device_auth_id: String,
        user_code: String,
        expires_at_ms: i64,
        login_epoch: u64,
    ) -> Result<(), CodexOAuthError> {
        let mut pending = self.pending_device_codes.write().await;
        if self.login_epoch.load(Ordering::Acquire) != login_epoch {
            return Err(CodexOAuthError::ExpiredToken);
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        pending.retain(|_, entry| entry.expires_at_ms > now_ms);
        pending.insert(
            device_auth_id,
            PendingDeviceCode {
                user_code,
                expires_at_ms,
            },
        );
        Ok(())
    }

    /// 轮询 Device Code 状态
    ///
    /// 接收 device_code（即 device_auth_id），返回 Some(account) 表示授权成功
    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<GitHubAccount>, CodexOAuthError> {
        let entry = {
            let pending = self.pending_device_codes.read().await;
            pending.get(device_code).cloned()
        };

        let entry = entry.ok_or_else(|| {
            CodexOAuthError::TokenFetchFailed(
                "未找到对应的 user_code，请重新启动登录流程".to_string(),
            )
        })?;

        if entry.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            let mut pending = self.pending_device_codes.write().await;
            pending.remove(device_code);
            return Err(CodexOAuthError::ExpiredToken);
        }

        let user_code = entry.user_code;

        log::debug!("[CodexOAuth] 轮询 Device Code");

        let poll_response = crate::proxy::http_client::get()
            .post(DEVICE_AUTH_TOKEN_URL)
            .timeout(OAUTH_HTTP_TIMEOUT)
            .header("Content-Type", "application/json")
            .header("User-Agent", CODEX_USER_AGENT)
            .json(&serde_json::json!({
                "device_auth_id": device_code,
                "user_code": user_code,
            }))
            .send()
            .await?;

        let status = poll_response.status();

        // 403/404 表示用户未完成授权，继续轮询
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            return Err(CodexOAuthError::AuthorizationPending);
        }

        if status == reqwest::StatusCode::GONE {
            return Err(CodexOAuthError::ExpiredToken);
        }

        if !status.is_success() {
            let text = poll_response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "{status} - {text}"
            )));
        }

        let success: DevicePollSuccess = poll_response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        log::info!("[CodexOAuth] 用户已授权，正在换取 OAuth Token");

        // 用 authorization_code + code_verifier 换 token
        let tokens = self
            .exchange_code_for_tokens(&success.authorization_code, &success.code_verifier)
            .await?;

        let refresh_token = tokens.refresh_token.clone().ok_or_else(|| {
            CodexOAuthError::TokenFetchFailed("响应缺少 refresh_token".to_string())
        })?;

        let (account_id, email, chatgpt_account_id) = extract_identity_from_tokens(&tokens);
        let account_id = account_id.ok_or_else(|| {
            CodexOAuthError::ParseError("无法从 token 中提取 account_id".to_string())
        })?;

        let obtained_at_ms = chrono::Utc::now().timestamp_millis();
        // 登录提交与该账号的 refresh/adopt 共用一把 generation 锁；账号和
        // access cache 一次写入，旧刷新响应因此不能覆盖新登录链。
        let account = self
            .add_account_internal(
                account_id.clone(),
                refresh_token,
                email,
                chatgpt_account_id,
                // 空字符串视为缺失，避免写出空的 id_token
                tokens.id_token.clone().filter(|t| !t.trim().is_empty()),
                Some(CachedAccessToken {
                    token: tokens.access_token.clone(),
                    expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                    obtained_at_ms,
                }),
                Some(device_code),
            )
            .await?;

        Ok(Some(account))
    }

    /// 用 authorization_code + code_verifier 换取 tokens
    async fn exchange_code_for_tokens(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenResponse, CodexOAuthError> {
        let response = crate::proxy::http_client::get()
            .post(OAUTH_TOKEN_URL)
            .timeout(OAUTH_HTTP_TIMEOUT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", CODEX_USER_AGENT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", DEVICE_REDIRECT_URI),
                ("client_id", CODEX_CLIENT_ID),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "Token 交换失败: {status} - {text}"
            )));
        }

        response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))
    }

    /// 用 refresh_token 刷新 access_token
    async fn refresh_with_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, CodexOAuthError> {
        let response = crate::proxy::http_client::get()
            .post(OAUTH_TOKEN_URL)
            .timeout(OAUTH_HTTP_TIMEOUT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", CODEX_USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", CODEX_CLIENT_ID),
                ("scope", "openid profile email"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let refresh_error_code = extract_refresh_error_code(&text);
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
                || matches!(
                    refresh_error_code.as_deref(),
                    Some(
                        "refresh_token_expired"
                            | "refresh_token_reused"
                            | "refresh_token_invalidated"
                    )
                )
            {
                return Err(CodexOAuthError::RefreshTokenInvalid);
            }
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "Refresh 失败: {status} - {text}"
            )));
        }

        response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))
    }

    // ==================== Token 获取（含自动刷新） ====================

    /// 获取指定账号的有效 access_token（必要时自动刷新）
    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        Ok(self.resolve_valid_cached_token(account_id).await?.token)
    }

    /// 解析账号的有效缓存 token（含真实获取时间），必要时刷新。
    ///
    /// 返回完整 `CachedAccessToken`，使 token 与其 `obtained_at_ms` 天然配套（写托管
    /// auth.json 的 `last_refresh` 直接取用），避免分两次读缓存造成的错配。
    ///
    /// 并发正确性：调用方持 lifecycle 读锁；刷新在 account refresh mutex 下先短暂
    /// 提交 accounts → access_tokens，释放这些锁后再持久化。`save_to_disk` 的实际
    /// 持久化锁序是 storage_lock → accounts/default。remove/clear 持 lifecycle 写锁，
    /// 因而会等待在飞刷新并阻断同 account_id 的 ABA 重建。
    async fn resolve_valid_cached_token(
        &self,
        account_id: &str,
    ) -> Result<CachedAccessToken, CodexOAuthError> {
        // 快路径：确认账号存在后读缓存
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.clone());
                }
            }
        }

        log::info!("[CodexOAuth] 账号 {account_id} 的 access_token 需要刷新");

        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;
        self.resolve_valid_cached_token_under_lock(account_id).await
    }

    /// Resolve a token while the caller owns this account's refresh mutex.
    /// Keeping this separate lets the full auth-bundle path hold one generation
    /// lock across access/id/refresh reads without recursively locking the mutex.
    async fn resolve_valid_cached_token_under_lock(
        &self,
        account_id: &str,
    ) -> Result<CachedAccessToken, CodexOAuthError> {
        // Codex CLI may have advanced the shared refresh-token generation since
        // this manager last used the account. Reload it under the same per-account
        // lock before deciding whether a network refresh is necessary.
        if let Some((live_refresh, live_id_token, live_last_refresh_ms)) =
            crate::codex_config::read_codex_live_auth_refresh_for_account(account_id)
        {
            self.adopt_account_refresh_token_under_lock(
                account_id,
                live_refresh,
                live_id_token,
                live_last_refresh_ms,
                RefreshTokenAdoptionMode::TimestampChecked,
            )
            .await?;
        }

        // double-check（同样在 accounts 读锁下）
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.clone());
                }
            }
        }

        let mut refresh_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .map(|a| a.refresh_token.clone())
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?
        };

        let new_tokens = match self.refresh_with_token(&refresh_token).await {
            Err(CodexOAuthError::RefreshTokenInvalid) => {
                // If Codex CLI refreshed between our pre-read and request, reload
                // its newer generation and retry exactly once. Error-code handling
                // includes OpenAI's `refresh_token_reused` response.
                let Some((live_refresh, live_id_token, live_last_refresh_ms)) =
                    crate::codex_config::read_codex_live_auth_refresh_for_account(account_id)
                        .filter(|(token, _, _)| token.trim() != refresh_token.as_str())
                else {
                    return Err(CodexOAuthError::RefreshTokenInvalid);
                };
                let adoption = self
                    .adopt_account_refresh_token_under_lock(
                        account_id,
                        live_refresh.clone(),
                        live_id_token,
                        live_last_refresh_ms,
                        RefreshTokenAdoptionMode::RejectedManagerToken,
                    )
                    .await?;
                if !matches!(adoption, RefreshTokenAdoptionOutcome::Adopted) {
                    return Err(CodexOAuthError::RefreshTokenInvalid);
                }
                refresh_token = live_refresh;
                self.refresh_with_token(&refresh_token).await?
            }
            result => result?,
        };

        let obtained_at_ms = chrono::Utc::now().timestamp_millis();

        // 如果服务端返回了新的 refresh_token 或 id_token，更新存储
        let mut needs_save = false;
        let (stored_refresh_token, stored_id_token, stored_chatgpt_account_id) = {
            let mut accounts = self.accounts.write().await;
            let account = accounts
                .get_mut(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
            // Device re-login and CLI-token adoption use the same account lock,
            // but keep a generation CAS here as defense in depth: a response for
            // R0 must never overwrite a newly committed R1/N0 chain.
            if account.refresh_token != refresh_token {
                return Err(CodexOAuthError::TokenFetchFailed(
                    "账号凭据已更新，已丢弃旧刷新响应".to_string(),
                ));
            }
            if let Some(new_refresh) = new_tokens
                .refresh_token
                .clone()
                .filter(|token| !token.trim().is_empty())
            {
                if new_refresh != account.refresh_token {
                    account.refresh_token = new_refresh;
                    needs_save = true;
                }
            }
            // 刷新使用 openid scope，正常会返回新 id_token；为空则视为缺失，
            // 保留旧值而非覆盖（旧值的 claims 仍可用于账号/套餐显示）。
            if let Some(new_id_token) = new_tokens
                .id_token
                .clone()
                .filter(|token| !token.trim().is_empty())
            {
                if account.id_token.as_deref() != Some(new_id_token.as_str()) {
                    account.id_token = Some(new_id_token);
                    needs_save = true;
                }
            }
            // 同步更新或回填 chatgpt_account_id
            let effective_chatgpt_id = account.synced_chatgpt_account_id();
            if account.chatgpt_account_id != effective_chatgpt_id {
                account.chatgpt_account_id = effective_chatgpt_id.clone();
                needs_save = true;
            }
            if account.token_updated_at_ms != obtained_at_ms {
                account.token_updated_at_ms = obtained_at_ms;
                needs_save = true;
            }
            (
                account.refresh_token.clone(),
                account.id_token.clone(),
                effective_chatgpt_id,
            )
        };
        if needs_save {
            self.save_to_disk().await?;
        }

        let cached = CachedAccessToken {
            token: new_tokens.access_token.clone(),
            expires_at_ms: compute_expires_at_ms(new_tokens.expires_in),
            obtained_at_ms,
        };

        let last_refresh = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(obtained_at_ms)
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let live_tokens_account_id =
            native_tokens_account_id(stored_chatgpt_account_id.as_deref(), account_id);
        let refreshed_auth = crate::codex_config::codex_managed_oauth_auth_value(
            live_tokens_account_id,
            &cached.token,
            stored_id_token.as_deref(),
            &stored_refresh_token,
            &last_refresh,
        );
        if let Err(err) = crate::codex_config::sync_codex_managed_oauth_live_auth_after_refresh(
            account_id,
            &refresh_token,
            &refreshed_auth,
        ) {
            // The manager token remains valid; a later provider write will
            // retry the live synchronization without rolling it back.
            log::warn!(
                "[CodexOAuth] 同步刷新后的 Codex live auth 失败（account={account_id}）: {err}"
            );
        }

        // 在 accounts 读锁下确认账号仍存在，再写缓存：与 remove/clear（持 accounts
        // 写锁并原子清缓存）互斥，杜绝把已删账号的 token 写回缓存。
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
            let mut tokens = self.access_tokens.write().await;
            tokens.insert(account_id.to_string(), cached.clone());
        }

        Ok(cached)
    }

    /// 获取指定账号的有效 access_token 与 id_token（必要时自动刷新）
    ///
    /// id_token 用于让托管写入的 Codex auth.json 与原生浏览器登录保持
    /// 一致的 tokens 字段形状（仅托管绑定路径使用）。旧账号若无 id_token
    /// 会返回 `None`，前端据此提示重新登录。
    pub async fn get_valid_token_and_id_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<(String, Option<String>), CodexOAuthError> {
        let bundle = self.get_valid_token_bundle_for_account(account_id).await?;
        Ok((bundle.access_token, bundle.id_token))
    }

    /// 获取写入托管 Codex `auth.json` 所需的完整可刷新 token 束
    /// （access_token + id_token + refresh_token）。
    ///
    /// 与仅返回 access_token 不同：写入 Codex CLI 的 auth.json 必须携带
    /// refresh_token，否则 CLI 在 access_token 过期后无法自刷新（详见托管直连
    /// 场景 “裸跑 codex”）。
    pub(crate) async fn get_valid_token_bundle_for_account(
        &self,
        account_id: &str,
    ) -> Result<ManagedTokenBundle, CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _refresh_guard = refresh_lock.lock().await;

        // Resolve and read every persistent token field while holding the same
        // account generation lock. Otherwise an adoption between these reads
        // can create an invalid A0 + R1/ID1 mixed bundle.
        let cached = self
            .resolve_valid_cached_token_under_lock(account_id)
            .await?;

        // A managed bundle is about to overwrite auth.json. Re-read under the
        // same manager generation lock after token resolution so an ambiguous
        // same-account disk generation can never be hidden by a valid cached
        // access token. Keeping this check after resolution also preserves the
        // RefreshTokenInvalid recovery path: the server may disprove manager R0,
        // force-adopt disk R1, and only then produce a safe bundle.
        if let Some((live_refresh, live_id_token, live_last_refresh_ms)) =
            crate::codex_config::read_codex_live_auth_refresh_for_account(account_id)
        {
            let outcome = self
                .adopt_account_refresh_token_under_lock(
                    account_id,
                    live_refresh,
                    live_id_token,
                    live_last_refresh_ms,
                    RefreshTokenAdoptionMode::TimestampChecked,
                )
                .await?;
            match outcome {
                RefreshTokenAdoptionOutcome::Synchronized { .. }
                | RefreshTokenAdoptionOutcome::ProvablyOlder => {}
                RefreshTokenAdoptionOutcome::Ambiguous => {
                    return Err(Self::ambiguous_live_refresh_error(account_id));
                }
                RefreshTokenAdoptionOutcome::Adopted => {
                    return Err(CodexOAuthError::TokenFetchFailed(format!(
                        "Codex CLI 账号 {account_id} 的磁盘凭据在准备写入期间已刷新；为避免写入混合 token bundle，本次操作已取消，请重试"
                    )));
                }
                RefreshTokenAdoptionOutcome::NotManaged => {
                    return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
                }
            }
        }
        let last_refresh =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(cached.obtained_at_ms)
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let (id_token, refresh_token, chatgpt_account_id) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
            (
                account.id_token.clone(),
                account.refresh_token.clone(),
                account.effective_chatgpt_account_id(),
            )
        };
        Ok(ManagedTokenBundle {
            access_token: cached.token,
            id_token,
            refresh_token,
            chatgpt_account_id,
            last_refresh,
        })
    }

    /// 采纳（读回）Codex CLI 轮换后的 refresh_token / id_token。
    ///
    /// 托管账号以「完整 bundle」写入 auth.json 后，Codex CLI 会自行刷新并把新的
    /// refresh_token 回写 auth.json。切换回该 provider 前调用本方法，把盘上的最新
    /// refresh_token 采纳进本地存储，避免用陈腐 token 覆盖 CLI 的有效登录。
    ///
    /// 仅当账号确由本 manager 托管、且值确有变化时才更新并落盘；返回是否更新。
    pub async fn adopt_account_refresh_token(
        &self,
        account_id: &str,
        refresh_token: String,
        id_token: Option<String>,
        last_refresh_ms: Option<i64>,
    ) -> Result<bool, CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        let refresh_token = refresh_token.trim().to_string();
        if refresh_token.is_empty() {
            return Ok(false);
        }
        // 与该账号的刷新串行化：若一个 refresh 正持旧 refresh_token 在飞，避免它返回后
        // 覆盖我们刚采纳的 CLI 轮换值。
        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;
        self.adopt_account_refresh_token_under_lock(
            account_id,
            refresh_token,
            id_token,
            last_refresh_ms,
            RefreshTokenAdoptionMode::TimestampChecked,
        )
        .await
        .map(RefreshTokenAdoptionOutcome::state_changed)
    }

    fn ambiguous_live_refresh_error(account_id: &str) -> CodexOAuthError {
        CodexOAuthError::TokenFetchFailed(format!(
            "Codex CLI 账号 {account_id} 的磁盘凭据已变化，但无法安全判断 refresh token 新旧；为避免覆盖或删除有效登录，本次操作已取消。请先在认证中心重新登录该账号；若仍失败，请移除后重新登录"
        ))
    }

    /// Reconcile the same-account Codex CLI refresh generation before a
    /// provider transaction overwrites or removes live auth.json.
    ///
    /// Returns the exact refresh token observed on disk. Callers must compare
    /// it again immediately before their live write/delete; the external Codex
    /// CLI does not participate in cc-switch's switch lock and may refresh in
    /// the adopt-to-write window.
    pub(crate) async fn prepare_live_auth_for_account_switch_away(
        &self,
        account_id: &str,
    ) -> Result<Option<String>, CodexOAuthError> {
        let Some((live_refresh, live_id_token, live_last_refresh_ms)) =
            crate::codex_config::read_codex_live_auth_refresh_for_account(account_id)
        else {
            return Ok(None);
        };

        let _lifecycle = self.lifecycle_lock.read().await;
        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;
        {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
        }

        let outcome = self
            .adopt_account_refresh_token_under_lock(
                account_id,
                live_refresh.clone(),
                live_id_token,
                live_last_refresh_ms,
                RefreshTokenAdoptionMode::TimestampChecked,
            )
            .await?;

        match outcome {
            RefreshTokenAdoptionOutcome::Synchronized { .. }
            | RefreshTokenAdoptionOutcome::Adopted
            | RefreshTokenAdoptionOutcome::ProvablyOlder => Ok(Some(live_refresh)),
            RefreshTokenAdoptionOutcome::Ambiguous => {
                Err(Self::ambiguous_live_refresh_error(account_id))
            }
            RefreshTokenAdoptionOutcome::NotManaged => {
                Err(CodexOAuthError::AccountNotFound(account_id.to_string()))
            }
        }
    }

    /// Same as `adopt_account_refresh_token`, for callers already holding the
    /// per-account refresh lock.
    async fn adopt_account_refresh_token_under_lock(
        &self,
        account_id: &str,
        refresh_token: String,
        id_token: Option<String>,
        last_refresh_ms: Option<i64>,
        mode: RefreshTokenAdoptionMode,
    ) -> Result<RefreshTokenAdoptionOutcome, CodexOAuthError> {
        let incoming_id_token = id_token.filter(|token| !token.trim().is_empty());
        let mut changed = false;
        let mut material_replaced = false;
        let mut outcome;
        {
            let mut accounts = self.accounts.write().await;
            let Some(account) = accounts.get_mut(account_id) else {
                // 不是本 manager 托管的账号：不接管、不落盘。
                return Ok(RefreshTokenAdoptionOutcome::NotManaged);
            };

            // A manager refresh may already have advanced the token generation
            // while auth.json still contains the older one. Never roll that
            // state back during the preflight/write double-build sequence.
            let refresh_changed = account.refresh_token != refresh_token;
            let id_token_changed = incoming_id_token
                .as_ref()
                .is_some_and(|token| account.id_token.as_deref() != Some(token.as_str()));
            let material_changed = refresh_changed || id_token_changed;
            let manager_was_undated = account.token_updated_at_ms <= 0;
            // Once the manager has a dated generation, any different token
            // material must carry a *strictly newer* live timestamp. Equality is
            // ambiguous at millisecond precision and therefore cannot authorize
            // replacing the manager generation either. Stores upgraded from
            // before generation timestamps existed keep a different live
            // generation ambiguous across retries; only matching material may
            // establish a timestamp. The server-rejected mode is the sole
            // exception because it has disproved the manager generation.
            let observed_order =
                last_refresh_ms.map(|observed| observed.cmp(&account.token_updated_at_ms));
            let should_adopt = material_changed
                && (matches!(mode, RefreshTokenAdoptionMode::RejectedManagerToken)
                    || (!manager_was_undated
                        && matches!(observed_order, Some(std::cmp::Ordering::Greater))));

            if !material_changed {
                outcome = RefreshTokenAdoptionOutcome::Synchronized {
                    state_changed: false,
                };
            } else if should_adopt {
                if refresh_changed {
                    account.refresh_token = refresh_token;
                    changed = true;
                    material_replaced = true;
                }
                if let Some(id_token) = incoming_id_token {
                    if account.id_token.as_deref() != Some(id_token.as_str()) {
                        account.id_token = Some(id_token);
                        changed = true;
                        material_replaced = true;
                    }
                }
                outcome = RefreshTokenAdoptionOutcome::Adopted;
            } else if !manager_was_undated
                && matches!(observed_order, Some(std::cmp::Ordering::Less))
            {
                outcome = RefreshTokenAdoptionOutcome::ProvablyOlder;
            } else {
                outcome = RefreshTokenAdoptionOutcome::Ambiguous;
            }

            if matches!(outcome, RefreshTokenAdoptionOutcome::Adopted)
                && matches!(mode, RefreshTokenAdoptionMode::RejectedManagerToken)
            {
                let adopted_at = last_refresh_ms
                    .filter(|observed| *observed > account.token_updated_at_ms)
                    .unwrap_or_else(|| {
                        chrono::Utc::now()
                            .timestamp_millis()
                            .max(account.token_updated_at_ms.saturating_add(1))
                    });
                if account.token_updated_at_ms != adopted_at {
                    account.token_updated_at_ms = adopted_at;
                    changed = true;
                }
            } else if matches!(outcome, RefreshTokenAdoptionOutcome::Adopted) {
                if let Some(observed) = last_refresh_ms {
                    if account.token_updated_at_ms != observed {
                        account.token_updated_at_ms = observed;
                        changed = true;
                    }
                }
            } else if matches!(outcome, RefreshTokenAdoptionOutcome::Synchronized { .. }) {
                if manager_was_undated {
                    // Matching material establishes one generation, so dating
                    // it cannot turn an unresolved R0/R1 conflict into a false
                    // "live is older" decision on the next retry.
                    account.token_updated_at_ms = last_refresh_ms
                        .filter(|observed| *observed > 0)
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
                    changed = true;
                } else if let Some(observed) = last_refresh_ms {
                    if observed > account.token_updated_at_ms {
                        account.token_updated_at_ms = observed;
                        changed = true;
                    }
                }
            }
            // 采纳了 CLI 轮换后的 refresh_token / id_token：同步更新 chatgpt_account_id
            // 并清理舊 access_token 缓存
            if material_replaced {
                let effective_chatgpt_id = account.synced_chatgpt_account_id();
                if account.chatgpt_account_id != effective_chatgpt_id {
                    account.chatgpt_account_id = effective_chatgpt_id;
                    changed = true;
                }
                self.access_tokens.write().await.remove(account_id);
            }

            if let RefreshTokenAdoptionOutcome::Synchronized { .. } = outcome {
                outcome = RefreshTokenAdoptionOutcome::Synchronized {
                    state_changed: changed,
                };
            }
        }
        if changed {
            self.save_to_disk().await?;
        }
        Ok(outcome)
    }

    /// 获取默认账号的有效 token
    pub async fn get_valid_token(&self) -> Result<String, CodexOAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.get_valid_token_for_account(&id).await,
            None => Err(CodexOAuthError::AccountNotFound(
                "无可用的 ChatGPT 账号".to_string(),
            )),
        }
    }

    /// 获取默认账号 ID（热路径使用，避免克隆整个账号 HashMap）
    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    /// 获取指定账号关联的 OpenAI Workspace / Team ID（chatgpt_account_id）
    pub async fn get_chatgpt_account_id_for_account(&self, account_id: &str) -> Option<String> {
        let accounts = self.accounts.read().await;
        accounts
            .get(account_id)
            .and_then(|a| a.effective_chatgpt_account_id())
    }

    // ==================== 多账号管理 ====================

    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_id.as_deref())
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), CodexOAuthError> {
        log::info!("[CodexOAuth] 移除账号: {account_id}");
        // Wait for all in-flight refresh/adopt operations before deleting. New
        // token work is blocked until the account, cache, lock and disk state
        // have been removed as one lifecycle transition.
        let _lifecycle = self.lifecycle_lock.write().await;

        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        // Explicit Auth Center removal means credentials for this managed
        // account must leave the machine. Content matching intentionally also
        // claims a native `codex login` of the same account; that is the same
        // account-scoped credential the user just chose to remove.
        crate::codex_config::clear_codex_live_auth_for_managed_account(account_id)
            .map_err(|error| CodexOAuthError::IoError(error.to_string()))?;

        {
            // 在 accounts 写锁内原子清除该账号的 token 缓存（accounts -> access_tokens
            // 顺序），确保不存在「账号已删但缓存仍在」的窗口。
            let mut accounts = self.accounts.write().await;
            accounts.remove(account_id);
            self.access_tokens.write().await.remove(account_id);
        }
        {
            let mut locks = self.refresh_locks.write().await;
            locks.remove(account_id);
        }

        {
            let accounts = self.accounts.read().await;
            let mut default = self.default_account_id.write().await;
            if default.as_deref() == Some(account_id) {
                *default = Self::fallback_default_account_id(&accounts);
            }
        }

        self.save_to_disk().await?;
        Ok(())
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let mut default = self.default_account_id.write().await;
            *default = Some(account_id.to_string());
        }

        self.save_to_disk().await?;
        Ok(())
    }

    pub async fn clear_auth(&self) -> Result<(), CodexOAuthError> {
        log::info!("[CodexOAuth] 清除所有认证");

        // Acquire lifecycle before storage. Refresh follows lifecycle(read) ->
        // account mutex -> storage, so this fixed order cannot deadlock and the
        // write guard guarantees no refresh can recreate live/disk state after
        // the clear has committed.
        let _lifecycle = self.lifecycle_lock.write().await;

        let account_ids = self
            .accounts
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for account_id in &account_ids {
            crate::codex_config::clear_codex_live_auth_for_managed_account(account_id)
                .map_err(|error| CodexOAuthError::IoError(error.to_string()))?;
        }

        // 与 save_to_disk 共用持久化锁：确保「清内存 + 删文件」相对于并发保存原子，
        // 不会被一个持有旧快照的 save 复活已清除的账号。
        let _persist = self.storage_lock.lock().await;

        {
            // 在 accounts 写锁内原子清除 accounts 与 token 缓存（accounts ->
            // access_tokens 顺序），杜绝「账号已清但缓存仍在」及并发 refresh 回填。
            let mut accounts = self.accounts.write().await;
            accounts.clear();
            self.access_tokens.write().await.clear();
        }
        {
            let mut default = self.default_account_id.write().await;
            *default = None;
        }
        {
            let mut locks = self.refresh_locks.write().await;
            locks.clear();
        }
        {
            let mut pending = self.pending_device_codes.write().await;
            self.login_epoch.fetch_add(1, Ordering::AcqRel);
            pending.clear();
        }

        if self.storage_path.exists() {
            std::fs::remove_file(&self.storage_path)?;
        }

        Ok(())
    }

    pub async fn is_authenticated(&self) -> bool {
        let accounts = self.accounts.read().await;
        !accounts.is_empty()
    }

    /// 获取认证状态摘要（与 Copilot 的格式保持一致，便于复用前端）
    pub async fn get_status(&self) -> CodexOAuthStatus {
        let accounts_map = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        let account_list = Self::sorted_accounts(&accounts_map, default_id.as_deref());
        let authenticated = !account_list.is_empty();
        let username = default_id
            .as_ref()
            .and_then(|id| accounts_map.get(id))
            .and_then(|a| a.email.clone())
            .or_else(|| account_list.first().map(|a| a.login.clone()));

        CodexOAuthStatus {
            accounts: account_list,
            default_account_id: default_id,
            authenticated,
            username,
        }
    }

    #[cfg(test)]
    pub(crate) async fn add_test_account_with_access_token(
        &self,
        account_id: &str,
        access_token: &str,
        id_token: Option<&str>,
    ) -> Result<(), CodexOAuthError> {
        let obtained_at_ms = chrono::Utc::now().timestamp_millis();
        self.add_account_internal(
            account_id.to_string(),
            "test-refresh-token".to_string(),
            Some(format!("{account_id}@example.test")),
            None,
            id_token.map(|token| token.to_string()),
            Some(CachedAccessToken {
                token: access_token.to_string(),
                expires_at_ms: obtained_at_ms + 3_600_000,
                obtained_at_ms,
            }),
            None,
        )
        .await?;

        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn test_refresh_token_for_account(&self, account_id: &str) -> Option<String> {
        self.accounts
            .read()
            .await
            .get(account_id)
            .map(|account| account.refresh_token.clone())
    }

    #[cfg(test)]
    pub(crate) async fn test_set_token_updated_at_ms(
        &self,
        account_id: &str,
        token_updated_at_ms: i64,
    ) {
        self.accounts
            .write()
            .await
            .get_mut(account_id)
            .expect("test account present")
            .token_updated_at_ms = token_updated_at_ms;
    }

    // ==================== 内部方法 ====================

    // 账号身份（account_id / email / workspace）与凭据必须一起原子写入，拆成
    // 中间结构只会把同一次登录的字段分散到两处。
    #[allow(clippy::too_many_arguments)]
    async fn add_account_internal(
        &self,
        account_id: String,
        refresh_token: String,
        email: Option<String>,
        chatgpt_account_id: Option<String>,
        id_token: Option<String>,
        initial_access_token: Option<CachedAccessToken>,
        pending_device_code: Option<&str>,
    ) -> Result<GitHubAccount, CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        if let Some(device_code) = pending_device_code {
            // `clear_auth` owns lifecycle(write) while clearing pending flows.
            // Re-check under lifecycle(read) at commit time so a poll that was
            // already on the network cannot recreate an account after clear.
            if self
                .pending_device_codes
                .write()
                .await
                .remove(device_code)
                .is_none()
            {
                return Err(CodexOAuthError::ExpiredToken);
            }
        }
        let refresh_lock = self.get_refresh_lock(&account_id).await;
        let _refresh_guard = refresh_lock.lock().await;
        let now = chrono::Utc::now().timestamp();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let data = CodexAccountData {
            account_id: account_id.clone(),
            email,
            chatgpt_account_id,
            refresh_token,
            authenticated_at: now,
            id_token,
            token_updated_at_ms: now_ms,
        };

        let account = GitHubAccount::from(&data);

        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(account_id.clone(), data);
            let mut access_tokens = self.access_tokens.write().await;
            if let Some(cached) = initial_access_token {
                access_tokens.insert(account_id.clone(), cached);
            } else {
                access_tokens.remove(&account_id);
            }
        }

        {
            let mut default = self.default_account_id.write().await;
            if default.is_none() {
                *default = Some(account_id);
            }
        }

        self.save_to_disk().await?;
        Ok(account)
    }

    fn fallback_default_account_id(accounts: &HashMap<String, CodexAccountData>) -> Option<String> {
        accounts
            .iter()
            .max_by(|(id_a, a), (id_b, b)| {
                a.authenticated_at
                    .cmp(&b.authenticated_at)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, _)| id.clone())
    }

    fn sorted_accounts(
        accounts: &HashMap<String, CodexAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<GitHubAccount> {
        let mut list: Vec<GitHubAccount> = accounts.values().map(GitHubAccount::from).collect();
        list.sort_by(|a, b| {
            let a_default = default_account_id == Some(a.id.as_str());
            let b_default = default_account_id == Some(b.id.as_str());
            b_default
                .cmp(&a_default)
                .then_with(|| b.authenticated_at.cmp(&a.authenticated_at))
                .then_with(|| a.login.cmp(&b.login))
        });
        list
    }

    async fn resolve_default_account_id(&self) -> Option<String> {
        let stored = self.default_account_id.read().await.clone();
        let accounts = self.accounts.read().await;

        if let Some(id) = stored {
            if accounts.contains_key(&id) {
                return Some(id);
            }
        }

        Self::fallback_default_account_id(&accounts)
    }

    async fn get_refresh_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        {
            let locks = self.refresh_locks.read().await;
            if let Some(lock) = locks.get(account_id) {
                return Arc::clone(lock);
            }
        }

        let mut locks = self.refresh_locks.write().await;
        Arc::clone(
            locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CodexOAuthError> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| CodexOAuthError::IoError("无效的存储路径".to_string()))?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| CodexOAuthError::IoError("无效的存储文件名".to_string()))?
            .to_string_lossy()
            .to_string();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = parent.join(format!("{file_name}.tmp.{ts}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            fs::rename(&tmp_path, &self.storage_path)?;
            fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(windows)]
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            if self.storage_path.exists() {
                let _ = fs::remove_file(&self.storage_path);
            }
            fs::rename(&tmp_path, &self.storage_path)?;
        }

        Ok(())
    }

    fn load_from_disk_sync(&self) -> Result<(), CodexOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.storage_path)?;
        let store: CodexOAuthStore = serde_json::from_str(&content)
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
            log::info!("[CodexOAuth] 从磁盘加载 {} 个账号", accounts.len());
        }
        if let Ok(mut default) = self.default_account_id.try_write() {
            *default = store.default_account_id;
            if default.is_none() {
                if let Ok(accounts) = self.accounts.try_read() {
                    *default = Self::fallback_default_account_id(&accounts);
                }
            }
        }

        Ok(())
    }

    async fn save_to_disk(&self) -> Result<(), CodexOAuthError> {
        // 串行化「快照 + 写盘」：在持久化锁内取快照，确保并发保存/清除不会用
        // 陈旧快照覆盖，避免已删账号被复活。
        let _persist = self.storage_lock.lock().await;
        let accounts = self.accounts.read().await.clone();
        let default = self.resolve_default_account_id().await;

        let store = CodexOAuthStore {
            version: 1,
            accounts,
            default_account_id: default,
        };

        let content = serde_json::to_string_pretty(&store)
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        self.write_store_atomic(&content)?;

        log::info!(
            "[CodexOAuth] 保存到磁盘成功（{} 个账号）",
            store.accounts.len()
        );

        Ok(())
    }
}

/// Codex OAuth 状态摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexOAuthStatus {
    pub accounts: Vec<GitHubAccount>,
    pub default_account_id: Option<String>,
    pub authenticated: bool,
    pub username: Option<String>,
}

// ==================== 工具函数 ====================

/// 解析 OpenAI Device Code 响应中的 interval 字段
///
/// 服务端可能返回字符串或数字，需要兼容
fn parse_interval(value: Option<&serde_json::Value>) -> u64 {
    let raw = match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(5),
        Some(serde_json::Value::String(s)) => s.parse::<u64>().unwrap_or(5),
        _ => 5,
    };
    raw.max(1) + POLLING_SAFETY_MARGIN_SECS
}

/// 从 expires_in（秒）计算过期时间戳（毫秒）
fn compute_expires_at_ms(expires_in: Option<i64>) -> i64 {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let secs = expires_in.unwrap_or(3600);
    now_ms + secs * 1000
}

fn extract_refresh_error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|error| match error {
            serde_json::Value::Object(object) => object.get("code").and_then(|code| code.as_str()),
            serde_json::Value::String(code) => Some(code.as_str()),
            _ => None,
        })
        .or_else(|| value.get("code").and_then(|code| code.as_str()))
        .map(|code| code.to_ascii_lowercase())
}

/// 解析 JWT 中的 claims
pub(crate) fn parse_jwt_claims(token: &str) -> Option<IdTokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// 组合账号主键：有个人身份且有工作区时用 `user_identity::workspace_id`，
/// 使同一工作区的多个成员、以及同一成员的多个工作区都不会互相覆盖。
fn compose_primary_account_id(
    user_identity: Option<String>,
    chatgpt_account_id: Option<String>,
) -> Option<String> {
    match (user_identity, chatgpt_account_id) {
        (Some(user), Some(workspace)) => Some(format!("{user}{ACCOUNT_ID_SEPARATOR}{workspace}")),
        (Some(user), None) => Some(user),
        (None, Some(workspace)) => Some(workspace),
        (None, None) => None,
    }
}

/// 从 token 响应中提取 (account_id, email, chatgpt_account_id)
///
/// `account_id` 为区分不同个人的唯一标识（Primary Key）：
/// 个人身份优先级 email > openai_auth.user_id > sub，再拼接工作区 ID；
/// 完全没有个人 claim 时退化为工作区 ID。
///
/// `chatgpt_account_id` 为 OpenAI Workspace/Team ID
fn extract_identity_from_tokens(
    tokens: &OAuthTokenResponse,
) -> (Option<String>, Option<String>, Option<String>) {
    let id_claims = tokens.id_token.as_deref().and_then(parse_jwt_claims);
    let access_claims = parse_jwt_claims(&tokens.access_token);

    let (identity, chatgpt_account_id) =
        resolve_identity_and_workspace(id_claims.as_ref(), access_claims.as_ref());

    resolve_account_identity_from(&identity, chatgpt_account_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval_number() {
        let v = serde_json::Value::Number(serde_json::Number::from(5));
        assert_eq!(parse_interval(Some(&v)), 5 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_string() {
        let v = serde_json::Value::String("10".to_string());
        assert_eq!(parse_interval(Some(&v)), 10 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_default() {
        assert_eq!(parse_interval(None), 5 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_min() {
        let v = serde_json::Value::Number(serde_json::Number::from(0));
        // 0 应被提升到 1
        assert_eq!(parse_interval(Some(&v)), 1 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_compute_expires_at_ms() {
        let result = compute_expires_at_ms(Some(3600));
        let now = chrono::Utc::now().timestamp_millis();
        // 应在未来约 3600 秒处（允许少量误差）
        assert!(result > now + 3500 * 1000);
        assert!(result < now + 3700 * 1000);
    }

    #[test]
    fn test_compute_expires_at_ms_default() {
        let result = compute_expires_at_ms(None);
        let now = chrono::Utc::now().timestamp_millis();
        assert!(result > now);
    }

    #[test]
    fn test_cached_token_expiring_soon() {
        let now = chrono::Utc::now().timestamp_millis();
        // 30 秒后过期 - 在缓冲期内
        let expiring = CachedAccessToken {
            token: "t".to_string(),
            expires_at_ms: now + 30_000,
            obtained_at_ms: now,
        };
        assert!(expiring.is_expiring_soon());

        // 1 小时后过期 - 不在缓冲期内
        let valid = CachedAccessToken {
            token: "t".to_string(),
            expires_at_ms: now + 3_600_000,
            obtained_at_ms: now,
        };
        assert!(!valid.is_expiring_soon());
    }

    #[test]
    fn test_parse_jwt_claims_invalid() {
        assert!(parse_jwt_claims("not-a-jwt").is_none());
        assert!(parse_jwt_claims("only.two").is_none());
    }

    #[test]
    fn test_parse_jwt_claims_valid() {
        // Header: {"alg":"none"}
        // Payload: {"chatgpt_account_id":"acc-123","email":"test@example.com"}
        // Signature: empty
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD
            .encode(b"{\"chatgpt_account_id\":\"acc-123\",\"email\":\"test@example.com\"}");
        let jwt = format!("{header}.{payload}.");
        let claims = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(claims.chatgpt_account_id.as_deref(), Some("acc-123"));
        assert_eq!(claims.email.as_deref(), Some("test@example.com"));
    }

    #[test]
    fn test_parse_jwt_claims_organizations_fallback() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"organizations\":[{\"id\":\"org-456\"}]}");
        let jwt = format!("{header}.{payload}.");
        let claims = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(
            claims
                .organizations
                .first()
                .and_then(|o| o.id.clone())
                .as_deref(),
            Some("org-456")
        );
    }

    #[tokio::test]
    async fn test_manager_initial_state() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        assert!(!manager.is_authenticated().await);
        assert!(manager.list_accounts().await.is_empty());
    }

    #[tokio::test]
    async fn test_manager_save_and_load() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();

        // Manually inject an account through internal methods
        {
            let manager = CodexOAuthManager::new(path.clone());
            manager
                .add_account_internal(
                    "acc-123".to_string(),
                    "rt-secret".to_string(),
                    Some("user@example.com".to_string()),
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        // New manager should load from disk
        let manager2 = CodexOAuthManager::new(path);
        let accounts = manager2.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-123");
    }

    #[tokio::test]
    async fn test_remove_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        manager
            .add_account_internal(
                "acc-123".to_string(),
                "rt".to_string(),
                Some("a@example.com".to_string()),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        manager
            .add_account_internal(
                "acc-456".to_string(),
                "rt2".to_string(),
                Some("b@example.com".to_string()),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        manager.remove_account("acc-123").await.unwrap();
        let accounts = manager.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-456");
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_syncs_rotated_value() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-1"))
            .await
            .unwrap();

        // 采纳带有更新 last_refresh 的 Codex CLI 轮换 refresh_token / id_token。
        let manager_updated_at = manager
            .accounts
            .read()
            .await
            .get("acc-1")
            .expect("account present")
            .token_updated_at_ms;
        let changed = manager
            .adopt_account_refresh_token(
                "acc-1",
                "rotated-rt".to_string(),
                Some("id-2".to_string()),
                Some(manager_updated_at.saturating_add(1)),
            )
            .await
            .unwrap();
        assert!(changed, "rotated refresh_token should be adopted");

        // 存储里的 refresh_token / id_token 已更新为盘上（CLI 轮换后）的值。
        {
            let accounts = manager.accounts.read().await;
            let account = accounts.get("acc-1").expect("account present");
            assert_eq!(account.refresh_token, "rotated-rt");
            assert_eq!(account.id_token.as_deref(), Some("id-2"));
        }
        // 采纳后清掉了该账号的缓存 access_token，以便下次按新 refresh_token 重取
        // （因此这里不再用 get_valid_token_bundle_for_account 断言——它会触发联网刷新）。
        assert!(
            !manager.access_tokens.read().await.contains_key("acc-1"),
            "adopt should invalidate the cached access token"
        );

        // 未知账号不接管。
        assert!(!manager
            .adopt_account_refresh_token("acc-unknown", "x".to_string(), None, None)
            .await
            .unwrap());

        // 相同值不算变化。
        assert!(!manager
            .adopt_account_refresh_token("acc-1", "rotated-rt".to_string(), None, None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_rejects_older_live_generation() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-1"))
            .await
            .unwrap();

        let manager_updated_at = manager
            .accounts
            .read()
            .await
            .get("acc-1")
            .expect("account present")
            .token_updated_at_ms;
        let changed = manager
            .adopt_account_refresh_token(
                "acc-1",
                "stale-live-refresh".to_string(),
                None,
                Some(manager_updated_at.saturating_sub(1)),
            )
            .await
            .unwrap();

        assert!(!changed, "older live state must not roll the manager back");
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("acc-1")
                .expect("account present")
                .refresh_token,
            "test-refresh-token"
        );
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_rejects_undated_live_generation() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-1"))
            .await
            .unwrap();

        let changed = manager
            .adopt_account_refresh_token("acc-1", "ambiguous-live-refresh".to_string(), None, None)
            .await
            .unwrap();

        assert!(
            !changed,
            "an undated live token must not roll back a timestamped manager generation"
        );
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("acc-1")
                .expect("account present")
                .refresh_token,
            "test-refresh-token"
        );
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_rejects_stale_id_token_with_same_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-new"))
            .await
            .unwrap();
        let manager_updated_at = manager
            .accounts
            .read()
            .await
            .get("acc-1")
            .expect("account present")
            .token_updated_at_ms;

        let changed = manager
            .adopt_account_refresh_token(
                "acc-1",
                "test-refresh-token".to_string(),
                Some("id-stale".to_string()),
                Some(manager_updated_at.saturating_sub(1)),
            )
            .await
            .unwrap();

        assert!(!changed);
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("acc-1")
                .expect("account present")
                .id_token
                .as_deref(),
            Some("id-new")
        );
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_rejects_equal_timestamp_generation() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-1"))
            .await
            .unwrap();
        let manager_updated_at = manager
            .accounts
            .read()
            .await
            .get("acc-1")
            .expect("account present")
            .token_updated_at_ms;

        let changed = manager
            .adopt_account_refresh_token(
                "acc-1",
                "same-millisecond-refresh".to_string(),
                None,
                Some(manager_updated_at),
            )
            .await
            .unwrap();

        assert!(!changed);
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("acc-1")
                .expect("account present")
                .refresh_token,
            "test-refresh-token"
        );
    }

    #[tokio::test]
    async fn adopt_account_refresh_token_keeps_legacy_conflict_ambiguous_across_retries() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-manager"))
            .await
            .unwrap();
        manager
            .accounts
            .write()
            .await
            .get_mut("acc-1")
            .expect("account present")
            .token_updated_at_ms = 0;

        for attempt in 1..=2 {
            let changed = manager
                .adopt_account_refresh_token(
                    "acc-1",
                    "ambiguous-live-refresh".to_string(),
                    Some("id-live".to_string()),
                    Some(1_700_000_000_000),
                )
                .await
                .unwrap();
            assert!(
                !changed,
                "legacy conflict must remain unresolved on attempt {attempt}"
            );
        }

        let accounts = manager.accounts.read().await;
        let account = accounts.get("acc-1").expect("account present");
        assert_eq!(account.refresh_token, "test-refresh-token");
        assert_eq!(account.id_token.as_deref(), Some("id-manager"));
        assert_eq!(
            account.token_updated_at_ms, 0,
            "dating old manager material would make the next retry falsely classify the live token as older"
        );
        drop(accounts);
        assert!(
            manager.access_tokens.read().await.contains_key("acc-1"),
            "an unresolved conflict must not invalidate a valid access token"
        );
    }

    #[tokio::test]
    async fn rejected_manager_token_adopts_different_disk_token_without_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_test_account_with_access_token("acc-1", "access-cached", Some("id-manager"))
            .await
            .unwrap();

        let outcome = manager
            .adopt_account_refresh_token_under_lock(
                "acc-1",
                "recovered-live-refresh".to_string(),
                Some("id-live".to_string()),
                None,
                RefreshTokenAdoptionMode::RejectedManagerToken,
            )
            .await
            .unwrap();

        assert_eq!(outcome, RefreshTokenAdoptionOutcome::Adopted);
        let accounts = manager.accounts.read().await;
        let account = accounts.get("acc-1").expect("account present");
        assert_eq!(account.refresh_token, "recovered-live-refresh");
        assert_eq!(account.id_token.as_deref(), Some("id-live"));
        assert!(account.token_updated_at_ms > 0);
        drop(accounts);
        assert!(
            !manager.access_tokens.read().await.contains_key("acc-1"),
            "forced recovery must invalidate the cached access token"
        );
    }

    #[tokio::test]
    async fn device_commit_rejects_flow_cleared_during_network_poll() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager.pending_device_codes.write().await.insert(
            "device-auth-id".to_string(),
            PendingDeviceCode {
                user_code: "ABCD-EFGH".to_string(),
                expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
            },
        );

        manager.clear_auth().await.unwrap();
        let result = manager
            .add_account_internal(
                "acc-after-clear".to_string(),
                "refresh-after-clear".to_string(),
                None,
                None,
                None,
                None,
                Some("device-auth-id"),
            )
            .await;

        assert!(matches!(result, Err(CodexOAuthError::ExpiredToken)));
        assert!(manager.list_accounts().await.is_empty());
        assert!(!manager.storage_path.exists());
    }

    #[tokio::test]
    async fn device_start_rejects_flow_cleared_during_network_request() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let login_epoch = manager.login_epoch.load(Ordering::Acquire);

        manager.clear_auth().await.unwrap();
        let result = manager
            .register_pending_device_code(
                "stale-device-auth-id".to_string(),
                "ABCD-EFGH".to_string(),
                chrono::Utc::now().timestamp_millis() + 60_000,
                login_epoch,
            )
            .await;

        assert!(matches!(result, Err(CodexOAuthError::ExpiredToken)));
        assert!(manager.pending_device_codes.read().await.is_empty());
    }

    #[tokio::test]
    async fn same_team_multiple_accounts_coexist_without_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        // 两个属于同一 Team 工作区（相同 chatgpt_account_id）但不同邮箱的账号
        let team_workspace_id = "e7a7bd67-d460-48dd-b066-0a550abd3778";
        let acct_1 = format!("user1@example.com::{team_workspace_id}");
        let acct_2 = format!("user2@example.com::{team_workspace_id}");

        manager
            .add_account_internal(
                acct_1.clone(),
                "rt-user1".to_string(),
                Some("user1@example.com".to_string()),
                Some(team_workspace_id.to_string()),
                Some("id-token-1".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        manager
            .add_account_internal(
                acct_2.clone(),
                "rt-user2".to_string(),
                Some("user2@example.com".to_string()),
                Some(team_workspace_id.to_string()),
                Some("id-token-2".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = manager.list_accounts().await;
        assert_eq!(accounts.len(), 2, "两个同 Team 账号必须独立并存");
        assert!(accounts
            .iter()
            .any(|a| a.id == acct_1 && a.login == "user1@example.com (e7a7bd67)"));
        assert!(accounts
            .iter()
            .any(|a| a.id == acct_2 && a.login == "user2@example.com (e7a7bd67)"));

        assert_eq!(
            manager
                .get_chatgpt_account_id_for_account(&acct_1)
                .await
                .as_deref(),
            Some(team_workspace_id)
        );
        assert_eq!(
            manager
                .get_chatgpt_account_id_for_account(&acct_2)
                .await
                .as_deref(),
            Some(team_workspace_id)
        );
    }

    #[tokio::test]
    async fn same_user_multiple_teams_coexist_without_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        // 同一个邮箱加入两个不同的 Team 以及一个 Personal 空间
        let team_1 = "e7a7bd67-d460-48dd-b066-0a550abd3778";
        let team_2 = "9b32f10a-81c2-48dd-b066-0a550abd9999";
        let acct_team_1 = format!("user@example.com::{team_1}");
        let acct_team_2 = format!("user@example.com::{team_2}");
        let acct_personal = "user@example.com".to_string();

        manager
            .add_account_internal(
                acct_team_1.clone(),
                "rt-team1".to_string(),
                Some("user@example.com".to_string()),
                Some(team_1.to_string()),
                Some("id-team-1".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        manager
            .add_account_internal(
                acct_team_2.clone(),
                "rt-team2".to_string(),
                Some("user@example.com".to_string()),
                Some(team_2.to_string()),
                Some("id-team-2".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        manager
            .add_account_internal(
                acct_personal.clone(),
                "rt-personal".to_string(),
                Some("user@example.com".to_string()),
                None,
                Some("id-personal".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = manager.list_accounts().await;
        assert_eq!(
            accounts.len(),
            3,
            "同一用户的多个工作区账号必须全部独立并存"
        );
        assert!(accounts
            .iter()
            .any(|a| a.id == acct_team_1 && a.login == "user@example.com (e7a7bd67)"));
        assert!(accounts
            .iter()
            .any(|a| a.id == acct_team_2 && a.login == "user@example.com (9b32f10a)"));
        assert!(accounts
            .iter()
            .any(|a| a.id == acct_personal && a.login == "user@example.com"));

        assert_eq!(
            manager
                .get_chatgpt_account_id_for_account(&acct_team_1)
                .await
                .as_deref(),
            Some(team_1)
        );
        assert_eq!(
            manager
                .get_chatgpt_account_id_for_account(&acct_team_2)
                .await
                .as_deref(),
            Some(team_2)
        );
        assert_eq!(
            manager
                .get_chatgpt_account_id_for_account(&acct_personal)
                .await
                .as_deref(),
            None
        );
    }

    #[test]
    fn test_extract_identity_from_tokens_priority() {
        // 1. 同时有 user_id / sub / email 时用 user_id::workspace_id：
        //    user_id 与 sub 是不可变的不透明标识，email 可以被用户改掉
        let payload1 = serde_json::json!({
            "email": "user@example.com",
            "sub": "auth0|12345",
            "chatgpt_account_id": "team-workspace-uuid",
            "https://api.openai.com/auth": {
                "user_id": "user-abcdef",
                "chatgpt_account_id": "team-workspace-uuid"
            }
        });
        let tokens = OAuthTokenResponse {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(payload1)),
            expires_in: Some(3600),
        };
        let (account_id, email, chatgpt_account_id) = extract_identity_from_tokens(&tokens);
        assert_eq!(
            account_id.as_deref(),
            Some("user-abcdef::team-workspace-uuid")
        );
        // email 仍然照常解析出来，只是不参与主键
        assert_eq!(email.as_deref(), Some("user@example.com"));
        assert_eq!(chatgpt_account_id.as_deref(), Some("team-workspace-uuid"));

        // 2. 缺失 email 时降级使用 user_id::workspace_id
        let payload2 = serde_json::json!({
            "sub": "auth0|12345",
            "https://api.openai.com/auth": {
                "user_id": "user-abcdef",
                "chatgpt_account_id": "team-workspace-uuid"
            }
        });
        let tokens2 = OAuthTokenResponse {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(payload2)),
            expires_in: Some(3600),
        };
        let (account_id2, email2, chatgpt_account_id2) = extract_identity_from_tokens(&tokens2);
        assert_eq!(
            account_id2.as_deref(),
            Some("user-abcdef::team-workspace-uuid")
        );
        assert_eq!(email2, None);
        assert_eq!(chatgpt_account_id2.as_deref(), Some("team-workspace-uuid"));
    }

    fn unsigned_jwt(payload: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{header}.{encoded}.")
    }

    #[test]
    fn identity_falls_back_to_access_token_personal_claims_before_workspace() {
        // id_token 只带工作区 claim，个人身份只在 access_token 里
        let id_token = unsigned_jwt(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "team-workspace-uuid" }
        }));
        let access_token = unsigned_jwt(serde_json::json!({ "email": "user@example.com" }));
        let tokens = OAuthTokenResponse {
            access_token,
            refresh_token: Some("refresh".to_string()),
            id_token: Some(id_token),
            expires_in: Some(3600),
        };

        let (account_id, email, chatgpt_account_id) = extract_identity_from_tokens(&tokens);
        assert_eq!(
            account_id.as_deref(),
            Some("user@example.com::team-workspace-uuid")
        );
        assert_eq!(email.as_deref(), Some("user@example.com"));
        assert_eq!(chatgpt_account_id.as_deref(), Some("team-workspace-uuid"));
    }

    #[test]
    fn synced_workspace_id_replaces_a_stale_stored_value() {
        let account = CodexAccountData {
            account_id: "user@example.com::old-workspace".to_string(),
            email: Some("user@example.com".to_string()),
            chatgpt_account_id: Some("old-workspace".to_string()),
            refresh_token: "refresh".to_string(),
            authenticated_at: 0,
            id_token: Some(unsigned_jwt(serde_json::json!({
                "email": "user@example.com",
                "chatgpt_account_id": "new-workspace"
            }))),
            token_updated_at_ms: 0,
        };

        // 存量字段非空时 effective_* 永远返回旧值，同步必须读新 token 的 claims
        assert_eq!(
            account.effective_chatgpt_account_id().as_deref(),
            Some("old-workspace")
        );
        assert_eq!(
            account.synced_chatgpt_account_id().as_deref(),
            Some("new-workspace")
        );
    }

    #[test]
    fn synced_workspace_id_keeps_stored_value_when_new_token_has_no_claim() {
        let account = CodexAccountData {
            account_id: "user@example.com::workspace".to_string(),
            email: Some("user@example.com".to_string()),
            chatgpt_account_id: Some("workspace".to_string()),
            refresh_token: "refresh".to_string(),
            authenticated_at: 0,
            id_token: Some(unsigned_jwt(
                serde_json::json!({ "email": "user@example.com" }),
            )),
            token_updated_at_ms: 0,
        };

        assert_eq!(
            account.synced_chatgpt_account_id().as_deref(),
            Some("workspace")
        );
    }

    /// v3.20.0 及更早版本存下来的形状：key 是工作区 ID，个人身份只体现在
    /// email / id_token claims 里。
    async fn seed_legacy_workspace_account(
        manager: &CodexOAuthManager,
        workspace: &str,
        email: Option<&str>,
    ) {
        let id_token = email.map(|email| {
            unsigned_jwt(serde_json::json!({
                "email": email,
                "chatgpt_account_id": workspace,
            }))
        });
        manager
            .add_account_internal(
                workspace.to_string(),
                "rt-legacy".to_string(),
                email.map(str::to_string),
                Some(workspace.to_string()),
                id_token,
                None,
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn relogin_writes_a_composite_key_and_leaves_the_legacy_entry() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let workspace = "team-workspace-uuid";
        seed_legacy_workspace_account(&manager, workspace, Some("alice@example.com")).await;

        // 登录一律写入复合键，不去改写升级前存下的裸工作区 key：那个 key 无法证明
        // 属于谁（同工作区另一个成员的重新登录长得一模一样），沿用它就可能静默覆盖
        // 别人的凭据。旧条目原样保留，用户在 UI 重新选一次账号即完成绑定。
        manager
            .add_account_internal(
                format!("alice@example.com::{workspace}"),
                "rt-alice-new".to_string(),
                Some("alice@example.com".to_string()),
                Some(workspace.to_string()),
                Some(unsigned_jwt(serde_json::json!({
                    "email": "alice@example.com",
                    "chatgpt_account_id": workspace,
                }))),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = manager.accounts.read().await;
        assert_eq!(accounts.len(), 2);
        assert_eq!(
            accounts.get(workspace).map(|a| a.refresh_token.as_str()),
            Some("rt-legacy")
        );
        assert_eq!(
            accounts
                .get(&format!("alice@example.com::{workspace}"))
                .map(|a| a.refresh_token.as_str()),
            Some("rt-alice-new")
        );
    }

    #[tokio::test]
    async fn another_member_of_the_same_workspace_gets_its_own_key() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let workspace = "team-workspace-uuid";
        seed_legacy_workspace_account(&manager, workspace, Some("alice@example.com")).await;

        manager
            .add_account_internal(
                format!("bob@example.com::{workspace}"),
                "rt-bob".to_string(),
                Some("bob@example.com".to_string()),
                Some(workspace.to_string()),
                Some(unsigned_jwt(serde_json::json!({
                    "email": "bob@example.com",
                    "chatgpt_account_id": workspace,
                }))),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = manager.accounts.read().await;
        assert_eq!(accounts.len(), 2);
        assert_eq!(
            accounts.get(workspace).map(|a| a.refresh_token.as_str()),
            Some("rt-legacy")
        );
        assert!(accounts.contains_key(&format!("bob@example.com::{workspace}")));
    }

    #[tokio::test]
    async fn the_same_user_in_another_workspace_gets_its_own_key() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        seed_legacy_workspace_account(&manager, "workspace-a", Some("alice@example.com")).await;

        manager
            .add_account_internal(
                "alice@example.com::workspace-b".to_string(),
                "rt-alice-b".to_string(),
                Some("alice@example.com".to_string()),
                Some("workspace-b".to_string()),
                Some(unsigned_jwt(serde_json::json!({
                    "email": "alice@example.com",
                    "chatgpt_account_id": "workspace-b",
                }))),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = manager.accounts.read().await;
        assert_eq!(accounts.len(), 2);
        assert!(accounts.contains_key("workspace-a"));
        assert!(accounts.contains_key("alice@example.com::workspace-b"));
    }

    #[test]
    fn email_case_does_not_split_one_account_into_two_keys() {
        // 同一个账号在不同次登录里可能回传不同大小写的 email，主键必须归一化，
        // 否则会长出两条互不认识的记录。user_id / sub 是不透明标识，不能动。
        let make = |email: &str| {
            extract_identity_from_tokens(&OAuthTokenResponse {
                access_token: unsigned_jwt(serde_json::json!({
                    "email": email,
                    "chatgpt_account_id": "workspace",
                })),
                refresh_token: Some("refresh".to_string()),
                id_token: None,
                expires_in: Some(3600),
            })
            .0
        };

        assert_eq!(
            make("Alice@Example.com").as_deref(),
            make("alice@example.com").as_deref()
        );
        assert_eq!(
            make("Alice@Example.com").as_deref(),
            Some("alice@example.com::workspace")
        );
    }

    #[test]
    fn the_primary_key_ignores_access_token_only_claims() {
        // id_token 带 sub、email 只在 access_token 里：主键必须取 id_token 的 sub。
        // access_token 会被 Codex CLI 自刷新轮换，让它参与主键，就会在换代后算出
        // 另一个 key，auth.json 从此认不回账号、CLI 轮换的 refresh_token 被孤儿化。
        let id_token = unsigned_jwt(serde_json::json!({
            "sub": "auth0|12345",
            "chatgpt_account_id": "team-workspace-uuid"
        }));
        let access_token = unsigned_jwt(serde_json::json!({ "email": "alice@example.com" }));
        let tokens = OAuthTokenResponse {
            access_token,
            refresh_token: Some("refresh".to_string()),
            id_token: Some(id_token),
            expires_in: Some(3600),
        };

        let (account_id, email, chatgpt_account_id) = extract_identity_from_tokens(&tokens);
        assert_eq!(
            account_id.as_deref(),
            Some("auth0|12345::team-workspace-uuid")
        );
        // email 只用于展示，仍然跨两个 token 收集
        assert_eq!(email.as_deref(), Some("alice@example.com"));
        assert_eq!(chatgpt_account_id.as_deref(), Some("team-workspace-uuid"));
    }

    #[test]
    fn refresh_error_code_accepts_openai_error_shapes() {
        assert_eq!(
            extract_refresh_error_code(r#"{"error":{"code":"refresh_token_reused"}}"#).as_deref(),
            Some("refresh_token_reused")
        );
        assert_eq!(
            extract_refresh_error_code(r#"{"error":"refresh_token_expired"}"#).as_deref(),
            Some("refresh_token_expired")
        );
        assert_eq!(
            extract_refresh_error_code(r#"{"code":"REFRESH_TOKEN_INVALIDATED"}"#).as_deref(),
            Some("refresh_token_invalidated")
        );
        assert_eq!(extract_refresh_error_code("not json"), None);
    }
}
