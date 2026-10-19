//! CodeBuddy（腾讯国内站 copilot.tencent.com）OAuth 授权中心管理器。
//!
//! 登录流程：`POST /v2/plugin/auth/state` → 浏览器授权 → 轮询
//! `GET /v2/plugin/auth/token`（code 11217 表示等待用户）→
//! `GET /v2/plugin/login/account`（code 12151 表示账号信息准备中）。
//!
//! Token 刷新走 `POST /v2/plugin/auth/token/refresh`，refresh token 可能轮换，
//! 响应带新值时原子持久化，缺省时保留旧值；HTTP 401/403 标记 `requires_reauth`。
//!
//! 额度查询：个人账号用 `billing/meter/get-user-resource-summary`（无 `/v2`
//! 前缀）+ best-effort 的 free-packages 补充包名/周期；企业账号走
//! `v2/billing/meter/get-enterprise-user-usage`。
//!
//! 存储文件：`<app_config_dir>/codebuddy_oauth_auth.json`。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::GitHubDeviceCodeResponse;
use super::CODEBUDDY_API_BASE_URL;

/// 浏览器侧 Origin/Referer（登录、额度接口常见要求）。
const CODEBUDDY_ORIGIN: &str = "https://www.codebuddy.cn";
/// 默认 `X-Domain`，token 响应里的 domain 优先。
const CODEBUDDY_DOMAIN: &str = "copilot.tencent.com";
/// 与 CodeBuddy CLI 对齐的 User-Agent（额度接口会校验该形态）。
const CODEBUDDY_USER_AGENT: &str = "CLI/2.107.0 CodeBuddy/2.107.0";

/// 授权 state 有效期（秒），与前端 `expires_in` 一致。
const AUTH_STATE_TTL_SECS: u64 = 600;
/// 前端轮询间隔（秒）。
const POLL_INTERVAL_SECS: u64 = 5;
/// token 响应缺少 expiresAt/expiresIn 时的保守有效期。
const DEFAULT_TOKEN_LIFETIME_SECS: i64 = 7_200;
/// 提前 60 秒刷新 token。
const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;
const REQUEST_TIMEOUT_SECS: u64 = 30;
const QUOTA_TIMEOUT_SECS: u64 = 20;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// 登录进行中（继续轮询，不是错误）。
const AUTH_PENDING_CODE: i64 = 11217;
/// 账号信息准备中（继续轮询，不是错误）。
const ACCOUNT_PENDING_CODE: i64 = 12151;

/// 免费包代码表（与 CodeBuddy 客户端一致）。
const FREE_PACKAGE_CODES: [&str; 11] = [
    "TCACA_code_001_PqouKr6QWV",
    "TCACA_code_008_cfWoLwvjU4",
    "TCACA_code_035_ArVxJcGDsm",
    "TCACA_code_006_DbXS0lrypC",
    "TCACA_code_039_KRcQj7wUat",
    "TCACA_code_040_mi9rCYg46x",
    "TCACA_code_007_nzdH5h4Nl0",
    "TCACA_code_028_NtpWi0jzXs",
    "TCACA_code_037_WxOD3MpI2o",
    "TCACA_code_029_6wCGEWquYy",
    "TCACA_code_030_BjSt89qTvr",
];

#[derive(Debug, thiserror::Error)]
pub enum CodebuddyOAuthError {
    #[error("等待用户授权中")]
    AuthorizationPending,
    #[error("授权已过期，请重新启动登录")]
    AuthStateExpired,
    #[error("CodeBuddy 登录失败: {0}")]
    TokenFetchFailed(String),
    #[error("CodeBuddy 许可证已过期，请重新登录")]
    LicenseExpired,
    #[error("CodeBuddy 试用授权已过期，请重新登录")]
    TrialExpired,
    #[error("企业许可证没有可用席位")]
    NoAvailableSeat,
    #[error("当前 IP 被访问策略限制")]
    IpRestricted,
    #[error("Refresh Token 失效或已过期，请重新登录 CodeBuddy")]
    RefreshTokenInvalid,
    #[error("账号需要重新登录: {0}")]
    ReauthRequired(String),
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("解析错误: {0}")]
    ParseError(String),
    #[error("IO 错误: {0}")]
    IoError(String),
    #[error("账号不存在: {0}")]
    AccountNotFound(String),
}

impl From<reqwest::Error> for CodebuddyOAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CodebuddyOAuthError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

#[derive(Debug, Clone)]
struct CachedAccessToken {
    token: String,
    expires_at_ms: i64,
}

impl CachedAccessToken {
    fn is_expiring_soon(&self) -> bool {
        self.expires_at_ms - chrono::Utc::now().timestamp_millis() < TOKEN_REFRESH_BUFFER_MS
    }
}

#[derive(Debug, Clone, Default)]
struct TokenBundle {
    access_token: String,
    /// 归一化后的 unix 秒。
    expires_at: i64,
    refresh_token: Option<String>,
    refresh_expires_at: Option<i64>,
    domain: Option<String>,
    enterprise_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct LoginAccount {
    uid: String,
    nickname: Option<String>,
    account_type: Option<String>,
    enterprise_id: Option<String>,
    enterprise_name: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingAuth {
    expires_at_ms: i64,
    /// 已经换到的 token；账号信息阶段失败时不必重新拿 token。
    token: Option<TokenBundle>,
}

/// 落盘的账号凭据（`codebuddy_oauth_auth.json` 的单个账号）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyAccountData {
    pub account_id: String,
    /// 账号接口的 uid；额度请求的 `X-User-Id` 用它（不是 JWT sub）。
    #[serde(default)]
    pub uid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_type: Option<String>,
    #[serde(default = "default_domain")]
    pub domain: String,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// unix 秒。
    pub expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_expires_at: Option<i64>,
    pub authenticated_at: i64,
    #[serde(default)]
    pub requires_reauth: bool,
}

fn default_domain() -> String {
    CODEBUDDY_DOMAIN.to_string()
}

/// 授权中心展示用的账号视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyOAuthAccount {
    pub id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub github_domain: String,
    pub requires_reauth: bool,
    pub email: Option<String>,
    pub enterprise_name: Option<String>,
    pub account_type: Option<String>,
}

impl From<&CodebuddyAccountData> for CodebuddyOAuthAccount {
    fn from(data: &CodebuddyAccountData) -> Self {
        let login = data
            .login
            .clone()
            .filter(|login| !login.trim().is_empty())
            .unwrap_or_else(|| {
                if data.uid.trim().is_empty() {
                    let short = short_id(&data.account_id);
                    format!("CodeBuddy ({short})")
                } else {
                    data.uid.clone()
                }
            });
        Self {
            id: data.account_id.clone(),
            login,
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: CODEBUDDY_DOMAIN.to_string(),
            requires_reauth: data.requires_reauth,
            email: data.email.clone(),
            enterprise_name: data.enterprise_name.clone(),
            account_type: data.account_type.clone(),
        }
    }
}

fn short_id(account_id: &str) -> String {
    account_id.chars().take(8).collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodebuddyOAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, CodebuddyAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyOAuthStatus {
    pub accounts: Vec<CodebuddyOAuthAccount>,
    pub default_account_id: Option<String>,
    pub authenticated: bool,
    pub username: Option<String>,
}

/// 一次上游请求需要的账号上下文：真实 token + CodeBuddy CLI 专属请求头取值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodebuddyRequestContext {
    pub account_id: String,
    /// 已刷新到有效期的 access token。
    pub token: String,
    /// 账号接口的 uid（`X-User-Id` 用它，不是 JWT sub）。
    pub uid: String,
    /// `X-Domain`；非法值已回退 `copilot.tencent.com`。
    pub domain: String,
    /// 企业账号的 `X-Enterprise-Id` / `X-Tenant-Id`；个人账号为 `None`。
    pub enterprise_id: Option<String>,
}

/// `/v3/config` 模型目录条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodebuddyModelInfo {
    pub id: String,
    pub vendor: Option<String>,
}

/// `get_codebuddy_oauth_quota` 的返回结构（camelCase 入参、snake_case 字段）。
#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyQuota {
    pub success: bool,
    pub error: Option<String>,
    /// unix 毫秒（与 `SubscriptionQuota.queried_at` 一致，前端按毫秒算「x 分钟前」）。
    pub queried_at: Option<i64>,
    pub total: f64,
    pub remaining: f64,
    /// 0-100，四舍五入。
    pub remaining_percent: u8,
    pub packages: Vec<CodebuddyQuotaPackage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodebuddyQuotaPackage {
    pub name: String,
    pub total: f64,
    pub remaining: f64,
    pub used: f64,
    pub cycle_start: Option<String>,
    pub cycle_end: Option<String>,
}

impl CodebuddyQuota {
    fn success(packages: Vec<CodebuddyQuotaPackage>, queried_at: i64) -> Self {
        let total: f64 = packages.iter().map(|package| package.total).sum();
        let remaining: f64 = packages.iter().map(|package| package.remaining).sum();
        let remaining_percent = remaining_percent(remaining, total);
        Self {
            success: true,
            error: None,
            queried_at: Some(queried_at),
            total,
            remaining,
            remaining_percent,
            packages,
        }
    }

    fn failure(message: impl Into<String>, queried_at: i64) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
            queried_at: Some(queried_at),
            total: 0.0,
            remaining: 0.0,
            remaining_percent: 0,
            packages: Vec::new(),
        }
    }
}

/// 单包内部事实（summary 与 free-packages 合并前）。
#[derive(Debug, Clone, Default)]
struct QuotaPackageFact {
    code: String,
    name: Option<String>,
    total: f64,
    remaining: f64,
    used: f64,
    cycle_start: Option<String>,
    cycle_end: Option<String>,
}

/// free-packages 只补包名 / 周期结束时间；余额一律以 summary 为准。
#[derive(Debug, Clone, Default)]
struct FreePackageFact {
    code: String,
    name: Option<String>,
    cycle_end: Option<String>,
}

pub struct CodebuddyOAuthManager {
    accounts: Arc<RwLock<HashMap<String, CodebuddyAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    pending_states: Arc<RwLock<HashMap<String, PendingAuth>>>,
    mutation_lock: Arc<Mutex<()>>,
    storage_path: PathBuf,
}

