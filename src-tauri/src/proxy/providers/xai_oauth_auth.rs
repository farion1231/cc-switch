//! xAI OAuth authentication manager.
//!
//! xAI uses the OAuth 2.0 Device Authorization Grant. Endpoints are resolved
//! from xAI's OpenID Connect discovery document so authentication protocol
//! changes do not require duplicating endpoint constants across the app.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::GitHubDeviceCodeResponse;

const XAI_ISSUER: &str = "https://auth.x.ai";
const XAI_DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const XAI_USER_AGENT: &str = "cc-switch-xai-oauth";
const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;
const DEFAULT_TOKEN_LIFETIME_SECS: i64 = 3_600;
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;
const MAX_DEVICE_CODE_LIFETIME_SECS: u64 = 24 * 60 * 60;
const MAX_POLL_INTERVAL_SECS: u64 = 60;
const MAX_OAUTH_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum XaiOAuthError {
    #[error("等待用户授权中")]
    AuthorizationPending,
    #[error("用户拒绝授权")]
    AccessDenied,
    #[error("Device Code 已过期")]
    ExpiredToken,
    #[error("OAuth Token 获取失败: {0}")]
    TokenFetchFailed(String),
    #[error("Refresh Token 失效或已过期，请重新登录 xAI")]
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

impl From<reqwest::Error> for XaiOAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for XaiOAuthError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    token_endpoint: String,
    device_authorization_endpoint: String,
}

