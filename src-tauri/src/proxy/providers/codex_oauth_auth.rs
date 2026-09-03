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
//! - 本地账号 ID 用于绑定和缓存；chatgpt_account_id 仅表示上游 workspace

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
use uuid::Uuid;

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

/// 解析后的 JWT claims（仅关心 chatgpt_account_id 等字段）
#[derive(Debug, Clone, Default, Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaim>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpenAiAuthClaim {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
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
    /// 仅重新认证时设置；登录完成后原位更新该本地账号，保留 provider 绑定。
    target_account_id: Option<String>,
    /// 同一目标账号只允许最新启动的重新认证流程提交。
    target_generation: Option<u64>,
}

#[derive(Default)]
struct AccountLoginContext<'a> {
    target_account_id: Option<&'a str>,
    pending_device_code: Option<&'a str>,
    target_generation: Option<u64>,
}

/// 持久化的账号数据
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAccountData {
    /// 本地稳定账号 ID（同时作为 HashMap 的 key）
    pub account_id: String,
    /// 上游 ChatGPT workspace ID（用于 chatgpt-account-id 请求头）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_account_id: Option<String>,
    /// 账号邮箱（如果可获取）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
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

/// 公开的账号信息（返回给前端，复用 GitHubAccount 结构）
impl From<&CodexAccountData> for GitHubAccount {
    fn from(data: &CodexAccountData) -> Self {
        GitHubAccount {
            id: data.account_id.clone(),
            // 用 email 作为显示名（若无则用上游 workspace ID）
            login: data.email.clone().unwrap_or_else(|| {
                format!(
                    "ChatGPT ({})",
                    data.chatgpt_account_id
                        .as_deref()
                        .unwrap_or(&data.account_id)
                )
            }),
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: "github.com".to_string(),
            // 旧账号可能缺少 id_token 或独立的上游 workspace 字段；两者都需要
            // 重新登录后才能安全参与本地 ID → workspace 的托管绑定。
            reauth_required: data
                .id_token
                .as_deref()
                .and_then(crate::codex_config::extract_codex_id_token_user_identity)
                .is_none()
                || data.chatgpt_account_id.is_none(),
        }
    }
}

impl CodexAccountData {
    fn apply_refreshed_tokens(&mut self, tokens: &OAuthTokenResponse) -> bool {
        let refreshed_account_id = extract_account_metadata_from_tokens(tokens).0;
        let mut changed = false;
        if let Some(account_id) = refreshed_account_id {
            // A missing workspace marks a quarantined pre-v2 record. Ordinary
            // refresh cannot prove which same-workspace user an old binding
            // originally represented; only explicit targeted reauth may fill it.
            if self.chatgpt_account_id.is_some()
                && self.chatgpt_account_id.as_deref() != Some(&account_id)
            {
                self.chatgpt_account_id = Some(account_id);
                changed = true;
            }
        }
        if let Some(refresh_token) = tokens
            .refresh_token
            .as_ref()
            .filter(|token| !token.trim().is_empty())
        {
            if self.refresh_token != *refresh_token {
                self.refresh_token = refresh_token.clone();
                changed = true;
            }
        }

        changed
    }
}

/// 持久化存储结构（v2）
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
    pub chatgpt_account_id: String,
    pub access_token: String,
    pub id_token: Option<String>,
    pub refresh_token: String,
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
    /// 每个目标账号最新一次定向重新认证的 generation。
    target_login_generations: Arc<RwLock<HashMap<String, u64>>>,
    next_target_login_generation: AtomicU64,
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
            target_login_generations: Arc::new(RwLock::new(HashMap::new())),
            next_target_login_generation: AtomicU64::new(0),
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
    pub async fn start_device_flow(
        &self,
        target_account_id: Option<&str>,
    ) -> Result<GitHubDeviceCodeResponse, CodexOAuthError> {
        log::info!("[CodexOAuth] 启动 Device Code 流程");
        let login_epoch = self.login_epoch.load(Ordering::Acquire);
        let target_account_id = target_account_id
            .map(str::trim)
            .filter(|account_id| !account_id.is_empty())
            .map(str::to_string);
        let target_generation = if let Some(account_id) = target_account_id.as_deref() {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
            drop(accounts);
            let generation = self
                .next_target_login_generation
                .fetch_add(1, Ordering::AcqRel)
                .wrapping_add(1);
            let mut generations = self.target_login_generations.write().await;
            generations
                .entry(account_id.to_string())
                .and_modify(|current| *current = (*current).max(generation))
                .or_insert(generation);
            Some(generation)
        } else {
            None
        };

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
            target_account_id,
            target_generation,
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
        target_account_id: Option<String>,
        target_generation: Option<u64>,
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
                target_account_id,
                target_generation,
            },
        );
        Ok(())
    }

    pub async fn cancel_device_flow(&self, device_code: &str) -> bool {
        self.pending_device_codes
            .write()
            .await
            .remove(device_code)
            .is_some()
    }

    /// 轮询 Device Code 状态
    ///
    /// 接收 device_code（即 device_auth_id），返回 Some(account) 表示授权成功
    pub async fn poll_for_token<BeforeCommit, CommitFuture, CommitGuard>(
        &self,
        device_code: &str,
        before_commit: BeforeCommit,
    ) -> Result<Option<GitHubAccount>, CodexOAuthError>
    where
        BeforeCommit: FnOnce() -> CommitFuture,
        CommitFuture: std::future::Future<Output = CommitGuard>,
    {
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

        let user_code = entry.user_code.clone();

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

        let (chatgpt_account_id, email) = extract_account_metadata_from_tokens(&tokens);
        let chatgpt_account_id = chatgpt_account_id.ok_or_else(|| {
            CodexOAuthError::ParseError("无法从 token 中提取 chatgpt_account_id".to_string())
        })?;

        let id_token = tokens
            .id_token
            .clone()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                CodexOAuthError::TokenFetchFailed(
                    "登录响应缺少 id_token，账号未保存，请重新登录".to_string(),
                )
            })?;
        if crate::codex_config::extract_codex_id_token_subject(&id_token).is_none() {
            return Err(CodexOAuthError::TokenFetchFailed(
                "登录响应无法确认稳定用户身份，账号未保存，请重新登录".to_string(),
            ));
        }

        let obtained_at_ms = chrono::Utc::now().timestamp_millis();
        // Provider switching and managed live-auth writes use the same guard.
        // Acquire it only after the network exchange succeeds so ordinary
        // authorization-pending polls never block provider operations.
        let _commit_guard = before_commit().await;
        // 登录提交与该账号的 refresh/adopt 共用一把 generation 锁；账号和
        // access cache 一次写入，旧刷新响应因此不能覆盖新登录链。
        let account = self
            .add_account_internal(
                chatgpt_account_id,
                refresh_token,
                email,
                Some(id_token),
                Some(CachedAccessToken {
                    token: tokens.access_token.clone(),
                    expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                    obtained_at_ms,
                }),
                AccountLoginContext {
                    target_account_id: entry.target_account_id.as_deref(),
                    pending_device_code: Some(device_code),
                    target_generation: entry.target_generation,
                },
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
        self.ensure_account_ready_for_use(account_id).await?;
        Ok(self.resolve_valid_cached_token(account_id).await?.token)
    }

    async fn ensure_account_ready_for_use(&self, account_id: &str) -> Result<(), CodexOAuthError> {
        let accounts = self.accounts.read().await;
        let account = accounts
            .get(account_id)
            .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
        if account
            .id_token
            .as_deref()
            .and_then(crate::codex_config::extract_codex_id_token_user_identity)
            .is_none()
            || account.chatgpt_account_id.is_none()
        {
            return Err(CodexOAuthError::ParseError(format!(
                "账号 {account_id} 缺少 id_token 中可证明的用户身份或 workspace，请重新认证"
            )));
        }
        Ok(())
    }

    async fn read_managed_live_auth_refresh_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<(String, Option<String>, Option<i64>)>, CodexOAuthError> {
        let (managed_id_token, managed_workspace) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
            let workspace = account.chatgpt_account_id.clone().ok_or_else(|| {
                CodexOAuthError::ParseError(format!(
                    "账号 {account_id} 缺少 workspace 身份，请重新认证"
                ))
            })?;
            (account.id_token.clone(), workspace)
        };
        let Some(live_refresh) =
            crate::codex_config::read_codex_live_auth_refresh_for_managed_account(
                account_id,
                managed_id_token.as_deref(),
            )
            .map_err(|error| CodexOAuthError::TokenFetchFailed(error.to_string()))?
        else {
            return Ok(None);
        };

        if managed_workspace != live_refresh.chatgpt_account_id {
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "Codex OAuth 账号 {account_id} 的 workspace 与磁盘凭据不一致，本次操作已取消"
            )));
        }
        Ok(Some((
            live_refresh.refresh_token,
            live_refresh.id_token,
            live_refresh.last_refresh_ms,
        )))
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
        if let Some((live_refresh, live_id_token, live_last_refresh_ms)) = self
            .read_managed_live_auth_refresh_for_account(account_id)
            .await?
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
                let Some((live_refresh, live_id_token, live_last_refresh_ms)) = self
                    .read_managed_live_auth_refresh_for_account(account_id)
                    .await?
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
        let (stored_refresh_token, stored_id_token, chatgpt_account_id) = {
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
            if account.apply_refreshed_tokens(&new_tokens) {
                needs_save = true;
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
            if account.token_updated_at_ms != obtained_at_ms {
                account.token_updated_at_ms = obtained_at_ms;
                needs_save = true;
            }
            (
                account.refresh_token.clone(),
                account.id_token.clone(),
                account.chatgpt_account_id.clone(),
            )
        };
        if needs_save {
            self.save_to_disk().await?;
        }
        let chatgpt_account_id = chatgpt_account_id.ok_or_else(|| {
            CodexOAuthError::ParseError(
                "无法从刷新后的 token 中提取 chatgpt_account_id".to_string(),
            )
        })?;

        let cached = CachedAccessToken {
            token: new_tokens.access_token.clone(),
            expires_at_ms: compute_expires_at_ms(new_tokens.expires_in),
            obtained_at_ms,
        };

        let last_refresh = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(obtained_at_ms)
            .unwrap_or_else(chrono::Utc::now)
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let refreshed_auth = crate::codex_config::codex_managed_oauth_auth_value(
            &chatgpt_account_id,
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
        self.ensure_account_ready_for_use(account_id).await?;
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
        if let Some((live_refresh, live_id_token, live_last_refresh_ms)) = self
            .read_managed_live_auth_refresh_for_account(account_id)
            .await?
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
        let (chatgpt_account_id, id_token, refresh_token) = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
            (
                account.chatgpt_account_id.clone().ok_or_else(|| {
                    CodexOAuthError::ParseError(
                        "账号缺少 chatgpt_account_id，请重新认证".to_string(),
                    )
                })?,
                account.id_token.clone(),
                account.refresh_token.clone(),
            )
        };
        Ok(ManagedTokenBundle {
            chatgpt_account_id,
            access_token: cached.token,
            id_token,
            refresh_token,
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
        let _lifecycle = self.lifecycle_lock.read().await;
        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;
        {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
        }
        let Some((live_refresh, live_id_token, live_last_refresh_ms)) = self
            .read_managed_live_auth_refresh_for_account(account_id)
            .await?
        else {
            return Ok(None);
        };

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
            // 采纳了 CLI 轮换后的 refresh_token：与之配套的旧 access_token 可能已被
            // 服务端提前失效。在同一 accounts 写锁内（accounts -> access_tokens 顺序）
            // 清缓存，避免释放锁后被快路径读到旧 token；下次按新 refresh_token 重取。
            if material_replaced {
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

    /// 将本地账号 ID 解析为上游 ChatGPT workspace ID。
    pub async fn chatgpt_account_id_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CodexOAuthError> {
        let accounts = self.accounts.read().await;
        let account = accounts
            .get(account_id)
            .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
        account.chatgpt_account_id.clone().ok_or_else(|| {
            CodexOAuthError::ParseError("账号缺少 chatgpt_account_id，请重新认证".to_string())
        })
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

        let managed_id_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?
                .id_token
                .clone()
        };

        // Explicit Auth Center removal means credentials for this managed
        // account must leave the machine. Content matching intentionally also
        // claims a native `codex login` of the same account; that is the same
        // account-scoped credential the user just chose to remove.
        crate::codex_config::prepare_codex_live_auth_for_managed_account_removal(
            account_id,
            managed_id_token.as_deref(),
        )
        .map_err(|error| CodexOAuthError::TokenFetchFailed(error.to_string()))?;
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
        self.target_login_generations
            .write()
            .await
            .remove(account_id);

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

        let accounts_to_clear = self
            .accounts
            .read()
            .await
            .iter()
            .map(|(account_id, account)| (account_id.clone(), account.id_token.clone()))
            .collect::<Vec<_>>();
        for (account_id, id_token) in &accounts_to_clear {
            crate::codex_config::prepare_codex_live_auth_for_managed_account_removal(
                account_id,
                id_token.as_deref(),
            )
            .map_err(|error| CodexOAuthError::TokenFetchFailed(error.to_string()))?;
        }
        for (account_id, _) in &accounts_to_clear {
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
        self.target_login_generations.write().await.clear();
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
        self.add_test_account_with_workspace_and_access_token(
            account_id,
            account_id,
            access_token,
            id_token,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn add_test_account_with_user_identity(
        &self,
        account_id: &str,
        access_token: &str,
        subject: &str,
    ) -> Result<(), CodexOAuthError> {
        let id_token = crate::codex_config::test_codex_id_token(subject);
        self.add_test_account_with_access_token(account_id, access_token, Some(&id_token))
            .await
    }

    #[cfg(test)]
    pub(crate) async fn add_test_account_with_workspace_and_access_token(
        &self,
        account_id: &str,
        chatgpt_account_id: &str,
        access_token: &str,
        id_token: Option<&str>,
    ) -> Result<(), CodexOAuthError> {
        let obtained_at_ms = chrono::Utc::now().timestamp_millis();
        let data = CodexAccountData {
            account_id: account_id.to_string(),
            chatgpt_account_id: Some(chatgpt_account_id.to_string()),
            email: Some(format!("{account_id}@example.test")),
            refresh_token: "test-refresh-token".to_string(),
            authenticated_at: chrono::Utc::now().timestamp(),
            id_token: id_token.map(|token| token.to_string()),
            token_updated_at_ms: obtained_at_ms,
        };
        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(account_id.to_string(), data);
            self.access_tokens.write().await.insert(
                account_id.to_string(),
                CachedAccessToken {
                    token: access_token.to_string(),
                    expires_at_ms: obtained_at_ms + 3_600_000,
                    obtained_at_ms,
                },
            );
        }
        {
            let mut default = self.default_account_id.write().await;
            if default.is_none() {
                *default = Some(account_id.to_string());
            }
        }
        self.save_to_disk().await?;

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

    async fn add_account_internal(
        &self,
        chatgpt_account_id: String,
        refresh_token: String,
        email: Option<String>,
        id_token: Option<String>,
        initial_access_token: Option<CachedAccessToken>,
        context: AccountLoginContext<'_>,
    ) -> Result<GitHubAccount, CodexOAuthError> {
        let _lifecycle = self.lifecycle_lock.read().await;
        let target_account_id = context
            .target_account_id
            .map(str::trim)
            .filter(|account_id| !account_id.is_empty())
            .map(str::to_string);
        let replacing_existing = target_account_id.is_some();
        let account_id = target_account_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let refresh_lock = self.get_refresh_lock(&account_id).await;
        let _refresh_guard = refresh_lock.lock().await;
        let now = chrono::Utc::now().timestamp();
        let now_ms = chrono::Utc::now().timestamp_millis();

        if replacing_existing {
            let accounts = self.accounts.read().await;
            let existing = accounts
                .get(&account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.clone()))?;
            let expected_workspace = existing
                .chatgpt_account_id
                .as_deref()
                .unwrap_or(existing.account_id.as_str());
            if expected_workspace != chatgpt_account_id {
                return Err(CodexOAuthError::TokenFetchFailed(format!(
                    "重新登录的 ChatGPT workspace 与账号 {account_id} 不一致"
                )));
            }

            let new_id_token = id_token.as_deref().ok_or_else(|| {
                CodexOAuthError::TokenFetchFailed(
                    "重新登录未返回 id_token，原账号保持不变".to_string(),
                )
            })?;
            let existing_subject = existing
                .id_token
                .as_deref()
                .and_then(crate::codex_config::extract_codex_id_token_subject);
            let new_subject = crate::codex_config::extract_codex_id_token_subject(new_id_token);
            let user_identity_matches = match existing_subject.as_deref() {
                Some(existing) if new_subject.as_deref() == Some(existing) => true,
                Some(_) => {
                    return Err(CodexOAuthError::TokenFetchFailed(format!(
                        "重新登录的 ChatGPT 用户与账号 {account_id} 不一致"
                    )));
                }
                None => {
                    let existing_email = existing
                        .email
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let new_email = email
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    matches!(
                        (existing_email, new_email),
                        (Some(existing), Some(new)) if existing.eq_ignore_ascii_case(new)
                    )
                }
            };
            if !user_identity_matches {
                return Err(CodexOAuthError::TokenFetchFailed(format!(
                    "无法确认重新登录的 ChatGPT 用户属于账号 {account_id}，原账号保持不变"
                )));
            }
        }

        let data = CodexAccountData {
            account_id: account_id.clone(),
            chatgpt_account_id: Some(chatgpt_account_id),
            email,
            refresh_token,
            authenticated_at: now,
            id_token,
            token_updated_at_ms: now_ms,
        };

        let account = GitHubAccount::from(&data);

        // Linearize cancel/newer-flow against the actual commit, after waiting
        // for the account lock. Holding both guards through persistence means
        // cancellation cannot report success after this point, while a cancel
        // or newer generation that won the race makes this flow fail closed.
        let _pending_commit_guard = if let Some(device_code) = context.pending_device_code {
            let generations = self.target_login_generations.read().await;
            let mut pending_codes = self.pending_device_codes.write().await;
            let pending = pending_codes
                .get(device_code)
                .ok_or(CodexOAuthError::ExpiredToken)?;
            let generation_matches = match (
                pending.target_account_id.as_deref(),
                pending.target_generation,
            ) {
                (Some(account_id), Some(generation)) => {
                    context.target_account_id == Some(account_id)
                        && context.target_generation == Some(generation)
                        && generations.get(account_id) == Some(&generation)
                }
                (None, None) => {
                    context.target_account_id.is_none() && context.target_generation.is_none()
                }
                _ => false,
            };
            if pending.expires_at_ms <= chrono::Utc::now().timestamp_millis() || !generation_matches
            {
                return Err(CodexOAuthError::ExpiredToken);
            }
            pending_codes.remove(device_code);
            Some((generations, pending_codes))
        } else {
            None
        };

        // Persist a prospective snapshot before publishing new credentials to
        // readers. A failed atomic write therefore leaves the target account
        // and its access-token cache untouched.
        let _persist = self.storage_lock.lock().await;
        let mut persisted_accounts = self.accounts.read().await.clone();
        persisted_accounts.insert(account_id.clone(), data.clone());
        let persisted_default = self
            .resolve_default_account_id()
            .await
            .or_else(|| Some(account_id.clone()));
        let store = CodexOAuthStore {
            version: 2,
            accounts: persisted_accounts,
            default_account_id: persisted_default,
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|error| CodexOAuthError::ParseError(error.to_string()))?;
        self.write_store_atomic(&content)?;

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
        let mut default = self.default_account_id.write().await;
        if default.is_none() {
            *default = Some(account_id);
        }
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
            version: 2,
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
fn parse_jwt_claims(token: &str) -> Option<IdTokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// 从 token 响应中提取 (chatgpt_account_id, email)
fn extract_account_metadata_from_tokens(
    tokens: &OAuthTokenResponse,
) -> (Option<String>, Option<String>) {
    let mut account_id: Option<String> = None;
    let mut email: Option<String> = None;

    if let Some(id_token) = tokens.id_token.as_deref() {
        if let Some(claims) = parse_jwt_claims(id_token) {
            account_id = claims.chatgpt_account_id.clone().or_else(|| {
                claims
                    .openai_auth
                    .as_ref()
                    .and_then(|a| a.chatgpt_account_id.clone())
            });
            email = claims.email.clone();
        }
    }

    if account_id.is_none() {
        if let Some(claims) = parse_jwt_claims(&tokens.access_token) {
            account_id = claims.chatgpt_account_id.clone().or_else(|| {
                claims
                    .openai_auth
                    .as_ref()
                    .and_then(|a| a.chatgpt_account_id.clone())
            });
            if email.is_none() {
                email = claims.email.clone();
            }
        }
    }

    (account_id, email)
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
    fn test_extract_account_metadata_does_not_use_organization_id() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"organizations\":[{\"id\":\"org-456\"}]}");
        let jwt = format!("{header}.{payload}.");
        let tokens = OAuthTokenResponse {
            access_token: jwt,
            refresh_token: None,
            id_token: None,
            expires_in: None,
        };

        assert_eq!(extract_account_metadata_from_tokens(&tokens), (None, None));
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
        let account_id = {
            let manager = CodexOAuthManager::new(path.clone());
            let account = manager
                .add_account_internal(
                    "workspace-123".to_string(),
                    "rt-secret".to_string(),
                    Some("user@example.com".to_string()),
                    None,
                    None,
                    AccountLoginContext::default(),
                )
                .await
                .unwrap();
            account.id
        };

        // New manager should load from disk
        let manager2 = CodexOAuthManager::new(path);
        let accounts = manager2.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account_id);
        assert_eq!(
            manager2
                .chatgpt_account_id_for_account(&account_id)
                .await
                .unwrap(),
            "workspace-123"
        );
    }

    #[tokio::test]
    async fn test_same_workspace_accounts_are_stored_separately() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();
        let manager = CodexOAuthManager::new(path.clone());

        let first = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-first".to_string(),
                Some("first@example.com".to_string()),
                None,
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();
        let second = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-second".to_string(),
                Some("second@example.com".to_string()),
                None,
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();

        assert_ne!(first.id, second.id);
        assert_eq!(manager.list_accounts().await.len(), 2);
        assert_eq!(
            manager
                .chatgpt_account_id_for_account(&first.id)
                .await
                .unwrap(),
            "shared-workspace"
        );
        assert_eq!(
            manager
                .chatgpt_account_id_for_account(&second.id)
                .await
                .unwrap(),
            "shared-workspace"
        );

        let accounts = manager.accounts.read().await;
        assert_eq!(accounts.get(&first.id).unwrap().refresh_token, "rt-first");
        assert_eq!(accounts.get(&second.id).unwrap().refresh_token, "rt-second");
        drop(accounts);
        drop(manager);

        let manager = CodexOAuthManager::new(path);
        assert_eq!(manager.list_accounts().await.len(), 2);
        let accounts = manager.accounts.read().await;
        assert_eq!(accounts.get(&first.id).unwrap().refresh_token, "rt-first");
        assert_eq!(accounts.get(&second.id).unwrap().refresh_token, "rt-second");
    }

    #[tokio::test]
    async fn targeted_reauth_updates_account_in_place_and_preserves_binding_id() {
        let temp = tempfile::tempdir().unwrap();
        let mut manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let user_a_id_token = crate::codex_config::test_codex_id_token("user-a");
        let existing = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-old".to_string(),
                Some("user@example.com".to_string()),
                None,
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();

        let refreshed = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-new".to_string(),
                Some("user@example.com".to_string()),
                Some(user_a_id_token.clone()),
                Some(CachedAccessToken {
                    token: "access-new".to_string(),
                    expires_at_ms: i64::MAX,
                    obtained_at_ms: 42,
                }),
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(refreshed.id, existing.id);
        assert_eq!(manager.list_accounts().await.len(), 1);
        let accounts = manager.accounts.read().await;
        let account = accounts.get(&existing.id).unwrap();
        assert_eq!(account.refresh_token, "rt-new");
        assert_eq!(account.id_token.as_deref(), Some(user_a_id_token.as_str()));
        drop(accounts);
        assert_eq!(
            manager
                .access_tokens
                .read()
                .await
                .get(&existing.id)
                .map(|token| token.token.as_str()),
            Some("access-new")
        );

        let mismatch = manager
            .add_account_internal(
                "different-workspace".to_string(),
                "rt-wrong".to_string(),
                None,
                Some("id-wrong".to_string()),
                None,
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(
            mismatch,
            Err(CodexOAuthError::TokenFetchFailed(_))
        ));
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get(&existing.id)
                .unwrap()
                .refresh_token,
            "rt-new"
        );

        let other_user = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-other-user".to_string(),
                Some("user@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("user-b")),
                None,
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(
            other_user,
            Err(CodexOAuthError::TokenFetchFailed(_))
        ));
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get(&existing.id)
                .unwrap()
                .refresh_token,
            "rt-new"
        );

        // Make the configured store path unwritable as a file. Reauth must not
        // publish the new generation in memory when the atomic write fails.
        manager.storage_path = temp.path().to_path_buf();
        let write_failure = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "rt-uncommitted".to_string(),
                Some("user@example.com".to_string()),
                Some(user_a_id_token),
                Some(CachedAccessToken {
                    token: "access-uncommitted".to_string(),
                    expires_at_ms: i64::MAX,
                    obtained_at_ms: 43,
                }),
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(write_failure, Err(CodexOAuthError::IoError(_))));
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get(&existing.id)
                .unwrap()
                .refresh_token,
            "rt-new"
        );
        assert_eq!(
            manager
                .access_tokens
                .read()
                .await
                .get(&existing.id)
                .map(|token| token.token.as_str()),
            Some("access-new")
        );
    }

    #[tokio::test]
    async fn targeted_reauth_accepts_email_change_for_same_subject() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let id_token = crate::codex_config::test_codex_id_token("stable-user");
        let existing = manager
            .add_account_internal(
                "workspace".to_string(),
                "refresh-old".to_string(),
                Some("old@example.com".to_string()),
                Some(id_token.clone()),
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();

        manager
            .add_account_internal(
                "workspace".to_string(),
                "refresh-new".to_string(),
                Some("new@example.com".to_string()),
                Some(id_token),
                None,
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let accounts = manager.accounts.read().await;
        let account = accounts.get(&existing.id).unwrap();
        assert_eq!(account.email.as_deref(), Some("new@example.com"));
        assert_eq!(account.refresh_token, "refresh-new");
    }

    #[tokio::test]
    async fn test_v1_store_stays_quarantined_until_targeted_reauth() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();
        let manager = CodexOAuthManager::new(path.clone());
        let legacy = serde_json::json!({
            "version": 1,
            "accounts": {
                "legacy-workspace": {
                    "account_id": "legacy-workspace",
                    "email": "legacy@example.com",
                    "refresh_token": "rt-legacy",
                    "id_token": "legacy-id-token",
                    "authenticated_at": 1
                }
            },
            "default_account_id": "legacy-workspace"
        });
        manager
            .write_store_atomic(&serde_json::to_string(&legacy).unwrap())
            .unwrap();
        drop(manager);

        let manager = CodexOAuthManager::new(path.clone());
        let accounts = manager.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "legacy-workspace");
        assert!(accounts[0].reauth_required);
        assert!(matches!(
            manager
                .chatgpt_account_id_for_account("legacy-workspace")
                .await,
            Err(CodexOAuthError::ParseError(_))
        ));
        assert!(matches!(
            manager
                .get_valid_token_for_account("legacy-workspace")
                .await,
            Err(CodexOAuthError::ParseError(_))
        ));

        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let unresolved_payload =
            URL_SAFE_NO_PAD.encode(b"{\"organizations\":[{\"id\":\"org-456\"}]}");
        let unresolved_tokens = OAuthTokenResponse {
            access_token: format!("{header}.{unresolved_payload}."),
            refresh_token: Some("rt-rotated".to_string()),
            id_token: None,
            expires_in: Some(3600),
        };
        {
            let mut stored = manager.accounts.write().await;
            assert!(stored
                .get_mut("legacy-workspace")
                .unwrap()
                .apply_refreshed_tokens(&unresolved_tokens));
        }
        manager.save_to_disk().await.unwrap();
        drop(manager);

        let manager = CodexOAuthManager::new(path.clone());
        assert!(manager
            .chatgpt_account_id_for_account("legacy-workspace")
            .await
            .is_err());
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("legacy-workspace")
                .unwrap()
                .refresh_token,
            "rt-rotated"
        );

        let payload = URL_SAFE_NO_PAD.encode(
            b"{\"https://api.openai.com/auth\":{\"chatgpt_account_id\":\"actual-workspace\"}}",
        );
        let refreshed_tokens = OAuthTokenResponse {
            access_token: format!("{header}.{payload}."),
            refresh_token: None,
            id_token: None,
            expires_in: Some(3600),
        };
        {
            let mut stored = manager.accounts.write().await;
            assert!(!stored
                .get_mut("legacy-workspace")
                .unwrap()
                .apply_refreshed_tokens(&refreshed_tokens));
        }
        manager.save_to_disk().await.unwrap();
        drop(manager);

        let manager = CodexOAuthManager::new(path);
        assert_eq!(
            manager.default_account_id().await.as_deref(),
            Some("legacy-workspace")
        );
        assert!(manager
            .chatgpt_account_id_for_account("legacy-workspace")
            .await
            .is_err());
        assert!(manager.list_accounts().await[0].reauth_required);
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("legacy-workspace")
                .unwrap()
                .refresh_token,
            "rt-rotated"
        );

        manager
            .add_account_internal(
                "legacy-workspace".to_string(),
                "rt-reauthenticated".to_string(),
                Some("legacy@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("legacy-user")),
                None,
                AccountLoginContext {
                    target_account_id: Some("legacy-workspace"),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            manager
                .chatgpt_account_id_for_account("legacy-workspace")
                .await
                .unwrap(),
            "legacy-workspace"
        );
        assert!(!manager.list_accounts().await[0].reauth_required);
    }

    #[tokio::test]
    async fn test_remove_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        let first = manager
            .add_account_internal(
                "acc-123".to_string(),
                "rt".to_string(),
                Some("a@example.com".to_string()),
                None,
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();
        let second = manager
            .add_account_internal(
                "acc-456".to_string(),
                "rt2".to_string(),
                Some("b@example.com".to_string()),
                None,
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();

        manager.remove_account(&first.id).await.unwrap();
        let accounts = manager.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, second.id);
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
                target_account_id: None,
                target_generation: None,
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
                AccountLoginContext {
                    pending_device_code: Some("device-auth-id"),
                    ..Default::default()
                },
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
                None,
                None,
            )
            .await;

        assert!(matches!(result, Err(CodexOAuthError::ExpiredToken)));
        assert!(manager.pending_device_codes.read().await.is_empty());
    }

    #[tokio::test]
    async fn cancel_device_flow_invalidates_a_pending_commit() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .register_pending_device_code(
                "cancelled-device-auth-id".to_string(),
                "ABCD-EFGH".to_string(),
                chrono::Utc::now().timestamp_millis() + 60_000,
                manager.login_epoch.load(Ordering::Acquire),
                Some("target-account".to_string()),
                None,
            )
            .await
            .unwrap();

        assert!(manager.cancel_device_flow("cancelled-device-auth-id").await);

        assert!(manager.pending_device_codes.read().await.is_empty());
    }

    #[tokio::test]
    async fn cancel_device_flow_invalidates_commit_waiting_for_account_lock() {
        let temp = tempfile::tempdir().unwrap();
        let manager = Arc::new(CodexOAuthManager::new(temp.path().to_path_buf()));
        let existing = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "refresh-original".to_string(),
                Some("user@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("user-a")),
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();
        manager
            .target_login_generations
            .write()
            .await
            .insert(existing.id.clone(), 1);
        manager
            .register_pending_device_code(
                "cancelled-waiting-flow".to_string(),
                "ABCD-EFGH".to_string(),
                chrono::Utc::now().timestamp_millis() + 60_000,
                manager.login_epoch.load(Ordering::Acquire),
                Some(existing.id.clone()),
                Some(1),
            )
            .await
            .unwrap();

        let refresh_lock = manager.get_refresh_lock(&existing.id).await;
        let refresh_guard = refresh_lock.lock().await;
        let commit_manager = Arc::clone(&manager);
        let account_id = existing.id.clone();
        let commit = tokio::spawn(async move {
            commit_manager
                .add_account_internal(
                    "shared-workspace".to_string(),
                    "refresh-cancelled".to_string(),
                    Some("user@example.com".to_string()),
                    Some(crate::codex_config::test_codex_id_token("user-a")),
                    None,
                    AccountLoginContext {
                        target_account_id: Some(&account_id),
                        pending_device_code: Some("cancelled-waiting-flow"),
                        target_generation: Some(1),
                    },
                )
                .await
        });

        for _ in 0..100 {
            if Arc::strong_count(&refresh_lock) >= 3 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            Arc::strong_count(&refresh_lock) >= 3,
            "commit did not reach the account lock"
        );

        assert!(manager.cancel_device_flow("cancelled-waiting-flow").await);
        drop(refresh_guard);

        let result = tokio::time::timeout(std::time::Duration::from_secs(1), commit)
            .await
            .expect("commit should finish")
            .expect("commit task should not panic");
        assert!(matches!(result, Err(CodexOAuthError::ExpiredToken)));
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get(&existing.id)
                .unwrap()
                .refresh_token,
            "refresh-original"
        );
    }

    #[tokio::test]
    async fn only_latest_targeted_device_flow_can_commit() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        let existing = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "refresh-original".to_string(),
                Some("user@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("user-a")),
                None,
                AccountLoginContext::default(),
            )
            .await
            .unwrap();

        manager
            .target_login_generations
            .write()
            .await
            .insert(existing.id.clone(), 2);
        for (device_code, generation) in [("stale-flow", 1), ("latest-flow", 2)] {
            manager.pending_device_codes.write().await.insert(
                device_code.to_string(),
                PendingDeviceCode {
                    user_code: "ABCD-EFGH".to_string(),
                    expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
                    target_account_id: Some(existing.id.clone()),
                    target_generation: Some(generation),
                },
            );
        }

        let stale = manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "refresh-stale".to_string(),
                Some("user@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("user-a")),
                None,
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    pending_device_code: Some("stale-flow"),
                    target_generation: Some(1),
                },
            )
            .await;
        assert!(matches!(stale, Err(CodexOAuthError::ExpiredToken)));

        manager
            .add_account_internal(
                "shared-workspace".to_string(),
                "refresh-latest".to_string(),
                Some("user@example.com".to_string()),
                Some(crate::codex_config::test_codex_id_token("user-a")),
                None,
                AccountLoginContext {
                    target_account_id: Some(&existing.id),
                    pending_device_code: Some("latest-flow"),
                    target_generation: Some(2),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get(&existing.id)
                .unwrap()
                .refresh_token,
            "refresh-latest"
        );
    }

    #[tokio::test]
    async fn device_flow_cannot_commit_after_expiry() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager.pending_device_codes.write().await.insert(
            "expired-flow".to_string(),
            PendingDeviceCode {
                user_code: "ABCD-EFGH".to_string(),
                expires_at_ms: chrono::Utc::now().timestamp_millis() - 1,
                target_account_id: None,
                target_generation: None,
            },
        );

        let result = manager
            .add_account_internal(
                "workspace".to_string(),
                "refresh".to_string(),
                None,
                Some(crate::codex_config::test_codex_id_token("user")),
                None,
                AccountLoginContext {
                    pending_device_code: Some("expired-flow"),
                    ..Default::default()
                },
            )
            .await;

        assert!(matches!(result, Err(CodexOAuthError::ExpiredToken)));
        assert!(manager.list_accounts().await.is_empty());
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