impl CodebuddyOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            pending_states: Arc::new(RwLock::new(HashMap::new())),
            mutation_lock: Arc::new(Mutex::new(())),
            storage_path: data_dir.join("codebuddy_oauth_auth.json"),
        };
        if let Err(error) = manager.load_from_disk_sync() {
            log::warn!("[CodebuddyOAuth] 加载存储失败: {error}");
        }
        manager
    }

    /// 启动 CodeBuddy 账号 OAuth：申请 auth_state 与浏览器授权链接。
    pub async fn start_auth(&self) -> Result<GitHubDeviceCodeResponse, CodebuddyOAuthError> {
        let (request_id, _span_id) = next_request_ids();
        let response = apply_login_headers(
            crate::proxy::http_client::get()
                .post(format!(
                    "{CODEBUDDY_API_BASE_URL}/v2/plugin/auth/state?platform=CLI"
                ))
                .json(&serde_json::json!({})),
            &request_id,
            None,
        )
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        let value = read_json_response(response).await;
        if !status.is_success() {
            return Err(CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 授权服务暂不可用: HTTP {status}"
            )));
        }
        let start = parse_auth_state_response(&value?)?;

        let now_ms = chrono::Utc::now().timestamp_millis();
        {
            let mut pending = self.pending_states.write().await;
            pending.retain(|_, entry| entry.expires_at_ms > now_ms);
            pending.insert(
                start.state.clone(),
                PendingAuth {
                    expires_at_ms: now_ms.saturating_add(
                        i64::try_from(AUTH_STATE_TTL_SECS)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(1_000),
                    ),
                    token: None,
                },
            );
        }

        Ok(GitHubDeviceCodeResponse {
            device_code: start.state,
            user_code: String::new(),
            verification_uri: start.auth_url,
            expires_in: AUTH_STATE_TTL_SECS,
            interval: POLL_INTERVAL_SECS,
        })
    }

    /// 轮询登录状态；等待中返回 `AuthorizationPending`（命令层转成 `Ok(None)`）。
    pub async fn poll_for_account(
        &self,
        state: &str,
    ) -> Result<Option<CodebuddyOAuthAccount>, CodebuddyOAuthError> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut entry = self
            .pending_states
            .read()
            .await
            .get(state)
            .cloned()
            .ok_or_else(|| {
                CodebuddyOAuthError::TokenFetchFailed(
                    "授权 state 不存在或已过期，请重新启动登录".to_string(),
                )
            })?;
        if entry.expires_at_ms <= now_ms {
            self.pending_states.write().await.remove(state);
            return Err(CodebuddyOAuthError::AuthStateExpired);
        }

        if entry.token.is_none() {
            let Some(token) = self.poll_token(state).await? else {
                return Err(CodebuddyOAuthError::AuthorizationPending);
            };
            {
                let mut pending = self.pending_states.write().await;
                let Some(pending_entry) = pending.get_mut(state) else {
                    return Err(CodebuddyOAuthError::AuthStateExpired);
                };
                pending_entry.token = Some(token.clone());
            }
            entry.token = Some(token);
        }

        let token = entry
            .token
            .ok_or(CodebuddyOAuthError::AuthorizationPending)?;
        let Some(account) = self.poll_account(state, &token).await? else {
            return Err(CodebuddyOAuthError::AuthorizationPending);
        };

        let data = build_account_data(&token, &account, chrono::Utc::now().timestamp());
        let view = CodebuddyOAuthAccount::from(&data);
        self.add_account_internal(data, Some(state)).await?;
        Ok(Some(view))
    }

    /// 取消尚未完成的登录。
    pub async fn cancel_auth(&self, state: &str) -> bool {
        self.pending_states.write().await.remove(state).is_some()
    }

    /// 获取（必要时自动刷新）指定账号的 access token。
    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CodebuddyOAuthError> {
        if let Some(token) = self.cached_token_for_usable_account(account_id).await {
            return Ok(token);
        }

        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _refresh_guard = refresh_lock.lock().await;
        if let Some(token) = self.cached_token_for_usable_account(account_id).await {
            return Ok(token);
        }

        let account = self
            .accounts
            .read()
            .await
            .get(account_id)
            .cloned()
            .ok_or_else(|| CodebuddyOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(CodebuddyOAuthError::ReauthRequired(account_id.to_string()));
        }

        let refresh_token = account.refresh_token.clone().unwrap_or_default();
        if refresh_token.trim().is_empty() {
            self.mark_reauth_required(account_id).await?;
            return Err(CodebuddyOAuthError::RefreshTokenInvalid);
        }
        if account.refresh_expires_at.is_some_and(|expires_at| {
            expires_at > 0 && chrono::Utc::now().timestamp() >= expires_at
        }) {
            self.mark_reauth_required(account_id).await?;
            return Err(CodebuddyOAuthError::RefreshTokenInvalid);
        }

        let refreshed = match self.refresh_with_token(&account).await {
            Ok(token) => token,
            Err(CodebuddyOAuthError::RefreshTokenInvalid) => {
                self.mark_reauth_required(account_id).await?;
                return Err(CodebuddyOAuthError::ReauthRequired(account_id.to_string()));
            }
            Err(error) => return Err(error),
        };

        let updated = apply_refresh(&account, refreshed);
        self.commit_refreshed_account(account_id, &refresh_token, updated)
            .await
    }

    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    /// 解析本次请求要使用的账号：显式 `account_id` 优先，否则用默认账号。
    async fn resolve_request_account_id(
        &self,
        account_id: Option<&str>,
    ) -> Result<String, CodebuddyOAuthError> {
        match account_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => Ok(id.to_string()),
            None => self.default_account_id().await.ok_or_else(|| {
                CodebuddyOAuthError::AccountNotFound(
                    "没有可用的 CodeBuddy 账号，请先在授权中心登录".to_string(),
                )
            }),
        }
    }

    /// 为一次上游请求准备账号上下文：有效 token + `X-User-Id` / `X-Domain` /
    /// `X-Enterprise-Id` 取值。token 缺失/过期时走既有刷新链路。
    pub async fn request_context(
        &self,
        account_id: Option<&str>,
    ) -> Result<CodebuddyRequestContext, CodebuddyOAuthError> {
        let account_id = self.resolve_request_account_id(account_id).await?;
        // 先拿 token（可能触发刷新并更新 domain / enterprise 上下文），再读快照。
        let token = self.get_valid_token_for_account(&account_id).await?;
        let account = self
            .accounts
            .read()
            .await
            .get(&account_id)
            .cloned()
            .ok_or_else(|| CodebuddyOAuthError::AccountNotFound(account_id.clone()))?;
        if account.requires_reauth {
            return Err(CodebuddyOAuthError::ReauthRequired(account_id));
        }
        let domain = effective_domain(&account.domain).to_string();
        let enterprise_id = account.enterprise_id.filter(|id| !id.trim().is_empty());
        Ok(CodebuddyRequestContext {
            account_id,
            token,
            uid: account.uid,
            domain,
            enterprise_id,
        })
    }

    /// 拉取当前账号的 CodeBuddy CLI 模型目录（`GET /v3/config`）。
    ///
    /// `data.models[]` 是详情（`id` / `vendor`），`data.agents[name=="cli"].models[]`
    /// 是 CLI 可用模型 ID 列表（字符串或对象）。两类都解析：有 CLI 列表时以它为
    /// 准并用详情回填 vendor，没有时退回全部 `data.models`。过滤补全/NextEdit/
    /// 提示词增强等服务模型（`completion-` / `nes-` / `enhance-` 前缀），去重排序。
    pub async fn fetch_models(
        &self,
        account_id: Option<&str>,
    ) -> Result<Vec<CodebuddyModelInfo>, CodebuddyOAuthError> {
        let context = self.request_context(account_id).await?;
        let (request_id, _span_id) = next_request_ids();
        let uid = if context.uid.trim().is_empty() {
            context.account_id.as_str()
        } else {
            context.uid.as_str()
        };
        let response = apply_authenticated_headers(
            crate::proxy::http_client::get().get(format!("{CODEBUDDY_API_BASE_URL}/v3/config")),
            &request_id,
            &context.token,
            &context.domain,
            context.enterprise_id.as_deref(),
        )
        .header("X-User-Id", uid)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(CodebuddyOAuthError::ReauthRequired(
                context.account_id.clone(),
            ));
        }
        let value = read_json_response(response).await?;
        if !status.is_success() {
            return Err(CodebuddyOAuthError::NetworkError(format!(
                "CodeBuddy 模型目录请求失败: HTTP {status}"
            )));
        }
        let code = envelope_code(&value).ok_or_else(|| {
            CodebuddyOAuthError::ParseError("CodeBuddy 模型目录响应缺少 code".into())
        })?;
        if code != 0 {
            return Err(CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 模型目录请求失败（上游 code {code}）"
            )));
        }
        let data = envelope_data(&value).ok_or_else(|| {
            CodebuddyOAuthError::ParseError("CodeBuddy 模型目录响应缺少 data".into())
        })?;
        let models = parse_models_catalog(data);
        if models.is_empty() {
            return Err(CodebuddyOAuthError::TokenFetchFailed(
                "CodeBuddy 没有返回可用模型，请确认账号权限后重试".to_string(),
            ));
        }
        Ok(models)
    }

    pub async fn get_status(&self) -> CodebuddyOAuthStatus {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());
        let username = default_account_id
            .as_ref()
            .and_then(|id| accounts.get(id))
            .and_then(|account| account.login.clone());
        CodebuddyOAuthStatus {
            // 只要还有一个可用（未被标记失效）账号就算已认证；全部账号失效时
            // 前端据此阻止保存 CodeBuddy 供应商。
            authenticated: accounts.values().any(|account| !account.requires_reauth),
            default_account_id,
            accounts: account_list,
            username,
        }
    }

    pub async fn list_accounts(&self) -> Vec<CodebuddyOAuthAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        if accounts.remove(account_id).is_none() {
            return Err(CodebuddyOAuthError::AccountNotFound(account_id.to_string()));
        }
        let stored_default = self.default_account_id.read().await.clone();
        let default_account_id = if stored_default.as_deref() == Some(account_id) {
            Self::fallback_default_account_id(&accounts)
        } else {
            stored_default.filter(|id| Self::is_usable_account(&accounts, id))
        };
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens.write().await.remove(account_id);
        self.refresh_locks.write().await.remove(account_id);
        Ok(())
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let accounts = self.accounts.read().await.clone();
        let account = accounts
            .get(account_id)
            .ok_or_else(|| CodebuddyOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(CodebuddyOAuthError::ReauthRequired(account_id.to_string()));
        }
        self.persist_and_commit(accounts, Some(account_id.to_string()))
            .await
    }

    pub async fn clear_auth(&self) -> Result<(), CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }
        *self.accounts.write().await = HashMap::new();
        *self.default_account_id.write().await = None;
        self.access_tokens.write().await.clear();
        self.refresh_locks.write().await.clear();
        self.pending_states.write().await.clear();
        Ok(())
    }

    /// 查询账号额度；任何失败都以 `success: false` 返回中文原因，不向命令层抛错。
    pub async fn query_quota(&self, account_id: Option<&str>) -> CodebuddyQuota {
        let now = chrono::Utc::now().timestamp_millis();
        let resolved = match account_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => Some(id.to_string()),
            None => self.default_account_id().await,
        };
        let Some(account_id) = resolved else {
            return CodebuddyQuota::failure("还没有登录 CodeBuddy 账号", now);
        };

        let account = self.accounts.read().await.get(&account_id).cloned();
        let Some(account) = account else {
            return CodebuddyQuota::failure("CodeBuddy 账号不存在，请刷新账号列表", now);
        };
        if account.requires_reauth {
            return CodebuddyQuota::failure("CodeBuddy 账号已失效，请重新登录", now);
        }

        let token = match self.get_valid_token_for_account(&account_id).await {
            Ok(token) => token,
            Err(error) => return CodebuddyQuota::failure(error.to_string(), now),
        };
        // 刷新可能更新 domain / enterprise 上下文，用最新凭据查额度。
        let account = self
            .accounts
            .read()
            .await
            .get(&account_id)
            .cloned()
            .unwrap_or(account);

        match self.fetch_quota(&account, &token).await {
            Ok(packages) => CodebuddyQuota::success(packages, now),
            Err(error) => CodebuddyQuota::failure(error.to_string(), now),
        }
    }

    async fn poll_token(&self, state: &str) -> Result<Option<TokenBundle>, CodebuddyOAuthError> {
        let (request_id, span_id) = next_request_ids();
        let url = format!(
            "{CODEBUDDY_API_BASE_URL}/v2/plugin/auth/token?state={}",
            percent_encode(state)
        );
        let response = apply_login_headers(
            crate::proxy::http_client::get().get(url),
            &request_id,
            Some(&span_id),
        )
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        let value = read_json_response(response).await;
        if !status.is_success() {
            return Err(CodebuddyOAuthError::NetworkError(format!(
                "CodeBuddy 登录服务暂不可用: HTTP {status}"
            )));
        }
        token_poll_outcome(&value?, chrono::Utc::now().timestamp())
    }

    async fn poll_account(
        &self,
        state: &str,
        token: &TokenBundle,
    ) -> Result<Option<LoginAccount>, CodebuddyOAuthError> {
        let (request_id, _span_id) = next_request_ids();
        let url = format!(
            "{CODEBUDDY_API_BASE_URL}/v2/plugin/login/account?state={}",
            percent_encode(state)
        );
        let domain = token
            .domain
            .as_deref()
            .map(effective_domain)
            .unwrap_or(CODEBUDDY_DOMAIN);
        let response = apply_authenticated_headers(
            crate::proxy::http_client::get().get(url),
            &request_id,
            &token.access_token,
            domain,
            None,
        )
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        // 登录尚未完成时网关会拒绝 account 请求，继续等待即可。
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Ok(None);
        }
        let value = read_json_response(response).await;
        if !status.is_success() {
            return Err(CodebuddyOAuthError::NetworkError(format!(
                "CodeBuddy 账号服务暂不可用: HTTP {status}"
            )));
        }
        account_poll_outcome(&value?)
    }

    async fn refresh_with_token(
        &self,
        account: &CodebuddyAccountData,
    ) -> Result<TokenBundle, CodebuddyOAuthError> {
        let refresh_token = account
            .refresh_token
            .clone()
            .filter(|token| !token.trim().is_empty())
            .ok_or(CodebuddyOAuthError::RefreshTokenInvalid)?;
        let (request_id, _span_id) = next_request_ids();
        let domain = effective_domain(&account.domain);
        let response = apply_authenticated_headers(
            crate::proxy::http_client::get()
                .post(format!(
                    "{CODEBUDDY_API_BASE_URL}/v2/plugin/auth/token/refresh"
                ))
                .json(&serde_json::json!({}))
                .header("X-Refresh-Token", &refresh_token)
                .header("X-Auth-Refresh-Source", "plugin"),
            &request_id,
            &account.access_token,
            domain,
            account.enterprise_id.as_deref(),
        )
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        let value_result = read_json_response(response).await;
        // Refresh token 失效必须转 requires_reauth，即使上游返回空/HTML body。
        if refresh_status_requires_reauth(status) {
            return Err(CodebuddyOAuthError::RefreshTokenInvalid);
        }
        let value = value_result?;
        if !status.is_success() {
            return Err(CodebuddyOAuthError::NetworkError(format!(
                "CodeBuddy Token 刷新失败: HTTP {status}"
            )));
        }
        let code = envelope_code(&value)
            .ok_or_else(|| CodebuddyOAuthError::ParseError("CodeBuddy 刷新响应缺少 code".into()))?;
        match code {
            0 => parse_token_bundle(&value, chrono::Utc::now().timestamp()),
            other => Err(map_business_code(other).unwrap_or_else(|| {
                CodebuddyOAuthError::TokenFetchFailed(format!(
                    "CodeBuddy 刷新 Token 失败（上游 code {other}）"
                ))
            })),
        }
    }

    async fn fetch_quota(
        &self,
        account: &CodebuddyAccountData,
        access_token: &str,
    ) -> Result<Vec<CodebuddyQuotaPackage>, CodebuddyOAuthError> {
        let has_enterprise = account
            .enterprise_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|id| !id.is_empty());
        if has_enterprise {
            return self.fetch_enterprise_quota(account, access_token).await;
        }

        let summary = self
            .fetch_quota_summary(account, access_token)
            .await
            .and_then(|value| parse_quota_summary(&value))?;
        // free-packages 只是补充包名/周期，失败不影响主余额。
        let free = match self.fetch_free_packages(account, access_token).await {
            Ok(value) => parse_free_packages(&value),
            Err(error) => {
                log::debug!("[CodebuddyOAuth] 免费包信息查询失败（忽略）: {error}");
                Vec::new()
            }
        };
        Ok(merge_quota_packages(summary, free))
    }

    async fn fetch_quota_summary(
        &self,
        account: &CodebuddyAccountData,
        access_token: &str,
    ) -> Result<Value, CodebuddyOAuthError> {
        self.post_quota_json(
            account,
            access_token,
            "/billing/meter/get-user-resource-summary",
            serde_json::json!({}),
        )
        .await
    }

    async fn fetch_free_packages(
        &self,
        account: &CodebuddyAccountData,
        access_token: &str,
    ) -> Result<Value, CodebuddyOAuthError> {
        let now = chrono::Local::now();
        let body = serde_json::json!({
            "PageNumber": 1,
            "PageSize": 100,
            "PackageCodes": FREE_PACKAGE_CODES,
            "Status": [0, 3],
            "SlicePeriodStartTime": now.format("%Y-%m-%d 00:00:00").to_string(),
            "SlicePeriodEndTime": now.format("%Y-%m-%d 23:59:59").to_string(),
        });
        self.post_quota_json(
            account,
            access_token,
            "/billing/meter/get-user-resource-free-packages",
            body,
        )
        .await
    }

    async fn fetch_enterprise_quota(
        &self,
        account: &CodebuddyAccountData,
        access_token: &str,
    ) -> Result<Vec<CodebuddyQuotaPackage>, CodebuddyOAuthError> {
        let value = self
            .post_quota_json(
                account,
                access_token,
                "/v2/billing/meter/get-enterprise-user-usage",
                serde_json::json!({}),
            )
            .await?;
        parse_enterprise_quota(&value, account.enterprise_name.as_deref())
    }

    async fn post_quota_json(
        &self,
        account: &CodebuddyAccountData,
        access_token: &str,
        path: &str,
        body: Value,
    ) -> Result<Value, CodebuddyOAuthError> {
        let (request_id, _span_id) = next_request_ids();
        let domain = effective_domain(&account.domain);
        let uid = if account.uid.trim().is_empty() {
            account.account_id.as_str()
        } else {
            account.uid.as_str()
        };
        let response = apply_authenticated_headers(
            crate::proxy::http_client::get()
                .post(format!("{CODEBUDDY_API_BASE_URL}{path}"))
                .header("X-User-Id", uid)
                .json(&body),
            &request_id,
            access_token,
            domain,
            account.enterprise_id.as_deref(),
        )
        .timeout(Duration::from_secs(QUOTA_TIMEOUT_SECS))
        .send()
        .await?;

        let status = response.status();
        let value_result = read_json_response(response).await;
        if !status.is_success() {
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                return Err(CodebuddyOAuthError::TokenFetchFailed(
                    "CodeBuddy 登录状态已失效，请重新登录".to_string(),
                ));
            }
            return Err(CodebuddyOAuthError::NetworkError(format!(
                "CodeBuddy 额度服务暂不可用: HTTP {status}"
            )));
        }
        let value = value_result?;
        match envelope_code(&value) {
            Some(0) => Ok(value),
            Some(10085) => Err(CodebuddyOAuthError::TokenFetchFailed(
                "CodeBuddy 额度接口拒绝了请求（code 10085，通常是客户端标识校验失败）".to_string(),
            )),
            Some(code) => Err(CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 额度查询失败（上游 code {code}）"
            ))),
            None => Err(CodebuddyOAuthError::ParseError(
                "CodeBuddy 额度响应缺少 code".to_string(),
            )),
        }
    }

    async fn add_account_internal(
        &self,
        data: CodebuddyAccountData,
        pending_state: Option<&str>,
    ) -> Result<(), CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if let Some(state) = pending_state {
            let login_is_pending = self.pending_states.read().await.contains_key(state);
            if !login_is_pending {
                return Err(CodebuddyOAuthError::TokenFetchFailed(
                    "登录已取消，请重新启动登录".to_string(),
                ));
            }
        }
        let account_id = data.account_id.clone();
        let cached_access_token = CachedAccessToken {
            token: data.access_token.clone(),
            expires_at_ms: to_millis(data.expires_at),
        };
        let mut accounts = self.accounts.read().await.clone();
        accounts.insert(account_id.clone(), data);
        let current_default = self.default_account_id.read().await.clone();
        let default_account_id = match current_default {
            Some(id) if Self::is_usable_account(&accounts, &id) => Some(id),
            _ => Some(account_id.clone()),
        };
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens
            .write()
            .await
            .insert(account_id, cached_access_token);
        if let Some(state) = pending_state {
            self.pending_states.write().await.remove(state);
        }
        Ok(())
    }

    async fn commit_refreshed_account(
        &self,
        account_id: &str,
        expected_refresh_token: &str,
        updated: CodebuddyAccountData,
    ) -> Result<String, CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| CodebuddyOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(CodebuddyOAuthError::ReauthRequired(account_id.to_string()));
        }
        let current_refresh_token = account.refresh_token.clone().unwrap_or_default();
        if current_refresh_token != expected_refresh_token {
            return Err(CodebuddyOAuthError::TokenFetchFailed(
                "账号认证状态已变化，请重试请求".to_string(),
            ));
        }
        *account = updated;
        let access_token = account.access_token.clone();
        let expires_at = account.expires_at;
        let default_account_id = self.default_account_id.read().await.clone();
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens.write().await.insert(
            account_id.to_string(),
            CachedAccessToken {
                token: access_token.clone(),
                expires_at_ms: to_millis(expires_at),
            },
        );
        Ok(access_token)
    }

    async fn mark_reauth_required(&self, account_id: &str) -> Result<(), CodebuddyOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| CodebuddyOAuthError::AccountNotFound(account_id.to_string()))?;
        account.requires_reauth = true;
        // 只有失效账号本来就是默认账号时才需要改默认；否则保留用户当前选择。
        let stored_default = self.default_account_id.read().await.clone();
        let default_account_id = if stored_default.as_deref() == Some(account_id) {
            Self::fallback_default_account_id(&accounts)
        } else {
            stored_default.filter(|id| Self::is_usable_account(&accounts, id))
        };
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens.write().await.remove(account_id);
        Ok(())
    }

    async fn persist_and_commit(
        &self,
        accounts: HashMap<String, CodebuddyAccountData>,
        default_account_id: Option<String>,
    ) -> Result<(), CodebuddyOAuthError> {
        let store = CodebuddyOAuthStore {
            version: 1,
            accounts: accounts.clone(),
            default_account_id: default_account_id.clone(),
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|error| CodebuddyOAuthError::ParseError(error.to_string()))?;
        self.write_store_atomic(&content)?;
        *self.accounts.write().await = accounts;
        *self.default_account_id.write().await = default_account_id;
        Ok(())
    }

    async fn cached_token(&self, account_id: &str) -> Option<String> {
        self.access_tokens
            .read()
            .await
            .get(account_id)
            .filter(|token| !token.is_expiring_soon())
            .map(|token| token.token.clone())
    }

    async fn cached_token_for_usable_account(&self, account_id: &str) -> Option<String> {
        let account_is_usable = {
            let accounts = self.accounts.read().await;
            Self::is_usable_account(&accounts, account_id)
        };
        if !account_is_usable {
            return None;
        }
        self.cached_token(account_id).await
    }

    async fn get_refresh_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        if let Some(lock) = self.refresh_locks.read().await.get(account_id).cloned() {
            return lock;
        }
        Arc::clone(
            self.refresh_locks
                .write()
                .await
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    async fn resolve_default_account_id(&self) -> Option<String> {
        let stored = self.default_account_id.read().await.clone();
        let accounts = self.accounts.read().await;
        match stored {
            Some(id) if Self::is_usable_account(&accounts, &id) => Some(id),
            _ => Self::fallback_default_account_id(&accounts),
        }
    }

    fn fallback_default_account_id(
        accounts: &HashMap<String, CodebuddyAccountData>,
    ) -> Option<String> {
        accounts
            .iter()
            .filter(|(_, account)| !account.requires_reauth)
            .max_by(|(id_a, account_a), (id_b, account_b)| {
                account_a
                    .authenticated_at
                    .cmp(&account_b.authenticated_at)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, _)| id.clone())
    }

    fn is_usable_account(accounts: &HashMap<String, CodebuddyAccountData>, id: &str) -> bool {
        accounts
            .get(id)
            .is_some_and(|account| !account.requires_reauth)
    }

    fn sorted_accounts(
        accounts: &HashMap<String, CodebuddyAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<CodebuddyOAuthAccount> {
        let mut result: Vec<_> = accounts.values().map(CodebuddyOAuthAccount::from).collect();
        result.sort_by(|a, b| {
            let a_default = default_account_id == Some(a.id.as_str());
            let b_default = default_account_id == Some(b.id.as_str());
            b_default
                .cmp(&a_default)
                .then_with(|| a.requires_reauth.cmp(&b.requires_reauth))
                .then_with(|| b.authenticated_at.cmp(&a.authenticated_at))
                .then_with(|| a.login.cmp(&b.login))
        });
        result
    }

    fn load_from_disk_sync(&self) -> Result<(), CodebuddyOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)?;
        let store: CodebuddyOAuthStore = serde_json::from_str(&content)
            .map_err(|error| CodebuddyOAuthError::ParseError(error.to_string()))?;
        // 落盘的 access token 仍可能有效，先放进缓存，避免每次启动都刷新一次
        // （refresh token 轮换虽然已持久化，但少一次上游调用更稳）。
        let cached_tokens: HashMap<String, CachedAccessToken> = store
            .accounts
            .values()
            .filter(|account| !account.access_token.is_empty() && account.expires_at > 0)
            .map(|account| {
                (
                    account.account_id.clone(),
                    CachedAccessToken {
                        token: account.access_token.clone(),
                        expires_at_ms: to_millis(account.expires_at),
                    },
                )
            })
            .collect();
        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }
        if let Ok(mut access_tokens) = self.access_tokens.try_write() {
            *access_tokens = cached_tokens;
        }
        Ok(())
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CodebuddyOAuthError> {
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| CodebuddyOAuthError::IoError("无效的存储路径".to_string()))?;
        fs::create_dir_all(parent)?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| CodebuddyOAuthError::IoError("无效的存储文件名".to_string()))?
            .to_string_lossy();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary_path = parent.join(format!("{file_name}.tmp.{nonce}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let result = (|| -> Result<(), std::io::Error> {
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&temporary_path)?;
                file.write_all(content.as_bytes())?;
                file.flush()?;
                fs::rename(&temporary_path, &self.storage_path)?;
                fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary_path);
            }
            result?;
        }

        #[cfg(windows)]
        {
            let result = (|| -> Result<(), std::io::Error> {
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temporary_path)?;
                file.write_all(content.as_bytes())?;
                file.flush()?;
                if self.storage_path.exists() {
                    fs::remove_file(&self.storage_path)?;
                }
                fs::rename(&temporary_path, &self.storage_path)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary_path);
            }
            result?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct StartAuthState {
    state: String,
    auth_url: String,
}