#[derive(Debug, Clone)]
struct OAuthEndpoints {
    token_endpoint: String,
    device_authorization_endpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct XaiTokenClaims {
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
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

#[derive(Debug, Clone)]
struct PendingDeviceCode {
    token_endpoint: String,
    expires_at_ms: i64,
    interval_secs: u64,
    next_poll_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct XaiAccountData {
    account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login: Option<String>,
    refresh_token: String,
    authenticated_at: i64,
    #[serde(default)]
    requires_reauth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaiOAuthAccount {
    pub id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub github_domain: String,
    pub requires_reauth: bool,
}

impl From<&XaiAccountData> for XaiOAuthAccount {
    fn from(data: &XaiAccountData) -> Self {
        let short_id: String = data.account_id.chars().take(12).collect();
        Self {
            id: data.account_id.clone(),
            login: data
                .login
                .clone()
                .unwrap_or_else(|| format!("xAI ({short_id})")),
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: "x.ai".to_string(),
            requires_reauth: data.requires_reauth,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct XaiOAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, XaiAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaiOAuthStatus {
    pub accounts: Vec<XaiOAuthAccount>,
    pub default_account_id: Option<String>,
    pub authenticated: bool,
    pub username: Option<String>,
}

pub struct XaiOAuthManager {
    accounts: Arc<RwLock<HashMap<String, XaiAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    pending_device_codes: Arc<RwLock<HashMap<String, PendingDeviceCode>>>,
    discovered_endpoints: Arc<RwLock<Option<OAuthEndpoints>>>,
    mutation_lock: Arc<Mutex<()>>,
    storage_path: PathBuf,
}

impl XaiOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            pending_device_codes: Arc::new(RwLock::new(HashMap::new())),
            discovered_endpoints: Arc::new(RwLock::new(None)),
            mutation_lock: Arc::new(Mutex::new(())),
            storage_path: data_dir.join("xai_oauth_auth.json"),
        };

        if let Err(error) = manager.load_from_disk_sync() {
            log::warn!("[XaiOAuth] 加载存储失败: {error}");
        }
        manager
    }

    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, XaiOAuthError> {
        let endpoints = self.discover_endpoints().await?;
        let response = crate::proxy::http_client::get()
            .post(&endpoints.device_authorization_endpoint)
            .header("User-Agent", XAI_USER_AGENT)
            .form(&[("client_id", XAI_CLIENT_ID), ("scope", XAI_SCOPE)])
            .send()
            .await?;

        let status = response.status();
        let value = read_json_response(response).await?;
        if !status.is_success() {
            return Err(XaiOAuthError::TokenFetchFailed(format_oauth_error(
                status, &value,
            )));
        }
        let device = parse_device_code_response(value)?;
        let interval = device
            .interval
            .clamp(1, MAX_POLL_INTERVAL_SECS)
            .saturating_add(POLLING_SAFETY_MARGIN_SECS);
        let expires_in = device.expires_in.clamp(1, MAX_DEVICE_CODE_LIFETIME_SECS);
        let now_ms = chrono::Utc::now().timestamp_millis();

        {
            let mut pending = self.pending_device_codes.write().await;
            pending.retain(|_, entry| entry.expires_at_ms > now_ms);
            pending.insert(
                device.device_code.clone(),
                PendingDeviceCode {
                    token_endpoint: endpoints.token_endpoint,
                    expires_at_ms: now_ms.saturating_add(
                        i64::try_from(expires_in)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(1_000),
                    ),
                    interval_secs: interval,
                    next_poll_at_ms: now_ms,
                },
            );
        }

        Ok(GitHubDeviceCodeResponse {
            device_code: device.device_code,
            user_code: device.user_code,
            verification_uri: device
                .verification_uri_complete
                .unwrap_or(device.verification_uri),
            expires_in,
            interval,
        })
    }

    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<XaiOAuthAccount>, XaiOAuthError> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entry = {
            let pending = self.pending_device_codes.read().await;
            pending.get(device_code).cloned()
        }
        .ok_or_else(|| {
            XaiOAuthError::TokenFetchFailed("Device Code 不存在，请重新启动登录".to_string())
        })?;

        if entry.expires_at_ms <= now_ms {
            self.pending_device_codes.write().await.remove(device_code);
            return Err(XaiOAuthError::ExpiredToken);
        }
        if entry.next_poll_at_ms > now_ms {
            return Err(XaiOAuthError::AuthorizationPending);
        }
        self.schedule_next_poll(device_code, entry.interval_secs)
            .await;

        let response = crate::proxy::http_client::get()
            .post(&entry.token_endpoint)
            .header("User-Agent", XAI_USER_AGENT)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", XAI_CLIENT_ID),
                ("device_code", device_code),
            ])
            .send()
            .await?;
        let status = response.status();
        let value = read_json_response(response).await?;

        if let Some(error_code) = oauth_error_code(&value) {
            return match error_code.as_str() {
                "authorization_pending" => Err(XaiOAuthError::AuthorizationPending),
                "slow_down" => {
                    self.increase_poll_interval(device_code).await;
                    Err(XaiOAuthError::AuthorizationPending)
                }
                "access_denied" => {
                    self.pending_device_codes.write().await.remove(device_code);
                    Err(XaiOAuthError::AccessDenied)
                }
                "expired_token" => {
                    self.pending_device_codes.write().await.remove(device_code);
                    Err(XaiOAuthError::ExpiredToken)
                }
                _ => Err(XaiOAuthError::TokenFetchFailed(format_oauth_error(
                    status, &value,
                ))),
            };
        }
        if !status.is_success() {
            return Err(XaiOAuthError::TokenFetchFailed(format_oauth_error(
                status, &value,
            )));
        }

        let tokens = parse_token_response(value)?;
        validate_access_token(&tokens.access_token)?;
        let refresh_token = tokens
            .refresh_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .map(ToString::to_string)
            .ok_or_else(|| {
                XaiOAuthError::TokenFetchFailed("成功响应缺少 refresh_token".to_string())
            })?;
        let (account_id, login) = extract_identity_from_tokens(&tokens).ok_or_else(|| {
            XaiOAuthError::ParseError("xAI token 缺少稳定的 sub claim，未保存账号".to_string())
        })?;

        let cached_access_token = CachedAccessToken {
            token: tokens.access_token,
            expires_at_ms: compute_expires_at_ms(tokens.expires_in),
        };
        let account = self
            .add_account_internal(
                account_id,
                login,
                refresh_token,
                Some(device_code),
                Some(cached_access_token),
            )
            .await?;
        Ok(Some(account))
    }

    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, XaiOAuthError> {
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
            .ok_or_else(|| XaiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(XaiOAuthError::ReauthRequired(account_id.to_string()));
        }

        let tokens = match self.refresh_with_token(&account.refresh_token).await {
            Ok(tokens) => tokens,
            Err(XaiOAuthError::RefreshTokenInvalid) => {
                self.mark_reauth_required(account_id).await?;
                return Err(XaiOAuthError::ReauthRequired(account_id.to_string()));
            }
            Err(error) => return Err(error),
        };

        self.commit_refreshed_tokens(account_id, &account.refresh_token, tokens)
            .await
    }

    pub async fn get_valid_token(&self) -> Result<String, XaiOAuthError> {
        match self.resolve_default_account_id().await {
            Some(account_id) => self.get_valid_token_for_account(&account_id).await,
            None => Err(XaiOAuthError::AccountNotFound(
                "无可用的 xAI 账号，请登录或重新登录".to_string(),
            )),
        }
    }

    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    pub async fn get_status(&self) -> XaiOAuthStatus {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());
        let username = default_account_id
            .as_ref()
            .and_then(|id| accounts.get(id))
            .and_then(|account| account.login.clone());
        XaiOAuthStatus {
            authenticated: default_account_id.is_some(),
            default_account_id,
            accounts: account_list,
            username,
        }
    }