fn parse_auth_state_response(value: &Value) -> Result<StartAuthState, CodebuddyOAuthError> {
    let code = envelope_code(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 授权响应缺少 code".to_string())
    })?;
    if code != 0 {
        return Err(map_business_code(code).unwrap_or_else(|| {
            CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 授权启动失败（上游 code {code}）"
            ))
        }));
    }
    let data = envelope_data(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 授权响应缺少 data".to_string())
    })?;
    let state = value_str(data, &["state"]).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 授权响应缺少 state".to_string())
    })?;
    // authUrl 不做 trim：首尾带空白的链接视为不安全，由 validate_auth_url 拒绝。
    let auth_url = data
        .get("authUrl")
        .or_else(|| data.get("auth_url"))
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            CodebuddyOAuthError::ParseError("CodeBuddy 授权响应缺少 authUrl".to_string())
        })?;
    validate_auth_url(auth_url, state)?;
    Ok(StartAuthState {
        state: state.to_string(),
        auth_url: auth_url.to_string(),
    })
}

/// 只允许绝对 http(s)、无 userinfo、无控制字符、host 非空的授权链接；
/// 如果 URL 自带 state query，则必须与响应 state 一致。
fn validate_auth_url(auth_url: &str, state: &str) -> Result<(), CodebuddyOAuthError> {
    let invalid =
        || CodebuddyOAuthError::ParseError("CodeBuddy 返回了不安全的授权链接".to_string());
    if auth_url.is_empty() || auth_url.trim() != auth_url {
        return Err(invalid());
    }
    if auth_url
        .chars()
        .any(|character| character < ' ' || character == '\u{7f}')
    {
        return Err(invalid());
    }
    if !has_non_empty_authority(auth_url) {
        return Err(invalid());
    }
    let url = url::Url::parse(auth_url).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid());
    }
    let Some(host) = url.host_str() else {
        return Err(invalid());
    };
    if host.is_empty() || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    if let Some(query_state) = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
    {
        if query_state != state {
            return Err(CodebuddyOAuthError::ParseError(
                "CodeBuddy 授权链接的 state 与响应不一致".to_string(),
            ));
        }
    }
    Ok(())
}

/// WHATWG URL 解析器会把 `https:///login` 补成 `https://login/`（host 变成
/// `login`）。这里显式要求 `://` 后到下一个分隔符之间的 authority 非空，
/// 空 authority 一律视为不安全。
fn has_non_empty_authority(auth_url: &str) -> bool {
    let lowered = auth_url.to_ascii_lowercase();
    let Some((_, rest)) = lowered.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty()
}

/// 轮询 token 的结果：`Ok(None)` 表示继续等待。
fn token_poll_outcome(value: &Value, now: i64) -> Result<Option<TokenBundle>, CodebuddyOAuthError> {
    let code = envelope_code(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 登录响应缺少 code".to_string())
    })?;
    match code {
        0 => Ok(Some(parse_token_bundle(value, now)?)),
        AUTH_PENDING_CODE => Ok(None),
        other => Err(map_business_code(other).unwrap_or_else(|| {
            CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 登录失败（上游 code {other}）"
            ))
        })),
    }
}

/// 轮询 account 的结果：`Ok(None)` 表示继续等待。
fn account_poll_outcome(value: &Value) -> Result<Option<LoginAccount>, CodebuddyOAuthError> {
    let code = envelope_code(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 账号响应缺少 code".to_string())
    })?;
    match code {
        0 => Ok(Some(parse_login_account(value)?)),
        ACCOUNT_PENDING_CODE => Ok(None),
        other => Err(map_business_code(other).unwrap_or_else(|| {
            CodebuddyOAuthError::TokenFetchFailed(format!(
                "CodeBuddy 账号信息获取失败（上游 code {other}）"
            ))
        })),
    }
}

fn parse_token_bundle(value: &Value, now: i64) -> Result<TokenBundle, CodebuddyOAuthError> {
    let data = envelope_data(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 登录响应缺少 data".to_string())
    })?;
    let access_token = value_str(data, &["accessToken", "access_token"]).ok_or_else(|| {
        CodebuddyOAuthError::TokenFetchFailed("CodeBuddy 登录响应缺少 access_token".to_string())
    })?;

    let expires_at = epoch_field(data, &["expiresAt", "expires_at"])
        .or_else(|| {
            flexible_i64_field(data, &["expiresIn", "expires_in"])
                .map(|seconds| now.saturating_add(seconds.max(1)))
        })
        .unwrap_or_else(|| now.saturating_add(DEFAULT_TOKEN_LIFETIME_SECS));

    let refresh_expires_at = epoch_field(data, &["refreshExpiresAt", "refresh_expires_at"])
        .or_else(|| {
            flexible_i64_field(data, &["refreshExpiresIn", "refresh_expires_in"])
                .map(|seconds| now.saturating_add(seconds.max(1)))
        });

    Ok(TokenBundle {
        access_token: access_token.to_string(),
        expires_at,
        refresh_token: value_str(data, &["refreshToken", "refresh_token"]).map(str::to_string),
        refresh_expires_at,
        domain: value_str(data, &["domain"]).map(str::to_string),
        enterprise_id: value_str(data, &["enterpriseId", "enterprise_id"]).map(str::to_string),
    })
}