    pub async fn list_accounts(&self) -> Vec<XaiOAuthAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        if accounts.remove(account_id).is_none() {
            return Err(XaiOAuthError::AccountNotFound(account_id.to_string()));
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

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let accounts = self.accounts.read().await.clone();
        let account = accounts
            .get(account_id)
            .ok_or_else(|| XaiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(XaiOAuthError::ReauthRequired(account_id.to_string()));
        }
        self.persist_and_commit(accounts, Some(account_id.to_string()))
            .await
    }

    pub async fn clear_auth(&self) -> Result<(), XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }
        *self.accounts.write().await = HashMap::new();
        *self.default_account_id.write().await = None;
        self.access_tokens.write().await.clear();
        self.refresh_locks.write().await.clear();
        self.pending_device_codes.write().await.clear();
        Ok(())
    }

    async fn discover_endpoints(&self) -> Result<OAuthEndpoints, XaiOAuthError> {
        if let Some(endpoints) = self.discovered_endpoints.read().await.clone() {
            return Ok(endpoints);
        }
        let response = crate::proxy::http_client::get()
            .get(XAI_DISCOVERY_URL)
            .header("User-Agent", XAI_USER_AGENT)
            .send()
            .await?;
        let status = response.status();
        let value = read_json_response(response).await?;
        if !status.is_success() {
            return Err(XaiOAuthError::NetworkError(format!(
                "xAI discovery 请求失败: HTTP {status}"
            )));
        }
        let document = parse_discovery_document(value)?;
        if document.issuer.trim_end_matches('/') != XAI_ISSUER {
            return Err(XaiOAuthError::ParseError(
                "xAI discovery issuer 不匹配".to_string(),
            ));
        }
        validate_xai_endpoint(&document.token_endpoint)?;
        validate_xai_endpoint(&document.device_authorization_endpoint)?;
        let endpoints = OAuthEndpoints {
            token_endpoint: document.token_endpoint,
            device_authorization_endpoint: document.device_authorization_endpoint,
        };
        *self.discovered_endpoints.write().await = Some(endpoints.clone());
        Ok(endpoints)
    }

    async fn refresh_with_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, XaiOAuthError> {
        let endpoints = self.discover_endpoints().await?;
        let response = crate::proxy::http_client::get()
            .post(&endpoints.token_endpoint)
            .header("User-Agent", XAI_USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", XAI_CLIENT_ID),
                ("refresh_token", refresh_token),
                ("scope", XAI_SCOPE),
            ])
            .send()
            .await?;
        let status = response.status();
        let value_result = read_json_response(response).await;
        // Invalid credentials must transition the account to re-auth even when
        // the provider sends an empty, HTML, or otherwise malformed error body.
        if refresh_response_requires_reauth(status, value_result.is_err()) {
            return Err(XaiOAuthError::RefreshTokenInvalid);
        }
        let value = value_result?;
        let error_code = oauth_error_code(&value);
        if matches!(
            error_code.as_deref(),
            Some("invalid_grant" | "invalid_token")
        ) {
            return Err(XaiOAuthError::RefreshTokenInvalid);
        }
        if !status.is_success() || error_code.is_some() {
            return Err(XaiOAuthError::TokenFetchFailed(format_oauth_error(
                status, &value,
            )));
        }
        let tokens = parse_token_response(value)?;
        validate_access_token(&tokens.access_token)?;
        Ok(tokens)
    }

    async fn add_account_internal(
        &self,
        account_id: String,
        login: Option<String>,
        refresh_token: String,
        pending_device_code: Option<&str>,
        cached_access_token: Option<CachedAccessToken>,
    ) -> Result<XaiOAuthAccount, XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if let Some(device_code) = pending_device_code {
            let login_is_pending = self
                .pending_device_codes
                .read()
                .await
                .contains_key(device_code);
            if !login_is_pending {
                return Err(XaiOAuthError::TokenFetchFailed(
                    "登录已取消，请重新启动登录".to_string(),
                ));
            }
        }
        let mut accounts = self.accounts.read().await.clone();
        let data = XaiAccountData {
            account_id: account_id.clone(),
            login,
            refresh_token,
            authenticated_at: chrono::Utc::now().timestamp(),
            requires_reauth: false,
        };
        let account = XaiOAuthAccount::from(&data);
        accounts.insert(account_id.clone(), data);
        let current_default = self.default_account_id.read().await.clone();
        let default_account_id = match current_default {
            Some(id) if Self::is_usable_account(&accounts, &id) => Some(id),
            _ => Some(account_id.clone()),
        };
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        if let Some(access_token) = cached_access_token {
            self.access_tokens
                .write()
                .await
                .insert(account_id, access_token);
        }
        if let Some(device_code) = pending_device_code {
            self.pending_device_codes.write().await.remove(device_code);
        }
        Ok(account)
    }

    async fn commit_refreshed_tokens(
        &self,
        account_id: &str,
        expected_refresh_token: &str,
        tokens: OAuthTokenResponse,
    ) -> Result<String, XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| XaiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(XaiOAuthError::ReauthRequired(account_id.to_string()));
        }
        if account.refresh_token != expected_refresh_token {
            return Err(XaiOAuthError::TokenFetchFailed(
                "账号认证状态已变化，请重试请求".to_string(),
            ));
        }

        let refresh_token_changed = tokens
            .refresh_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .is_some_and(|refresh_token| {
                if refresh_token == account.refresh_token {
                    false
                } else {
                    account.refresh_token = refresh_token.to_string();
                    true
                }
            });
        if refresh_token_changed {
            let default_account_id = self.default_account_id.read().await.clone();
            self.persist_and_commit(accounts, default_account_id)
                .await?;
        }

        let access_token = tokens.access_token;
        self.access_tokens.write().await.insert(
            account_id.to_string(),
            CachedAccessToken {
                token: access_token.clone(),
                expires_at_ms: compute_expires_at_ms(tokens.expires_in),
            },
        );
        Ok(access_token)
    }

    async fn mark_reauth_required(&self, account_id: &str) -> Result<(), XaiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| XaiOAuthError::AccountNotFound(account_id.to_string()))?;
        account.requires_reauth = true;
        let default_account_id = Self::fallback_default_account_id(&accounts);
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens.write().await.remove(account_id);
        Ok(())
    }

    async fn persist_and_commit(
        &self,
        accounts: HashMap<String, XaiAccountData>,
        default_account_id: Option<String>,
    ) -> Result<(), XaiOAuthError> {
        let store = XaiOAuthStore {
            version: 1,
            accounts: accounts.clone(),
            default_account_id: default_account_id.clone(),
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|error| XaiOAuthError::ParseError(error.to_string()))?;
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

    async fn schedule_next_poll(&self, device_code: &str, interval_secs: u64) {
        if let Some(entry) = self.pending_device_codes.write().await.get_mut(device_code) {
            entry.next_poll_at_ms = chrono::Utc::now().timestamp_millis().saturating_add(
                i64::try_from(interval_secs)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(1_000),
            );
        }
    }

    async fn increase_poll_interval(&self, device_code: &str) {
        if let Some(entry) = self.pending_device_codes.write().await.get_mut(device_code) {
            entry.interval_secs = entry
                .interval_secs
                .saturating_add(5)
                .min(MAX_POLL_INTERVAL_SECS + POLLING_SAFETY_MARGIN_SECS);
            entry.next_poll_at_ms = chrono::Utc::now().timestamp_millis().saturating_add(
                i64::try_from(entry.interval_secs)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(1_000),
            );
        }
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

    fn fallback_default_account_id(accounts: &HashMap<String, XaiAccountData>) -> Option<String> {
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

    fn is_usable_account(accounts: &HashMap<String, XaiAccountData>, id: &str) -> bool {
        accounts
            .get(id)
            .is_some_and(|account| !account.requires_reauth)
    }

    fn sorted_accounts(
        accounts: &HashMap<String, XaiAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<XaiOAuthAccount> {
        let mut result: Vec<_> = accounts.values().map(XaiOAuthAccount::from).collect();
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

    fn load_from_disk_sync(&self) -> Result<(), XaiOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)?;
        let store: XaiOAuthStore = serde_json::from_str(&content)
            .map_err(|error| XaiOAuthError::ParseError(error.to_string()))?;
        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }
        Ok(())
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), XaiOAuthError> {
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| XaiOAuthError::IoError("无效的存储路径".to_string()))?;
        fs::create_dir_all(parent)?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| XaiOAuthError::IoError("无效的存储文件名".to_string()))?
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

fn default_poll_interval() -> u64 {
    5
}

fn compute_expires_at_ms(expires_in: Option<i64>) -> i64 {
    chrono::Utc::now().timestamp_millis().saturating_add(
        expires_in
            .unwrap_or(DEFAULT_TOKEN_LIFETIME_SECS)
            .max(1)
            .saturating_mul(1_000),
    )
}

fn validate_access_token(access_token: &str) -> Result<(), XaiOAuthError> {
    if access_token.trim().is_empty() {
        return Err(XaiOAuthError::TokenFetchFailed(
            "成功响应缺少 access_token".to_string(),
        ));
    }
    Ok(())
}

fn parse_device_code_response(
    value: serde_json::Value,
) -> Result<DeviceCodeResponse, XaiOAuthError> {
    serde_json::from_value(value)
        .map_err(|_| XaiOAuthError::ParseError("设备授权响应字段无效".to_string()))
}

fn parse_token_response(value: serde_json::Value) -> Result<OAuthTokenResponse, XaiOAuthError> {
    serde_json::from_value(value)
        .map_err(|_| XaiOAuthError::ParseError("OAuth Token 响应字段无效".to_string()))
}

fn parse_discovery_document(value: serde_json::Value) -> Result<DiscoveryDocument, XaiOAuthError> {
    serde_json::from_value(value)
        .map_err(|_| XaiOAuthError::ParseError("xAI discovery 响应字段无效".to_string()))
}

fn refresh_response_requires_reauth(
    status: reqwest::StatusCode,
    response_body_is_invalid: bool,
) -> bool {
    matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) || (status == reqwest::StatusCode::BAD_REQUEST && response_body_is_invalid)
}