fn parse_login_account(value: &Value) -> Result<LoginAccount, CodebuddyOAuthError> {
    let data = envelope_data(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 账号响应缺少 data".to_string())
    })?;
    let uid = value_str(data, &["uid"])
        .ok_or_else(|| CodebuddyOAuthError::ParseError("CodeBuddy 账号响应缺少 uid".to_string()))?;
    Ok(LoginAccount {
        uid: uid.to_string(),
        nickname: value_str(data, &["nickname"]).map(str::to_string),
        account_type: value_str(data, &["type"]).map(str::to_string),
        enterprise_id: value_str(data, &["enterpriseId", "enterprise_id"]).map(str::to_string),
        enterprise_name: value_str(data, &["enterpriseName", "enterprise_name"])
            .map(str::to_string),
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CodebuddyJwtClaims {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    iss: Option<String>,
}

/// 只解析 payload，不验证签名；仅用于本地身份/展示。
fn parse_jwt_claims(token: &str) -> Option<CodebuddyJwtClaims> {
    let payload = token.split('.').nth(1)?;
    let payload = payload.trim_end_matches('=');
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// `iss = https://accounts.example.com/oauth/sso-enterprise-1` → `enterprise-1`。
fn enterprise_id_from_issuer(issuer: &str) -> Option<String> {
    let url = url::Url::parse(issuer).ok()?;
    let mut segments = url.path_segments()?;
    let segment = segments.next_back()?.trim();
    segment
        .strip_prefix("sso-")
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn domain_from_issuer(issuer: &str) -> Option<String> {
    url::Url::parse(issuer).ok()?.host_str().map(str::to_string)
}

fn build_account_data(
    token: &TokenBundle,
    account: &LoginAccount,
    now: i64,
) -> CodebuddyAccountData {
    let claims = parse_jwt_claims(&token.access_token).unwrap_or_default();
    let raw_enterprise_id = account.enterprise_id.clone();
    let is_personal = account.account_type.as_deref() == Some("personal");

    // personal 账号必须清空企业上下文，避免把企业 header 带到个人请求里。
    let enterprise_id = if is_personal {
        None
    } else {
        token
            .enterprise_id
            .clone()
            .or_else(|| claims.iss.as_deref().and_then(enterprise_id_from_issuer))
            .or(raw_enterprise_id.clone())
    };
    let domain = token
        .domain
        .clone()
        .or_else(|| claims.iss.as_deref().and_then(domain_from_issuer))
        .filter(|domain| is_safe_domain(domain))
        .unwrap_or_else(|| CODEBUDDY_DOMAIN.to_string());

    let login = account
        .nickname
        .clone()
        .filter(|nickname| !nickname.trim().is_empty())
        .or_else(|| {
            claims
                .nickname
                .clone()
                .filter(|nickname| !nickname.trim().is_empty())
        })
        .or_else(|| {
            claims
                .preferred_username
                .clone()
                .filter(|name| !name.trim().is_empty())
        })
        .or_else(|| claims.name.clone().filter(|name| !name.trim().is_empty()))
        .unwrap_or_else(|| account.uid.clone());
    let email = claims
        .email
        .clone()
        .filter(|email| !email.trim().is_empty());

    CodebuddyAccountData {
        account_id: account_id_for(
            account.account_type.as_deref().unwrap_or_default(),
            raw_enterprise_id.as_deref().unwrap_or_default(),
            &account.uid,
        ),
        uid: account.uid.clone(),
        login: Some(login),
        email,
        enterprise_id,
        enterprise_name: account.enterprise_name.clone(),
        account_type: account.account_type.clone(),
        domain,
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.clone(),
        expires_at: token.expires_at,
        refresh_expires_at: token.refresh_expires_at,
        authenticated_at: now,
        requires_reauth: false,
    }
}

/// `sha256(type + "\0" + enterpriseId + "\0" + uid)` 前 16 个十六进制字符。
fn account_id_for(account_type: &str, enterprise_id: &str, uid: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(account_type.as_bytes());
    hasher.update([0u8]);
    hasher.update(enterprise_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(uid.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest[..16].to_string()
}

/// 把刷新响应合并进旧凭据：refresh token 可能轮换，响应缺省时保留旧值。
fn apply_refresh(existing: &CodebuddyAccountData, refreshed: TokenBundle) -> CodebuddyAccountData {
    let mut updated = existing.clone();
    updated.access_token = refreshed.access_token;
    updated.expires_at = refreshed.expires_at;
    if let Some(refresh_token) = refreshed.refresh_token {
        updated.refresh_token = Some(refresh_token);
    }
    if let Some(refresh_expires_at) = refreshed.refresh_expires_at {
        updated.refresh_expires_at = Some(refresh_expires_at);
    }
    if let Some(domain) = refreshed.domain.filter(|domain| is_safe_domain(domain)) {
        updated.domain = domain;
    }
    if updated.account_type.as_deref() == Some("personal") {
        updated.enterprise_id = None;
    } else if let Some(enterprise_id) = refreshed.enterprise_id {
        updated.enterprise_id = Some(enterprise_id);
    }
    updated.requires_reauth = false;
    updated
}

/// 只有 401/403 才代表 refresh token 失效；400 等其它响应按协议归为 invalid_response。
fn refresh_status_requires_reauth(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    )
}

fn map_business_code(code: i64) -> Option<CodebuddyOAuthError> {
    match code {
        12005 => Some(CodebuddyOAuthError::NoAvailableSeat),
        11212 => Some(CodebuddyOAuthError::LicenseExpired),
        11216 => Some(CodebuddyOAuthError::TrialExpired),
        10081 => Some(CodebuddyOAuthError::IpRestricted),
        _ => None,
    }
}

fn parse_quota_summary(value: &Value) -> Result<Vec<QuotaPackageFact>, CodebuddyOAuthError> {
    let data = envelope_data(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 额度响应缺少 data".to_string())
    })?;
    let packages = match data.get("Packages") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => {
            return Err(CodebuddyOAuthError::ParseError(
                "CodeBuddy 额度响应 Packages 字段格式无效".to_string(),
            ))
        }
    };
    let mut facts = Vec::with_capacity(packages.len());
    for item in packages {
        let code = value_str(item, &["PackageCode"])
            .unwrap_or_default()
            .to_string();
        let total = flexible_number(item.get("CycleTotalCapacity")).unwrap_or(0.0);
        let remaining = flexible_number(item.get("CycleRemainCapacity")).unwrap_or(0.0);
        let used = flexible_number(item.get("CycleUsedCapacity"))
            .unwrap_or_else(|| (total - remaining).max(0.0));
        facts.push(QuotaPackageFact {
            code,
            name: None,
            total,
            remaining,
            used,
            cycle_start: None,
            cycle_end: None,
        });
    }
    Ok(facts)
}

fn parse_free_packages(value: &Value) -> Vec<FreePackageFact> {
    let Some(data) = envelope_data(value) else {
        return Vec::new();
    };
    let Some(Value::Array(accounts)) = data.get("Accounts") else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for item in accounts {
        let Some(code) = value_str(item, &["PackageCode"]) else {
            continue;
        };
        let (name, cycle_end) = (
            value_str(item, &["PackageName"]).map(str::to_string),
            item.get("CycleEndTime").and_then(normalize_cycle_time),
        );
        facts.push(FreePackageFact {
            code: code.to_string(),
            name,
            cycle_end,
        });
    }
    facts
}

/// summary 与 free-packages 按 `PackageCode` 合并；free-packages 只补名称和周期结束时间，
/// 不新增包、不覆盖余额。余额一律以 summary 的包求和为准，
/// free-packages 独有的包不进合计（否则会和 summary 重复计算）。
fn merge_quota_packages(
    summary: Vec<QuotaPackageFact>,
    free: Vec<FreePackageFact>,
) -> Vec<CodebuddyQuotaPackage> {
    let mut facts = summary;
    for free_package in free {
        if let Some(fact) = facts
            .iter_mut()
            .find(|fact| !fact.code.is_empty() && fact.code == free_package.code)
        {
            if fact.name.is_none() {
                fact.name = free_package.name;
            }
            if fact.cycle_end.is_none() {
                fact.cycle_end = free_package.cycle_end;
            }
        }
    }

    let mut packages: Vec<CodebuddyQuotaPackage> = facts
        .into_iter()
        .map(|fact| CodebuddyQuotaPackage {
            name: fact
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| {
                    if fact.code.is_empty() {
                        "额度包".to_string()
                    } else {
                        fact.code.clone()
                    }
                }),
            total: fact.total,
            remaining: fact.remaining,
            used: if fact.used > 0.0 {
                fact.used
            } else {
                (fact.total - fact.remaining).max(0.0)
            },
            cycle_start: fact.cycle_start,
            cycle_end: fact.cycle_end,
        })
        .collect();
    // 有周期结束时间的包排前面，前端用 packages[0].cycle_end 展示重置时间。
    packages.sort_by_key(|package| package.cycle_end.is_none());
    packages
}

fn parse_enterprise_quota(
    value: &Value,
    enterprise_name: Option<&str>,
) -> Result<Vec<CodebuddyQuotaPackage>, CodebuddyOAuthError> {
    let data = envelope_data(value).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 企业额度响应缺少 data".to_string())
    })?;
    let credit = flexible_number(data.get("credit")).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 企业额度响应缺少 credit".to_string())
    })?;
    let limit = flexible_number(data.get("limitNum")).ok_or_else(|| {
        CodebuddyOAuthError::ParseError("CodeBuddy 企业额度响应缺少 limitNum".to_string())
    })?;
    let cycle_start = data.get("cycleStartTime").and_then(normalize_cycle_time);
    let cycle_end = data
        .get("cycleEndTime")
        .and_then(normalize_cycle_time)
        .or_else(|| data.get("cycleResetTime").and_then(normalize_cycle_time));
    Ok(vec![CodebuddyQuotaPackage {
        name: enterprise_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("企业额度")
            .to_string(),
        total: limit,
        remaining: (limit - credit).max(0.0),
        used: credit,
        cycle_start,
        cycle_end,
    }])
}

/// 服务模型前缀：代码补全 / NextEdit / 提示词增强，不能走 chat completions。
const CODEBUDDY_SERVICE_MODEL_PREFIXES: [&str; 3] = ["completion-", "nes-", "enhance-"];

/// `/v3/config` 里的“自动选择”入口（菜单项而非模型，上游会自己挑后端），不发布。
const CODEBUDDY_ROUTING_ALIAS_IDS: [&str; 3] = ["auto", "auto-chat", "default-model"];

fn is_codebuddy_routing_alias(id: &str) -> bool {
    let id = id.trim();
    CODEBUDDY_ROUTING_ALIAS_IDS
        .iter()
        .any(|alias| id.eq_ignore_ascii_case(alias))
}

fn is_codebuddy_chat_model(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && !is_codebuddy_routing_alias(id)
        && !CODEBUDDY_SERVICE_MODEL_PREFIXES
            .iter()
            .any(|prefix| id.starts_with(prefix))
}

fn model_vendor(model: &Value) -> Option<String> {
    model
        .get("vendor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|vendor| !vendor.is_empty())
        .map(ToString::to_string)
}

/// `data.agents[]`（或未登录形态 `data.agent.agents[]`）中 `name == "cli"` 的
/// 模型 ID 列表；元素可能是字符串，也可能是带 `id` 的对象。
fn collect_cli_model_ids(data: &Value) -> Vec<(String, Option<String>)> {
    let agents = data.get("agents").and_then(Value::as_array).or_else(|| {
        data.get("agent")
            .and_then(|agent| agent.get("agents"))
            .and_then(Value::as_array)
    });
    let Some(agents) = agents else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for agent in agents {
        if agent.get("name").and_then(Value::as_str) != Some("cli") {
            continue;
        }
        let Some(models) = agent.get("models").and_then(Value::as_array) else {
            continue;
        };
        for model in models {
            // 对象形态的 `isDefault=true` 是“自动选择”菜单项，不发布。
            if model
                .get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let (id, vendor) = match model {
                Value::String(id) => (id.trim().to_string(), None),
                Value::Object(object) => {
                    let id = object
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    (id, model_vendor(model))
                }
                _ => continue,
            };
            if !id.is_empty() && !is_codebuddy_routing_alias(&id) {
                result.push((id, vendor));
            }
        }
    }
    result
}

/// 解析 `/v3/config` 的 `data`：详情以 `data.models[]` 为准，CLI 可用列表以
/// `data.agents[name=="cli"].models[]` 为准，两者按 id 合并后过滤 + 去重排序。
fn parse_models_catalog(data: &Value) -> Vec<CodebuddyModelInfo> {
    let mut details: HashMap<String, Option<String>> = HashMap::new();
    if let Some(models) = data.get("models").and_then(Value::as_array) {
        for model in models {
            let Some(id) = model
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            // `isDefault=true` 是上游的“自动选择”菜单项，不是可调用模型。
            if model
                .get("isDefault")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || is_codebuddy_routing_alias(id)
            {
                continue;
            }
            details
                .entry(id.to_string())
                .or_insert_with(|| model_vendor(model));
        }
    }

    let cli_models = collect_cli_model_ids(data);
    let mut result: Vec<CodebuddyModelInfo> = if cli_models.is_empty() {
        details
            .into_iter()
            .map(|(id, vendor)| CodebuddyModelInfo { id, vendor })
            .collect()
    } else {
        cli_models
            .into_iter()
            .map(|(id, vendor)| {
                let vendor = details.get(&id).cloned().flatten().or(vendor);
                CodebuddyModelInfo { id, vendor }
            })
            .collect()
    };
    result.retain(|model| is_codebuddy_chat_model(&model.id));
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result.dedup_by(|a, b| a.id == b.id);
    result
}

fn remaining_percent(remaining: f64, total: f64) -> u8 {
    if total <= 0.0 {
        return 0;
    }
    (remaining / total * 100.0).clamp(0.0, 100.0).round() as u8
}

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 生成 32 位 hex 的 trace id 与 16 位 hex 的 span id。
fn next_request_ids() -> (String, String) {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let now = chrono::Utc::now().timestamp_micros();
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{now}-{sequence}").as_bytes())
    );
    (digest[..32].to_string(), digest[32..48].to_string())
}

fn percent_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn is_safe_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 255
        && domain.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '.' || character == '-'
        })
}

fn effective_domain(domain: &str) -> &str {
    let trimmed = domain.trim();
    if is_safe_domain(trimmed) {
        trimmed
    } else {
        CODEBUDDY_DOMAIN
    }
}

fn to_millis(seconds: i64) -> i64 {
    seconds.saturating_mul(1_000)
}

fn apply_login_headers(
    builder: reqwest::RequestBuilder,
    request_id: &str,
    span_id: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut builder = builder
        .header("Accept", "application/json, text/plain, */*")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("X-Domain", CODEBUDDY_DOMAIN)
        .header("X-No-Authorization", "true")
        .header("X-No-User-Id", "true")
        .header("X-No-Enterprise-Id", "true")
        .header("X-No-Department-Info", "true")
        .header("User-Agent", CODEBUDDY_USER_AGENT)
        .header("X-Product", "SaaS")
        .header("X-Request-ID", request_id)
        .header("Origin", CODEBUDDY_ORIGIN)
        .header("Referer", format!("{CODEBUDDY_ORIGIN}/"));
    if let Some(span_id) = span_id {
        builder = builder
            .header("b3", format!("{request_id}-{span_id}-1-"))
            .header("X-B3-TraceId", request_id)
            .header("X-B3-SpanId", span_id)
            .header("X-B3-Sampled", "1");
    }
    builder
}

fn apply_authenticated_headers(
    builder: reqwest::RequestBuilder,
    request_id: &str,
    access_token: &str,
    domain: &str,
    enterprise_id: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut builder = builder
        .header("Accept", "application/json, text/plain, */*")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("X-Domain", domain)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CODEBUDDY_USER_AGENT)
        .header("X-Product", "SaaS")
        .header("X-CodeBuddy-Request", "1")
        .header("X-Request-ID", request_id)
        .header("Origin", CODEBUDDY_ORIGIN)
        .header("Referer", format!("{CODEBUDDY_ORIGIN}/"));
    if let Some(enterprise_id) = enterprise_id.filter(|id| !id.trim().is_empty()) {
        builder = builder
            .header("X-Enterprise-Id", enterprise_id)
            .header("X-Tenant-Id", enterprise_id);
    }
    builder
}

async fn read_json_response(response: reqwest::Response) -> Result<Value, CodebuddyOAuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(CodebuddyOAuthError::ParseError(
            "CodeBuddy 响应超过大小限制".to_string(),
        ));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(CodebuddyOAuthError::ParseError(
            "CodeBuddy 响应超过大小限制".to_string(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| CodebuddyOAuthError::ParseError("CodeBuddy 响应不是有效 JSON".to_string()))
}

fn envelope_data(value: &Value) -> Option<&Value> {
    value.get("data").filter(|data| data.is_object())
}

fn envelope_code(value: &Value) -> Option<i64> {
    flexible_i64(value.get("code")?)
}

/// 数字容错：上游同一字段可能是字符串或数字。
fn flexible_number(value: Option<&Value>) -> Option<f64> {
    let parsed = match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.filter(|number| number.is_finite() && *number >= 0.0)
}

fn flexible_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|number| number as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn flexible_i64_field(data: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| data.get(*key).and_then(flexible_i64))
}

/// `expiresAt` 可能是秒或毫秒，统一归一化为秒。
fn epoch_field(data: &Value, keys: &[&str]) -> Option<i64> {
    flexible_i64_field(data, keys)
        .map(normalize_epoch)
        .filter(|seconds| *seconds > 0)
}

fn normalize_epoch(value: i64) -> i64 {
    if value >= 100_000_000_000 {
        value / 1000
    } else {
        value
    }
}

fn value_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    })
}