fn parse_jwt_claims(token: &str) -> Option<XaiTokenClaims> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn extract_identity_from_tokens(tokens: &OAuthTokenResponse) -> Option<(String, Option<String>)> {
    let claims = tokens
        .id_token
        .as_deref()
        .and_then(parse_jwt_claims)
        .or_else(|| parse_jwt_claims(&tokens.access_token))?;
    let account_id = claims.sub?.trim().to_string();
    if account_id.is_empty() {
        return None;
    }
    let login = claims
        .email
        .or(claims.preferred_username)
        .or(claims.name)
        .filter(|value| !value.trim().is_empty());
    Some((account_id, login))
}

fn validate_xai_endpoint(value: &str) -> Result<(), XaiOAuthError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| XaiOAuthError::ParseError("xAI 认证端点 URL 无效".to_string()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("auth.x.ai")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(XaiOAuthError::ParseError(
            "xAI discovery 返回了不受信任的认证端点".to_string(),
        ));
    }
    Ok(())
}

async fn read_json_response(
    response: reqwest::Response,
) -> Result<serde_json::Value, XaiOAuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OAUTH_RESPONSE_BYTES as u64)
    {
        return Err(XaiOAuthError::ParseError(
            "OAuth 响应超过大小限制".to_string(),
        ));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_OAUTH_RESPONSE_BYTES {
        return Err(XaiOAuthError::ParseError(
            "OAuth 响应超过大小限制".to_string(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| XaiOAuthError::ParseError("OAuth 响应不是有效 JSON".to_string()))
}

fn oauth_error_code(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_oauth_error_code)
        .filter(|value| !value.is_empty())
}

fn sanitize_oauth_error_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "_.-".contains(*character))
        .take(64)
        .collect()
}