/// `CycleEndTime` 可能是 `"YYYY-MM-DD HH:mm:ss"` 或毫秒 epoch。
fn normalize_cycle_time(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Number(number) => {
            let raw = number
                .as_i64()
                .or_else(|| number.as_f64().map(|number| number as i64))?;
            let seconds = normalize_epoch(raw);
            chrono::Local
                .timestamp_opt(seconds, 0)
                .single()
                .map(|datetime| datetime.format("%Y-%m-%d %H:%M:%S").to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unsigned_jwt(payload: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        format!("{header}.{payload}.")
    }

    fn sample_account_data(account_id: &str, authenticated_at: i64) -> CodebuddyAccountData {
        CodebuddyAccountData {
            account_id: account_id.to_string(),
            uid: format!("uid-{account_id}"),
            login: Some(format!("login-{account_id}")),
            email: Some("user@example.com".to_string()),
            enterprise_id: None,
            enterprise_name: None,
            account_type: Some("personal".to_string()),
            domain: CODEBUDDY_DOMAIN.to_string(),
            access_token: "access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: 1_760_000_000,
            refresh_expires_at: None,
            authenticated_at,
            requires_reauth: false,
        }
    }

    #[test]
    fn auth_state_response_parses_and_requires_safe_auth_url() {
        let value = json!({
            "code": 0,
            "data": {
                "state": "2f8a67ea-523c-4264-8213-e0cc9b8ef3b6",
                "authUrl": "https://copilot.tencent.com/login?platform=CLI&state=2f8a67ea-523c-4264-8213-e0cc9b8ef3b6"
            }
        });
        let start = parse_auth_state_response(&value).unwrap();
        assert_eq!(start.state, "2f8a67ea-523c-4264-8213-e0cc9b8ef3b6");
        assert!(start
            .auth_url
            .starts_with("https://copilot.tencent.com/login?platform=CLI"));

        for unsafe_url in [
            "javascript:alert(1)",
            "http://user:pass@copilot.tencent.com/login",
            "https://copilot.tencent.com/login?state=other",
            "https:///login",
            " https://copilot.tencent.com/login",
        ] {
            let value = json!({
                "code": 0,
                "data": {
                    "state": "2f8a67ea-523c-4264-8213-e0cc9b8ef3b6",
                    "authUrl": unsafe_url
                }
            });
            assert!(
                parse_auth_state_response(&value).is_err(),
                "{unsafe_url} 应被拒绝"
            );
        }

        let failed = json!({"code": 11212, "msg": "license expired"});
        assert!(matches!(
            parse_auth_state_response(&failed),
            Err(CodebuddyOAuthError::LicenseExpired)
        ));
    }

    #[test]
    fn pending_token_code_is_not_an_error() {
        let value = json!({"code": 11217, "msg": "11217:login ing..."});
        assert!(token_poll_outcome(&value, 0).unwrap().is_none());
        let account_pending = json!({"code": 12151});
        assert!(account_poll_outcome(&account_pending).unwrap().is_none());
    }

    #[test]
    fn token_response_parses_flexible_fields() {
        let value = json!({
            "code": 0,
            "data": {
                "accessToken": "jwt-token",
                "tokenType": "Bearer",
                "expiresIn": 7200,
                "expiresAt": 1_760_000_000_000i64,
                "refreshToken": "refresh-token",
                "refreshExpiresIn": 86400,
                "domain": "www.codebuddy.cn",
                "enterpriseId": "enterprise-1",
                "scope": "openid"
            }
        });
        let token = token_poll_outcome(&value, 1_700_000_000).unwrap().unwrap();
        assert_eq!(token.access_token, "jwt-token");
        assert_eq!(token.expires_at, 1_760_000_000);
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-token"));
        assert_eq!(token.refresh_expires_at, Some(1_700_000_000 + 86_400));
        assert_eq!(token.domain.as_deref(), Some("www.codebuddy.cn"));
        assert_eq!(token.enterprise_id.as_deref(), Some("enterprise-1"));

        let missing_token = json!({"code": 0, "data": {"expiresIn": 1}});
        assert!(matches!(
            token_poll_outcome(&missing_token, 0),
            Err(CodebuddyOAuthError::TokenFetchFailed(_))
        ));
        let unknown = json!({"code": 12345});
        assert!(matches!(
            token_poll_outcome(&unknown, 0),
            Err(CodebuddyOAuthError::TokenFetchFailed(_))
        ));
    }

    #[test]
    fn login_account_and_jwt_identity_are_merged() {
        let access_token = unsigned_jwt(&json!({
            "sub": "user-sub",
            "email": "alice@example.com",
            "nickname": "JWT 昵称",
            "preferred_username": "alice",
            "iss": "https://accounts.example.com/oauth/sso-enterprise-1"
        }));
        let token = TokenBundle {
            access_token: access_token.clone(),
            expires_at: 1_760_000_000,
            refresh_token: Some("refresh".to_string()),
            refresh_expires_at: None,
            domain: Some("www.codebuddy.cn".to_string()),
            enterprise_id: Some("enterprise-from-token".to_string()),
        };
        let account = parse_login_account(&json!({
            "code": 0,
            "data": {
                "uid": "account-uid",
                "nickname": "账号昵称",
                "type": "ultimate",
                "enterpriseId": "enterprise-from-account",
                "enterpriseName": "Example Corp",
                "departmentFullName": "研发部",
                "pluginEnabled": true
            }
        }))
        .unwrap();
        let data = build_account_data(&token, &account, 1_750_000_000);

        assert_eq!(data.uid, "account-uid");
        assert_eq!(data.login.as_deref(), Some("账号昵称"));
        assert_eq!(data.email.as_deref(), Some("alice@example.com"));
        assert_eq!(data.enterprise_id.as_deref(), Some("enterprise-from-token"));
        assert_eq!(data.enterprise_name.as_deref(), Some("Example Corp"));
        assert_eq!(data.account_type.as_deref(), Some("ultimate"));
        assert_eq!(data.domain, "www.codebuddy.cn");
        assert_eq!(data.authenticated_at, 1_750_000_000);
        assert_eq!(
            data.account_id,
            account_id_for("ultimate", "enterprise-from-account", "account-uid")
        );
        assert_eq!(data.account_id.len(), 16);
        assert_ne!(data.account_id, "account-uid");

        // 昵称优先级：account.nickname 缺失时回落到 JWT nickname。
        let account = parse_login_account(&json!({
            "code": 0,
            "data": {"uid": "account-uid", "type": "ultimate"}
        }))
        .unwrap();
        let fallback = build_account_data(&token, &account, 0);
        assert_eq!(fallback.login.as_deref(), Some("JWT 昵称"));

        // token.domain 缺失时用 JWT iss host。
        let token_without_domain = TokenBundle {
            domain: None,
            ..token
        };
        let from_iss = build_account_data(&token_without_domain, &account, 0);
        assert_eq!(from_iss.domain, "accounts.example.com");
    }

    #[test]
    fn personal_account_clears_enterprise_context() {
        let token = TokenBundle {
            access_token: unsigned_jwt(&json!({
                "iss": "https://accounts.example.com/oauth/sso-enterprise-1"
            })),
            expires_at: 1_760_000_000,
            refresh_token: None,
            refresh_expires_at: None,
            domain: None,
            enterprise_id: Some("enterprise-from-token".to_string()),
        };
        let account = parse_login_account(&json!({
            "code": 0,
            "data": {
                "uid": "account-uid",
                "type": "personal",
                "enterpriseId": "enterprise-from-account"
            }
        }))
        .unwrap();
        let data = build_account_data(&token, &account, 0);
        assert!(data.enterprise_id.is_none());
        // account_id 仍按上游原始账号字段计算，保证刷新后能匹配。
        assert_eq!(
            data.account_id,
            account_id_for("personal", "enterprise-from-account", "account-uid")
        );
    }

    #[test]
    fn refresh_rotation_keeps_or_replaces_refresh_token() {
        let existing = sample_account_data("account-one", 1_700_000_000);

        let kept = apply_refresh(
            &existing,
            TokenBundle {
                access_token: "new-access".to_string(),
                expires_at: 1_760_000_000,
                refresh_token: None,
                refresh_expires_at: None,
                domain: None,
                enterprise_id: None,
            },
        );
        assert_eq!(kept.access_token, "new-access");
        assert_eq!(kept.refresh_token.as_deref(), Some("refresh-token"));
        assert_eq!(kept.domain, existing.domain);

        let rotated = apply_refresh(
            &existing,
            TokenBundle {
                access_token: "new-access".to_string(),
                expires_at: 1_760_000_000,
                refresh_token: Some("rotated-refresh-token".to_string()),
                refresh_expires_at: Some(1_780_000_000),
                domain: Some("www.codebuddy.cn".to_string()),
                enterprise_id: Some("enterprise-2".to_string()),
            },
        );
        assert_eq!(
            rotated.refresh_token.as_deref(),
            Some("rotated-refresh-token")
        );
        assert_eq!(rotated.refresh_expires_at, Some(1_780_000_000));
        assert_eq!(rotated.domain, "www.codebuddy.cn");
        assert!(rotated.enterprise_id.is_none()); // personal 账号保持无企业上下文
    }

    #[test]
    fn refresh_auth_status_is_classified_as_reauth() {
        assert!(refresh_status_requires_reauth(
            reqwest::StatusCode::UNAUTHORIZED
        ));
        assert!(refresh_status_requires_reauth(
            reqwest::StatusCode::FORBIDDEN
        ));
        // 400 + 非 JSON 响应按 invalid_response 处理，不能误标账号失效。
        assert!(!refresh_status_requires_reauth(
            reqwest::StatusCode::BAD_REQUEST
        ));
        assert!(!refresh_status_requires_reauth(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(!refresh_status_requires_reauth(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
    }

    #[test]
    fn quota_summary_parses_flexible_numbers() {
        let value = json!({
            "code": 0,
            "data": {
                "Packages": [
                    {
                        "PackageCode": "TCACA_code_008_cfWoLwvjU4",
                        "CycleTotalCapacity": "500",
                        "CycleRemainCapacity": "500",
                        "CycleUsedCapacity": "0",
                        "CapacityUnit": "credits"
                    },
                    {
                        "PackageCode": "TCACA_code_007_nzdH5h4Nl0",
                        "CycleTotalCapacity": 1500,
                        "CycleRemainCapacity": 840.36000015,
                        "CycleUsedCapacity": "659.63999985",
                        "CapacityUnit": "credits"
                    },
                    {
                        "PackageCode": "TCACA_code_002_AkiJS3ZHF5",
                        "CycleTotalCapacity": 200,
                        "CycleRemainCapacity": 50,
                        "CycleUsedCapacity": "150",
                        "CapacityUnit": "credits"
                    }
                ],
                "IsPaidUser": false
            }
        });
        let facts = parse_quota_summary(&value).unwrap();
        assert_eq!(facts.len(), 3);
        assert_eq!(facts[0].total, 500.0);
        assert_eq!(facts[1].remaining, 840.36000015);
        assert_eq!(facts[2].used, 150.0);
        let quota = CodebuddyQuota::success(merge_quota_packages(facts, Vec::new()), 1_760_000_000);
        assert!(quota.success);
        assert!((quota.total - 2_200.0).abs() < 1e-9);
        assert!((quota.remaining - 1_390.36000015).abs() < 1e-9);
        assert_eq!(quota.remaining_percent, 63);
        assert_eq!(quota.queried_at, Some(1_760_000_000));

        let empty = parse_quota_summary(&json!({"code": 0, "data": {"Packages": null}})).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn free_packages_enrich_summary_without_changing_balance() {
        let summary = parse_quota_summary(&json!({
            "code": 0,
            "data": {"Packages": [
                {"PackageCode": "TCACA_code_008_cfWoLwvjU4", "CycleTotalCapacity": "500", "CycleRemainCapacity": "120"}
            ]}
        }))
        .unwrap();
        let free = parse_free_packages(&json!({
            "code": 0,
            "data": {
                "TotalCount": 2,
                "Accounts": [
                    {
                        "PackageName": "CodeBuddy个人体验版",
                        "PackageCode": "TCACA_code_008_cfWoLwvjU4",
                        "CapacityType": 4,
                        "CycleEndTime": "2026-09-30 23:59:59",
                        "SlicePeriodUsageDetails": [
                            {"SlicePeriodCapacitySizePrecise": "500", "SlicePeriodCapacityRemainPrecise": "120"}
                        ]
                    },
                    {
                        "PackageName": "CodeBuddy活动包",
                        "PackageCode": "TCACA_code_007_nzdH5h4Nl0",
                        "CapacityType": 4,
                        "CycleEndTime": 1_790_000_000_000i64,
                        "SlicePeriodUsageDetails": [
                            {"SlicePeriodCapacitySizePrecise": 100, "SlicePeriodCapacityRemainPrecise": 40}
                        ]
                    }
                ]
            }
        }));
        assert_eq!(free.len(), 2);
        assert_eq!(free[0].name.as_deref(), Some("CodeBuddy个人体验版"));
        assert_eq!(free[0].cycle_end.as_deref(), Some("2026-09-30 23:59:59"));

        let packages = merge_quota_packages(summary, free);
        // free-packages 独有的包不进合计（余额以 summary 求和为准，避免重复计算）。
        assert_eq!(packages.len(), 1);
        let package = &packages[0];
        assert_eq!(package.name, "CodeBuddy个人体验版");
        // summary 的余额不被 slice 覆盖。
        assert_eq!(package.total, 500.0);
        assert_eq!(package.remaining, 120.0);
        assert_eq!(package.used, 380.0);
        assert_eq!(package.cycle_end.as_deref(), Some("2026-09-30 23:59:59"));
    }

    #[test]
    fn enterprise_quota_uses_credit_and_limit() {
        let value = json!({
            "code": 0,
            "data": {
                "credit": 35.5,
                "limitNum": 100,
                "cycleStartTime": "2026-07-01 00:00:00",
                "cycleEndTime": "2026-07-31 23:59:59",
                "cycleResetTime": "2026-08-01 00:00:00"
            }
        });
        let packages = parse_enterprise_quota(&value, Some("Example Corp")).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "Example Corp");
        assert_eq!(packages[0].total, 100.0);
        assert_eq!(packages[0].remaining, 64.5);
        assert_eq!(packages[0].used, 35.5);
        assert_eq!(
            packages[0].cycle_start.as_deref(),
            Some("2026-07-01 00:00:00")
        );
        assert_eq!(
            packages[0].cycle_end.as_deref(),
            Some("2026-07-31 23:59:59")
        );

        assert!(parse_enterprise_quota(&json!({"code": 0, "data": {}}), None).is_err());
    }

    #[test]
    fn remaining_percent_clamps_and_rounds() {
        assert_eq!(remaining_percent(0.0, 0.0), 0);
        assert_eq!(remaining_percent(665.26, 1_000.0), 67);
        assert_eq!(remaining_percent(1_500.0, 1_000.0), 100);
        assert_eq!(remaining_percent(-1.0, 1_000.0), 0);
        assert_eq!(remaining_percent(0.0, 100.0), 0);
    }

    #[test]
    fn business_codes_map_to_expected_errors() {
        for (code, expected) in [
            (11212, "CodeBuddy 许可证已过期"),
            (11216, "CodeBuddy 试用授权已过期"),
            (12005, "企业许可证没有可用席位"),
            (10081, "当前 IP 被访问策略限制"),
        ] {
            let error = map_business_code(code).unwrap().to_string();
            assert!(error.starts_with(expected), "{code}: {error}");
        }
        assert!(map_business_code(0).is_none());
    }

    #[tokio::test]
    async fn account_store_round_trips_and_persists_reauth_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = CodebuddyOAuthManager::new(data_dir.path().to_path_buf());
        manager
            .add_account_internal(sample_account_data("account-one", 10), None)
            .await
            .unwrap();
        manager
            .add_account_internal(sample_account_data("account-two", 20), None)
            .await
            .unwrap();
        assert_eq!(
            manager.default_account_id().await.as_deref(),
            Some("account-one")
        );
        manager.set_default_account("account-two").await.unwrap();

        let reloaded = CodebuddyOAuthManager::new(data_dir.path().to_path_buf());
        let status = reloaded.get_status().await;
        assert_eq!(status.accounts.len(), 2);
        assert_eq!(status.default_account_id.as_deref(), Some("account-two"));
        assert!(status.authenticated);
        let listed = reloaded.list_accounts().await;
        assert_eq!(listed[0].id, "account-two");
        assert_eq!(listed[0].email.as_deref(), Some("user@example.com"));
        assert_eq!(listed[0].github_domain, CODEBUDDY_DOMAIN);
        assert!(status
            .accounts
            .iter()
            .all(|account| !account.requires_reauth));

        reloaded.mark_reauth_required("account-two").await.unwrap();
        let after_reauth = CodebuddyOAuthManager::new(data_dir.path().to_path_buf())
            .get_status()
            .await;
        assert_eq!(
            after_reauth.default_account_id.as_deref(),
            Some("account-one")
        );
        assert!(after_reauth
            .accounts
            .iter()
            .find(|account| account.id == "account-two")
            .is_some_and(|account| account.requires_reauth));
        // 还剩一个可用账号时仍算已认证；全部失效后 authenticated=false，前端
        // 会阻止保存 CodeBuddy 供应商。
        assert!(after_reauth.authenticated);
        reloaded.mark_reauth_required("account-one").await.unwrap();
        let all_reauth = CodebuddyOAuthManager::new(data_dir.path().to_path_buf())
            .get_status()
            .await;
        assert!(!all_reauth.authenticated);
        assert!(all_reauth.default_account_id.is_none());

        // 取消一个不存在的登录 state 返回 false。
        assert!(!reloaded.cancel_auth("missing-state").await);
    }

    #[tokio::test]
    async fn failed_persistence_does_not_commit_account_in_memory() {
        let data_dir = tempfile::tempdir().unwrap();
        let blocker = data_dir.path().join("not-a-directory");
        fs::write(&blocker, b"block").unwrap();
        let manager = CodebuddyOAuthManager::new(blocker);

        let result = manager
            .add_account_internal(sample_account_data("account-one", 10), None)
            .await;
        assert!(matches!(result, Err(CodebuddyOAuthError::IoError(_))));
        assert!(manager.list_accounts().await.is_empty());
    }

    #[tokio::test]
    async fn cancelled_pending_login_cannot_restore_account() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = CodebuddyOAuthManager::new(data_dir.path().to_path_buf());
        let result = manager
            .add_account_internal(
                sample_account_data("account-one", 10),
                Some("cancelled-state"),
            )
            .await;
        assert!(matches!(
            result,
            Err(CodebuddyOAuthError::TokenFetchFailed(_))
        ));
        assert!(manager.list_accounts().await.is_empty());
        assert!(!data_dir.path().join("codebuddy_oauth_auth.json").exists());
    }

    async fn seed_valid_token(manager: &CodebuddyOAuthManager, account_id: &str, token: &str) {
        manager.access_tokens.write().await.insert(
            account_id.to_string(),
            CachedAccessToken {
                token: token.to_string(),
                expires_at_ms: chrono::Utc::now().timestamp_millis() + 3_600_000,
            },
        );
    }

    #[tokio::test]
    async fn request_context_resolves_default_and_explicit_account() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = CodebuddyOAuthManager::new(data_dir.path().to_path_buf());
        manager
            .add_account_internal(sample_account_data("account-one", 10), None)
            .await
            .unwrap();
        let mut second = sample_account_data("account-two", 20);
        second.uid = "uid-two".to_string();
        second.account_type = Some("ultimate".to_string());
        second.enterprise_id = Some("enterprise-1".to_string());
        second.domain = "www.codebuddy.cn".to_string();
        manager.add_account_internal(second, None).await.unwrap();
        manager.set_default_account("account-two").await.unwrap();
        seed_valid_token(&manager, "account-one", "token-one").await;
        seed_valid_token(&manager, "account-two", "token-two").await;

        let default_context = manager.request_context(None).await.unwrap();
        assert_eq!(default_context.account_id, "account-two");
        assert_eq!(default_context.token, "token-two");
        assert_eq!(default_context.uid, "uid-two");
        assert_eq!(default_context.domain, "www.codebuddy.cn");
        assert_eq!(
            default_context.enterprise_id.as_deref(),
            Some("enterprise-1")
        );

        let explicit_context = manager.request_context(Some("account-one")).await.unwrap();
        assert_eq!(explicit_context.account_id, "account-one");
        assert_eq!(explicit_context.token, "token-one");
        assert_eq!(explicit_context.uid, "uid-account-one");
        assert_eq!(explicit_context.domain, CODEBUDDY_DOMAIN);
        assert!(explicit_context.enterprise_id.is_none());
    }

    #[tokio::test]
    async fn request_context_rejects_unknown_and_empty_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = CodebuddyOAuthManager::new(data_dir.path().to_path_buf());
        assert!(matches!(
            manager.request_context(None).await,
            Err(CodebuddyOAuthError::AccountNotFound(_))
        ));
        assert!(matches!(
            manager.request_context(Some("missing")).await,
            Err(CodebuddyOAuthError::AccountNotFound(_))
        ));
    }

    #[test]
    fn models_catalog_parses_flat_models_with_vendor() {
        let catalog = json!({
            "models": [
                { "id": "glm-5.3", "vendor": "Tencent" },
                { "id": "hy4-preview", "vendor": "Tencent" },
                { "id": "", "vendor": "ignored" }
            ]
        });

        let models = parse_models_catalog(&catalog);
        assert_eq!(
            models,
            vec![
                CodebuddyModelInfo {
                    id: "glm-5.3".to_string(),
                    vendor: Some("Tencent".to_string()),
                },
                CodebuddyModelInfo {
                    id: "hy4-preview".to_string(),
                    vendor: Some("Tencent".to_string()),
                },
            ]
        );
    }

    #[test]
    fn models_catalog_prefers_cli_agent_ids_and_backfills_vendor() {
        let catalog = json!({
            "agents": [
                { "name": "ide", "models": ["ide-only"] },
                { "name": "cli", "models": ["glm-5.3", { "id": "hy4-preview" }, "unknown-cli"] }
            ],
            "models": [
                { "id": "glm-5.3", "vendor": "Tencent" },
                { "id": "hy4-preview", "vendor": "Tencent" },
                { "id": "ide-only", "vendor": "Tencent" }
            ]
        });

        let models = parse_models_catalog(&catalog);
        assert_eq!(
            models
                .iter()
                .map(|model| (model.id.as_str(), model.vendor.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("glm-5.3", Some("Tencent")),
                ("hy4-preview", Some("Tencent")),
                ("unknown-cli", None),
            ]
        );
    }

    #[test]
    fn models_catalog_accepts_unlogged_agent_envelope() {
        let catalog = json!({
            "agent": {
                "agents": [{ "name": "cli", "models": ["glm-5.3"] }]
            },
            "models": null
        });

        let models = parse_models_catalog(&catalog);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "glm-5.3");
    }

    #[test]
    fn models_catalog_filters_service_models_dedupes_and_sorts() {
        let catalog = json!({
            "models": [
                { "id": "completion-1", "vendor": "Tencent" },
                { "id": "nes-2", "vendor": "Tencent" },
                { "id": "enhance-3", "vendor": "Tencent" },
                { "id": "zeta", "vendor": "Tencent" },
                { "id": "alpha", "vendor": "Tencent" },
                { "id": "alpha", "vendor": "Tencent" },
                { "id": "  ", "vendor": "Tencent" }
            ]
        });

        let models = parse_models_catalog(&catalog);
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn models_catalog_filters_routing_aliases() {
        let catalog = json!({
            "models": [
                { "id": "glm-5.3", "vendor": "Tencent" },
                { "id": "auto", "isDefault": true, "vendor": "Tencent" },
                { "id": "default-model", "isDefault": true, "vendor": "Tencent" },
                { "id": "auto-chat", "vendor": "Tencent" }
            ]
        });

        let models = parse_models_catalog(&catalog);
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["glm-5.3"]
        );

        let cli_catalog = json!({
            "agents": [{
                "name": "cli",
                "models": [
                    "glm-5.3",
                    "auto",
                    { "id": "default-model", "isDefault": true },
                    { "id": "hy4-preview" }
                ]
            }]
        });
        let models = parse_models_catalog(&cli_catalog);
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["glm-5.3", "hy4-preview"]
        );
    }
}