fn format_oauth_error(status: reqwest::StatusCode, value: &serde_json::Value) -> String {
    match oauth_error_code(value) {
        Some(code) => format!("HTTP {status} ({code})"),
        None => format!("HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_jwt(payload: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        format!("{header}.{payload}.")
    }

    #[test]
    fn identity_requires_nonempty_sub() {
        let tokens = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(&serde_json::json!({"email":"a@b.c"}))),
            expires_in: Some(3_600),
        };
        assert!(extract_identity_from_tokens(&tokens).is_none());
    }

    #[test]
    fn identity_uses_sub_without_shared_fallback_id() {
        let tokens = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(
                &serde_json::json!({"sub":"user-123","email":"a@b.c"}),
            )),
            expires_in: Some(3_600),
        };
        assert_eq!(
            extract_identity_from_tokens(&tokens),
            Some(("user-123".to_string(), Some("a@b.c".to_string())))
        );
    }

    #[test]
    fn oauth_error_never_embeds_upstream_body() {
        let value = serde_json::json!({
            "error": "invalid_grant<script>",
            "error_description": "refresh_token=super-secret"
        });
        let message = format_oauth_error(reqwest::StatusCode::BAD_REQUEST, &value);
        assert_eq!(message, "HTTP 400 Bad Request (invalid_grantscript)");
        assert!(!message.contains("super-secret"));
        assert!(!message.contains("refresh_token"));
    }

    #[test]
    fn malformed_token_response_never_embeds_upstream_values() {
        let result = parse_token_response(serde_json::json!({
            "access_token": ["upstream-secret"],
            "refresh_token": "another-secret",
            "expires_in": "refresh_token=third-secret"
        }));
        let error = result.unwrap_err().to_string();
        assert_eq!(error, "解析错误: OAuth Token 响应字段无效");
        assert!(!error.contains("secret"));
        assert!(validate_access_token("  ").is_err());
    }

    #[test]
    fn refresh_auth_status_is_classified_before_body_parsing() {
        assert!(refresh_response_requires_reauth(
            reqwest::StatusCode::UNAUTHORIZED,
            true,
        ));
        assert!(refresh_response_requires_reauth(
            reqwest::StatusCode::FORBIDDEN,
            true,
        ));
        assert!(refresh_response_requires_reauth(
            reqwest::StatusCode::BAD_REQUEST,
            true,
        ));
        assert!(!refresh_response_requires_reauth(
            reqwest::StatusCode::BAD_REQUEST,
            false,
        ));
        assert!(!refresh_response_requires_reauth(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            true,
        ));
        assert!(!refresh_response_requires_reauth(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            true,
        ));
    }

    #[test]
    fn fallback_default_skips_accounts_requiring_reauth() {
        let mut accounts = HashMap::new();
        accounts.insert(
            "invalid".to_string(),
            XaiAccountData {
                account_id: "invalid".to_string(),
                login: None,
                refresh_token: "r1".to_string(),
                authenticated_at: 20,
                requires_reauth: true,
            },
        );
        accounts.insert(
            "valid".to_string(),
            XaiAccountData {
                account_id: "valid".to_string(),
                login: None,
                refresh_token: "r2".to_string(),
                authenticated_at: 10,
                requires_reauth: false,
            },
        );
        assert_eq!(
            XaiOAuthManager::fallback_default_account_id(&accounts),
            Some("valid".to_string())
        );
    }

    #[test]
    fn discovery_endpoints_must_stay_on_xai_origin() {
        assert!(validate_xai_endpoint("https://auth.x.ai/oauth2/token").is_ok());
        assert!(validate_xai_endpoint("http://auth.x.ai/oauth2/token").is_err());
        assert!(validate_xai_endpoint("https://auth.x.ai:8443/oauth2/token").is_err());
        assert!(validate_xai_endpoint("https://user@auth.x.ai/oauth2/token").is_err());
        assert!(validate_xai_endpoint("https://attacker.example/token").is_err());
    }

    #[tokio::test]
    async fn account_store_round_trips_and_persists_reauth_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        manager
            .add_account_internal(
                "account-one".to_string(),
                Some("one@example.com".to_string()),
                "refresh-one".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        manager
            .add_account_internal(
                "account-two".to_string(),
                Some("two@example.com".to_string()),
                "refresh-two".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        manager.set_default_account("account-one").await.unwrap();

        let reloaded = XaiOAuthManager::new(data_dir.path().to_path_buf());
        let status = reloaded.get_status().await;
        assert_eq!(status.accounts.len(), 2);
        assert_eq!(status.default_account_id.as_deref(), Some("account-one"));
        assert!(status
            .accounts
            .iter()
            .all(|account| !account.requires_reauth));

        reloaded.mark_reauth_required("account-one").await.unwrap();
        let after_reauth = XaiOAuthManager::new(data_dir.path().to_path_buf())
            .get_status()
            .await;
        assert_eq!(
            after_reauth.default_account_id.as_deref(),
            Some("account-two")
        );
        assert!(after_reauth
            .accounts
            .iter()
            .find(|account| account.id == "account-one")
            .is_some_and(|account| account.requires_reauth));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(data_dir.path().join("xai_oauth_auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn failed_persistence_does_not_commit_account_in_memory() {
        let data_dir = tempfile::tempdir().unwrap();
        let blocker = data_dir.path().join("not-a-directory");
        fs::write(&blocker, b"block").unwrap();
        let manager = XaiOAuthManager::new(blocker);

        let result = manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-one".to_string(),
                None,
                None,
            )
            .await;
        assert!(matches!(result, Err(XaiOAuthError::IoError(_))));
        assert!(manager.list_accounts().await.is_empty());
    }

    #[tokio::test]
    async fn cached_token_cannot_bypass_account_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        let cached_token = CachedAccessToken {
            token: "cached-access-token".to_string(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 3_600_000,
        };

        manager
            .access_tokens
            .write()
            .await
            .insert("missing-account".to_string(), cached_token.clone());
        assert!(matches!(
            manager.get_valid_token_for_account("missing-account").await,
            Err(XaiOAuthError::AccountNotFound(_))
        ));

        manager
            .add_account_internal(
                "reauth-account".to_string(),
                None,
                "refresh-token".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        manager
            .mark_reauth_required("reauth-account")
            .await
            .unwrap();
        manager
            .access_tokens
            .write()
            .await
            .insert("reauth-account".to_string(), cached_token);
        assert!(matches!(
            manager.get_valid_token_for_account("reauth-account").await,
            Err(XaiOAuthError::ReauthRequired(_))
        ));
    }

    #[tokio::test]
    async fn refresh_commit_cannot_restore_removed_or_replaced_account() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "old-refresh-token".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        manager.remove_account("account-one").await.unwrap();

        let removed_result = manager
            .commit_refreshed_tokens(
                "account-one",
                "old-refresh-token",
                OAuthTokenResponse {
                    access_token: "stale-access-token".to_string(),
                    refresh_token: None,
                    id_token: None,
                    expires_in: Some(3_600),
                },
            )
            .await;
        assert!(matches!(
            removed_result,
            Err(XaiOAuthError::AccountNotFound(_))
        ));
        assert!(!manager
            .access_tokens
            .read()
            .await
            .contains_key("account-one"));

        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "new-refresh-token".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        let replaced_result = manager
            .commit_refreshed_tokens(
                "account-one",
                "old-refresh-token",
                OAuthTokenResponse {
                    access_token: "stale-access-token".to_string(),
                    refresh_token: Some("stale-rotated-token".to_string()),
                    id_token: None,
                    expires_in: Some(3_600),
                },
            )
            .await;
        assert!(matches!(
            replaced_result,
            Err(XaiOAuthError::TokenFetchFailed(_))
        ));
        assert!(!manager
            .access_tokens
            .read()
            .await
            .contains_key("account-one"));
        assert_eq!(
            manager
                .accounts
                .read()
                .await
                .get("account-one")
                .map(|account| account.refresh_token.as_str()),
            Some("new-refresh-token")
        );
    }

    #[tokio::test]
    async fn cancelled_pending_login_cannot_restore_account_or_cache() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        let result = manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-token".to_string(),
                Some("cancelled-device-code"),
                Some(CachedAccessToken {
                    token: "access-token".to_string(),
                    expires_at_ms: chrono::Utc::now().timestamp_millis() + 3_600_000,
                }),
            )
            .await;

        assert!(matches!(result, Err(XaiOAuthError::TokenFetchFailed(_))));
        assert!(manager.list_accounts().await.is_empty());
        assert!(manager.access_tokens.read().await.is_empty());
        assert!(!data_dir.path().join("xai_oauth_auth.json").exists());
    }
}
