//! Kimi OAuth account and token-lifecycle manager.
//!
//! Kimi uses the OAuth 2.0 Device Authorization Grant. Wire I/O is delegated
//! to [`super::kimi_oauth_api::KimiOAuthApiClient`] so orchestration remains
//! deterministic under injected clock and identifier sources.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::copilot_auth::GitHubDeviceCodeResponse;
use super::kimi_oauth_api::{
    KimiClientIdentity, KimiClock, KimiDeviceFacts, KimiDevicePollResult, KimiIdSource,
    KimiModelInfo, KimiOAuthApiClient, KimiTokenBundle, KimiUsageReport, KimiUserProfile,
    SystemKimiClock, UuidKimiIdSource,
};

const MIN_TOKEN_REFRESH_THRESHOLD_SECS: i64 = 300;
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;
const MAX_DEVICE_CODE_LIFETIME_SECS: u64 = 24 * 60 * 60;
const MAX_POLL_INTERVAL_SECS: u64 = 60;

/// Structured failures produced by the Kimi OAuth lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum KimiOAuthError {
    /// The user has not completed the device authorization yet.
    #[error("等待用户授权中")]
    AuthorizationPending,
    /// The user rejected the device authorization request.
    #[error("用户拒绝授权")]
    AccessDenied,
    /// The device code expired before authorization completed.
    #[error("Device Code 已过期")]
    ExpiredToken,
    /// The token endpoint returned a terminal OAuth failure.
    #[error("OAuth Token 获取失败: {0}")]
    TokenFetchFailed(String),
    /// The stored refresh token can no longer mint access tokens.
    #[error("Refresh Token 失效或已过期，请重新登录 Kimi")]
    RefreshTokenInvalid,
    /// The selected account must complete the login flow again.
    #[error("Kimi 账号需要重新登录")]
    ReauthRequired(String),
    /// A network or transport operation failed.
    #[error("网络错误: {0}")]
    NetworkError(String),
    /// An upstream response or persisted value failed validation.
    #[error("解析错误: {0}")]
    ParseError(String),
    /// Reading or atomically persisting account state failed.
    #[error("IO 错误: {0}")]
    IoError(String),
    /// The requested account identifier is not present.
    #[error("Kimi 账号不存在")]
    AccountNotFound(String),
    /// A managed endpoint rejected otherwise fresh credentials.
    #[error("Kimi managed endpoint {0} rejected the OAuth credential")]
    ManagementUnauthorized(&'static str),
    /// A managed endpoint returned a non-authentication HTTP failure.
    #[error("Kimi managed endpoint {endpoint} failed: HTTP {status}")]
    UpstreamRejected { endpoint: &'static str, status: u16 },
}

impl From<reqwest::Error> for KimiOAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for KimiOAuthError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
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

impl From<&KimiTokenBundle> for OAuthTokenResponse {
    fn from(tokens: &KimiTokenBundle) -> Self {
        Self {
            access_token: tokens.access_token.clone(),
            refresh_token: Some(tokens.refresh_token.clone()),
            id_token: tokens.id_token.clone(),
            expires_in: Some(tokens.expires_in),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct KimiTokenClaims {
    #[serde(default)]
    user_id: Option<String>,
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
    refresh_at_ms: i64,
}

impl CachedAccessToken {
    fn should_refresh(&self, now_ms: i64) -> bool {
        now_ms >= self.refresh_at_ms || now_ms >= self.expires_at_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenTiming {
    expires_at_ms: i64,
    refresh_at_ms: i64,
}

#[derive(Debug, Clone)]
struct PendingDeviceCode {
    expires_at_ms: i64,
    interval_secs: u64,
    next_poll_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KimiAccountData {
    account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    refresh_token: String,
    authenticated_at: i64,
    #[serde(default)]
    requires_reauth: bool,
}

/// Secret-free account metadata returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiOAuthAccount {
    /// Stable Kimi user identifier obtained from the authenticated profile.
    pub id: String,
    /// Best available human-readable account label.
    pub login: String,
    /// Optional profile avatar URL.
    pub avatar_url: Option<String>,
    /// Unix timestamp in seconds when the account was authenticated.
    pub authenticated_at: i64,
    /// Compatibility domain displayed by the shared managed-auth UI.
    pub github_domain: String,
    /// Whether the refresh credential was rejected and login is required.
    pub requires_reauth: bool,
}

impl From<&KimiAccountData> for KimiOAuthAccount {
    fn from(data: &KimiAccountData) -> Self {
        let short_id: String = data.account_id.chars().take(12).collect();
        Self {
            id: data.account_id.clone(),
            login: data
                .login
                .clone()
                .unwrap_or_else(|| format!("Kimi ({short_id})")),
            avatar_url: data.avatar_url.clone(),
            authenticated_at: data.authenticated_at,
            github_domain: "kimi.com".to_string(),
            requires_reauth: data.requires_reauth,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct KimiOAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, KimiAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kimi_device_id: Option<String>,
}

/// Aggregate Kimi authentication state returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiOAuthStatus {
    /// Managed accounts in stable presentation order.
    pub accounts: Vec<KimiOAuthAccount>,
    /// Identifier of the selected usable account, when one exists.
    pub default_account_id: Option<String>,
    /// Whether a usable default account is available.
    pub authenticated: bool,
    /// Display label of the default account.
    pub username: Option<String>,
}

/// Coordinates Kimi device login, account persistence, and token refresh.
pub struct KimiOAuthManager {
    accounts: Arc<RwLock<HashMap<String, KimiAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    pending_device_codes: Arc<RwLock<HashMap<String, PendingDeviceCode>>>,
    kimi_device_id: Arc<RwLock<String>>,
    mutation_lock: Arc<Mutex<()>>,
    storage_path: PathBuf,
    api: Arc<KimiOAuthApiClient>,
    clock: Arc<dyn KimiClock>,
    id_source: Arc<dyn KimiIdSource>,
    device_facts: KimiDeviceFacts,
}

/// Secret-bearing inference credentials plus non-secret Kimi identity.
pub(crate) struct KimiAccessContext {
    access_token: String,
    account_id: String,
    identity: KimiClientIdentity,
}

#[derive(Clone, Copy)]
enum KimiManagementOperation {
    Models,
    Usage,
}

enum KimiManagementResponse {
    Models(Vec<KimiModelInfo>),
    Usage(KimiUsageReport),
}

impl KimiAccessContext {
    /// Returns the OAuth access token for `x-api-key` authentication.
    pub(crate) fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Returns the managed account that issued this access token.
    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }

    /// Returns the Kimi Code CLI User-Agent.
    pub(crate) fn user_agent(&self) -> &str {
        self.identity.user_agent()
    }

    /// Returns the complete non-User-Agent `X-Msh-*` identity set.
    pub(crate) fn device_headers(&self) -> Vec<(String, String)> {
        self.identity.device_headers()
    }
}

impl KimiOAuthManager {
    /// Creates the production OAuth manager for one cc-switch data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_dependencies(
            data_dir,
            Arc::new(KimiOAuthApiClient::production()),
            Arc::new(SystemKimiClock),
            Arc::new(UuidKimiIdSource),
            KimiDeviceFacts::from_system(),
        )
    }

    /// Creates a manager with deterministic orchestration dependencies.
    pub(crate) fn with_dependencies(
        data_dir: PathBuf,
        api: Arc<KimiOAuthApiClient>,
        clock: Arc<dyn KimiClock>,
        id_source: Arc<dyn KimiIdSource>,
        device_facts: KimiDeviceFacts,
    ) -> Self {
        let initial_device_id = id_source.next_id();
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            pending_device_codes: Arc::new(RwLock::new(HashMap::new())),
            kimi_device_id: Arc::new(RwLock::new(initial_device_id)),
            mutation_lock: Arc::new(Mutex::new(())),
            storage_path: data_dir.join("kimi_oauth_auth.json"),
            api,
            clock,
            id_source,
            device_facts,
        };

        if let Err(error) = manager.load_from_disk_sync() {
            log::warn!("[KimiOAuth] 加载存储失败: {error}");
        }
        manager
    }

    /// Starts the Kimi device-code login flow.
    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, KimiOAuthError> {
        let identity = self.client_identity().await;
        let device = self.api.request_device_authorization(&identity).await?;
        let interval = device
            .interval
            .clamp(1, MAX_POLL_INTERVAL_SECS)
            .saturating_add(POLLING_SAFETY_MARGIN_SECS);
        let expires_in = device.expires_in.clamp(1, MAX_DEVICE_CODE_LIFETIME_SECS);
        let now_ms = self.clock.now_millis();

        {
            let mut pending = self.pending_device_codes.write().await;
            pending.retain(|_, entry| entry.expires_at_ms > now_ms);
            pending.insert(
                device.device_code.clone(),
                PendingDeviceCode {
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
            verification_uri: if device.verification_uri_complete.is_empty() {
                device.verification_uri
            } else {
                device.verification_uri_complete
            },
            expires_in,
            interval,
        })
    }

    /// Drops a pending device-code login so a cancelled poll cannot persist an account.
    pub async fn cancel_pending_login(&self, device_code: &str) {
        let _mutation_guard = self.mutation_lock.lock().await;
        self.pending_device_codes.write().await.remove(device_code);
    }

    /// Polls once for completion of a previously started device-code flow.
    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<KimiOAuthAccount>, KimiOAuthError> {
        let now_ms = self.clock.now_millis();
        let entry = {
            let pending = self.pending_device_codes.read().await;
            pending.get(device_code).cloned()
        }
        .ok_or_else(|| {
            KimiOAuthError::TokenFetchFailed("Device Code 不存在，请重新启动登录".to_string())
        })?;

        if entry.expires_at_ms <= now_ms {
            self.pending_device_codes.write().await.remove(device_code);
            return Err(KimiOAuthError::ExpiredToken);
        }
        if entry.next_poll_at_ms > now_ms {
            return Err(KimiOAuthError::AuthorizationPending);
        }
        self.schedule_next_poll(device_code, entry.interval_secs)
            .await;

        let identity = self.client_identity().await;
        let tokens = match self.api.poll_device_token(&identity, device_code).await? {
            KimiDevicePollResult::Pending => return Err(KimiOAuthError::AuthorizationPending),
            KimiDevicePollResult::SlowDown => {
                self.increase_poll_interval(device_code).await;
                return Err(KimiOAuthError::AuthorizationPending);
            }
            KimiDevicePollResult::Denied => {
                self.pending_device_codes.write().await.remove(device_code);
                return Err(KimiOAuthError::AccessDenied);
            }
            KimiDevicePollResult::Expired => {
                self.pending_device_codes.write().await.remove(device_code);
                return Err(KimiOAuthError::ExpiredToken);
            }
            KimiDevicePollResult::Success(tokens) => tokens,
        };
        let profile = self
            .api
            .fetch_profile(&tokens.access_token, &identity)
            .await?;
        let token_identity = extract_identity_from_tokens(&OAuthTokenResponse::from(&tokens));
        if token_identity
            .as_ref()
            .is_some_and(|(account_id, _)| account_id != &profile.user_id)
        {
            return Err(KimiOAuthError::ParseError(
                "Kimi profile did not match the authenticated token subject".to_string(),
            ));
        }

        let timing = compute_token_timing(self.clock.now_millis(), tokens.expires_in)?;
        let cached_access_token = CachedAccessToken {
            token: tokens.access_token.clone(),
            expires_at_ms: timing.expires_at_ms,
            refresh_at_ms: timing.refresh_at_ms,
        };
        let account = self
            .add_account_with_profile(
                profile,
                tokens.refresh_token,
                Some(device_code),
                Some(cached_access_token),
            )
            .await?;
        Ok(Some(account))
    }

    /// Returns a fresh access token for a specific managed account.
    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, KimiOAuthError> {
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
            .ok_or_else(|| KimiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
        }

        let tokens = match self.refresh_with_token(&account.refresh_token).await {
            Ok(tokens) => tokens,
            Err(KimiOAuthError::RefreshTokenInvalid) => {
                self.mark_reauth_required_if_token_matches(
                    account_id,
                    None,
                    Some(account.refresh_token.as_str()),
                )
                .await?;
                return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
            }
            Err(error) => return Err(error),
        };

        self.commit_refreshed_tokens(account_id, &account.refresh_token, tokens)
            .await
    }

    /// Returns a fresh access token for the default managed account.
    pub async fn get_valid_token(&self) -> Result<String, KimiOAuthError> {
        match self.resolve_default_account_id().await {
            Some(account_id) => self.get_valid_token_for_account(&account_id).await,
            None => Err(KimiOAuthError::AccountNotFound(
                "无可用的 Kimi 账号，请登录或重新登录".to_string(),
            )),
        }
    }

    /// Returns inference credentials and the stable Kimi CLI device identity.
    pub(crate) async fn get_valid_access_context_for_account(
        &self,
        account_id: &str,
    ) -> Result<KimiAccessContext, KimiOAuthError> {
        let access_token = self.get_valid_token_for_account(account_id).await?;
        Ok(KimiAccessContext {
            access_token,
            account_id: account_id.to_string(),
            identity: self.client_identity().await,
        })
    }

    /// Returns default-account inference credentials and device identity.
    pub(crate) async fn get_valid_access_context(
        &self,
    ) -> Result<KimiAccessContext, KimiOAuthError> {
        let account_id = self.resolve_default_account_id().await.ok_or_else(|| {
            KimiOAuthError::AccountNotFound("无可用的 Kimi 账号，请登录或重新登录".to_string())
        })?;
        self.get_valid_access_context_for_account(&account_id).await
    }

    /// Forces a refresh after the inference endpoint rejects an access token.
    pub(crate) async fn refresh_after_inference_auth_rejection(
        &self,
        account_id: Option<&str>,
        rejected_access_token: Option<&str>,
    ) -> Result<String, KimiOAuthError> {
        let account_id = self.resolve_inference_account_id(account_id).await?;
        self.force_refresh_for_account(&account_id, rejected_access_token)
            .await?;
        Ok(account_id)
    }

    /// Persists a reauthentication requirement after a refreshed token is rejected.
    pub(crate) async fn require_reauthentication_after_inference_rejection(
        &self,
        account_id: &str,
        rejected_access_token: Option<&str>,
    ) -> Result<(), KimiOAuthError> {
        self.mark_reauth_required_if_token_matches(account_id, rejected_access_token, None)
            .await
    }

    /// Fetches the live Anthropic-protocol model catalog for an account.
    pub(crate) async fn fetch_models_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<KimiModelInfo>, KimiOAuthError> {
        match self
            .execute_management(account_id, KimiManagementOperation::Models)
            .await?
        {
            KimiManagementResponse::Models(models) => Ok(models),
            _ => Err(KimiOAuthError::ParseError(
                "Kimi models operation returned an unexpected response".to_string(),
            )),
        }
    }

    /// Fetches live subscription usage for an account.
    pub(crate) async fn fetch_usage_for_account(
        &self,
        account_id: &str,
    ) -> Result<KimiUsageReport, KimiOAuthError> {
        match self
            .execute_management(account_id, KimiManagementOperation::Usage)
            .await?
        {
            KimiManagementResponse::Usage(usage) => Ok(usage),
            _ => Err(KimiOAuthError::ParseError(
                "Kimi usage operation returned an unexpected response".to_string(),
            )),
        }
    }

    /// Returns the injected clock value for result timestamps.
    pub(crate) fn now_millis(&self) -> i64 {
        self.clock.now_millis()
    }

    /// Resolves the currently usable default account identifier.
    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    /// Returns aggregate authentication status for the UI.
    pub async fn get_status(&self) -> KimiOAuthStatus {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());
        let username = default_account_id
            .as_ref()
            .and_then(|id| accounts.get(id))
            .and_then(|account| account.login.clone());
        KimiOAuthStatus {
            authenticated: default_account_id.is_some(),
            default_account_id,
            accounts: account_list,
            username,
        }
    }

    /// Returns all managed accounts in stable presentation order.
    pub async fn list_accounts(&self) -> Vec<KimiOAuthAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    /// Removes one managed account and its in-memory token.
    pub async fn remove_account(&self, account_id: &str) -> Result<(), KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        if accounts.remove(account_id).is_none() {
            return Err(KimiOAuthError::AccountNotFound(account_id.to_string()));
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

    /// Selects the default managed account.
    pub async fn set_default_account(&self, account_id: &str) -> Result<(), KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let accounts = self.accounts.read().await.clone();
        let account = accounts
            .get(account_id)
            .ok_or_else(|| KimiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
        }
        self.persist_and_commit(accounts, Some(account_id.to_string()))
            .await
    }

    /// Removes all managed Kimi authentication state.
    pub async fn clear_auth(&self) -> Result<(), KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        self.persist_and_commit(HashMap::new(), None).await?;
        self.access_tokens.write().await.clear();
        self.refresh_locks.write().await.clear();
        self.pending_device_codes.write().await.clear();
        Ok(())
    }

    async fn refresh_with_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, KimiOAuthError> {
        let identity = self.client_identity().await;
        self.api
            .refresh_access_token(&identity, refresh_token)
            .await
            .map(|tokens| OAuthTokenResponse::from(&tokens))
    }

    async fn client_identity(&self) -> KimiClientIdentity {
        let device_id = self.kimi_device_id.read().await.clone();
        KimiClientIdentity::from_facts(&device_id, self.device_facts.clone())
    }

    async fn execute_management(
        &self,
        account_id: &str,
        operation: KimiManagementOperation,
    ) -> Result<KimiManagementResponse, KimiOAuthError> {
        let token = self.get_valid_token_for_account(account_id).await?;
        match self.call_management(operation, &token).await {
            Err(KimiOAuthError::ManagementUnauthorized(_)) => {
                let refreshed = self
                    .force_refresh_for_account(account_id, Some(&token))
                    .await?;
                match self.call_management(operation, &refreshed).await {
                    Err(KimiOAuthError::ManagementUnauthorized(_)) => {
                        self.mark_reauth_required_if_token_matches(
                            account_id,
                            Some(&refreshed),
                            None,
                        )
                        .await?;
                        Err(KimiOAuthError::ReauthRequired(account_id.to_string()))
                    }
                    result => result,
                }
            }
            result => result,
        }
    }

    async fn call_management(
        &self,
        operation: KimiManagementOperation,
        access_token: &str,
    ) -> Result<KimiManagementResponse, KimiOAuthError> {
        let identity = self.client_identity().await;
        match operation {
            KimiManagementOperation::Models => self
                .api
                .fetch_models(access_token, &identity)
                .await
                .map(KimiManagementResponse::Models),
            KimiManagementOperation::Usage => self
                .api
                .fetch_usage(access_token, &identity)
                .await
                .map(KimiManagementResponse::Usage),
        }
    }

    async fn force_refresh_for_account(
        &self,
        account_id: &str,
        rejected_access_token: Option<&str>,
    ) -> Result<String, KimiOAuthError> {
        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _refresh_guard = refresh_lock.lock().await;
        // A concurrent rejection may have already refreshed this account while
        // this task waited on the per-account lock. Rotating again would
        // discard a live token and burn another refresh grant, so reuse the
        // replacement unless the cache still holds the rejected credential.
        if let Some(current) = self.cached_token_for_usable_account(account_id).await {
            if rejected_access_token != Some(current.as_str()) {
                return Ok(current);
            }
        }
        let account = self
            .accounts
            .read()
            .await
            .get(account_id)
            .cloned()
            .ok_or_else(|| KimiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
        }
        let tokens = match self.refresh_with_token(&account.refresh_token).await {
            Ok(tokens) => tokens,
            Err(KimiOAuthError::RefreshTokenInvalid) => {
                self.mark_reauth_required_if_token_matches(
                    account_id,
                    None,
                    Some(account.refresh_token.as_str()),
                )
                .await?;
                return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
            }
            Err(error) => return Err(error),
        };
        self.commit_refreshed_tokens(account_id, &account.refresh_token, tokens)
            .await
    }

    #[cfg(test)]
    async fn add_account_internal(
        &self,
        account_id: String,
        login: Option<String>,
        refresh_token: String,
        pending_device_code: Option<&str>,
        cached_access_token: Option<CachedAccessToken>,
    ) -> Result<KimiOAuthAccount, KimiOAuthError> {
        self.add_account_data(
            account_id,
            login,
            None,
            refresh_token,
            pending_device_code,
            cached_access_token,
        )
        .await
    }

    async fn add_account_with_profile(
        &self,
        profile: KimiUserProfile,
        refresh_token: String,
        pending_device_code: Option<&str>,
        cached_access_token: Option<CachedAccessToken>,
    ) -> Result<KimiOAuthAccount, KimiOAuthError> {
        self.add_account_data(
            profile.user_id.clone(),
            Some(profile.display_label().to_string()),
            profile.avatar_url,
            refresh_token,
            pending_device_code,
            cached_access_token,
        )
        .await
    }

    async fn add_account_data(
        &self,
        account_id: String,
        login: Option<String>,
        avatar_url: Option<String>,
        refresh_token: String,
        pending_device_code: Option<&str>,
        cached_access_token: Option<CachedAccessToken>,
    ) -> Result<KimiOAuthAccount, KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if let Some(device_code) = pending_device_code {
            let login_is_pending = self
                .pending_device_codes
                .read()
                .await
                .contains_key(device_code);
            if !login_is_pending {
                return Err(KimiOAuthError::TokenFetchFailed(
                    "登录已取消，请重新启动登录".to_string(),
                ));
            }
        }
        let mut accounts = self.accounts.read().await.clone();
        let data = KimiAccountData {
            account_id: account_id.clone(),
            login,
            avatar_url,
            refresh_token,
            authenticated_at: self.clock.now_seconds(),
            requires_reauth: false,
        };
        let account = KimiOAuthAccount::from(&data);
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
    ) -> Result<String, KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let mut accounts = self.accounts.read().await.clone();
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| KimiOAuthError::AccountNotFound(account_id.to_string()))?;
        if account.requires_reauth {
            return Err(KimiOAuthError::ReauthRequired(account_id.to_string()));
        }
        if account.refresh_token != expected_refresh_token {
            return Err(KimiOAuthError::TokenFetchFailed(
                "账号认证状态已变化，请重试请求".to_string(),
            ));
        }

        let refresh_token = tokens
            .refresh_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                KimiOAuthError::ParseError("Kimi OAuth response missing refresh_token".to_string())
            })?;
        let refresh_token_changed = refresh_token != account.refresh_token;
        if refresh_token_changed {
            account.refresh_token = refresh_token.to_string();
        }
        if refresh_token_changed {
            let default_account_id = self.default_account_id.read().await.clone();
            self.persist_and_commit(accounts, default_account_id)
                .await?;
        }

        validate_access_token(&tokens.access_token)?;
        let timing = compute_token_timing(
            self.clock.now_millis(),
            tokens.expires_in.ok_or_else(|| {
                KimiOAuthError::ParseError("Kimi OAuth response missing expires_in".to_string())
            })?,
        )?;
        let access_token = tokens.access_token;
        self.access_tokens.write().await.insert(
            account_id.to_string(),
            CachedAccessToken {
                token: access_token.clone(),
                expires_at_ms: timing.expires_at_ms,
                refresh_at_ms: timing.refresh_at_ms,
            },
        );
        Ok(access_token)
    }

    async fn mark_reauth_required(&self, account_id: &str) -> Result<(), KimiOAuthError> {
        self.mark_reauth_required_if_token_matches(account_id, None, None)
            .await
    }

    async fn mark_reauth_required_if_token_matches(
        &self,
        account_id: &str,
        rejected_access_token: Option<&str>,
        expected_refresh_token: Option<&str>,
    ) -> Result<(), KimiOAuthError> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if let Some(rejected) = rejected_access_token {
            let still_current = self
                .access_tokens
                .read()
                .await
                .get(account_id)
                .is_some_and(|token| token.token == rejected);
            if !still_current {
                return Ok(());
            }
        }
        let mut accounts = self.accounts.read().await.clone();
        if let Some(expected) = expected_refresh_token {
            let still_current = accounts
                .get(account_id)
                .is_some_and(|account| account.refresh_token == expected);
            if !still_current {
                return Ok(());
            }
        }
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| KimiOAuthError::AccountNotFound(account_id.to_string()))?;
        account.requires_reauth = true;
        let stored_default = self.default_account_id.read().await.clone();
        let default_account_id = match stored_default {
            Some(id) if Self::is_usable_account(&accounts, &id) => Some(id),
            _ => Self::fallback_default_account_id(&accounts),
        };
        self.persist_and_commit(accounts, default_account_id)
            .await?;
        self.access_tokens.write().await.remove(account_id);
        Ok(())
    }

    async fn persist_and_commit(
        &self,
        accounts: HashMap<String, KimiAccountData>,
        default_account_id: Option<String>,
    ) -> Result<(), KimiOAuthError> {
        let kimi_device_id = self.kimi_device_id.read().await.clone();
        let store = KimiOAuthStore {
            version: 1,
            accounts: accounts.clone(),
            default_account_id: default_account_id.clone(),
            kimi_device_id: Some(kimi_device_id),
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|error| KimiOAuthError::ParseError(error.to_string()))?;
        self.write_store_atomic(&content)?;
        *self.accounts.write().await = accounts;
        *self.default_account_id.write().await = default_account_id;
        Ok(())
    }

    async fn cached_token(&self, account_id: &str) -> Option<String> {
        let now_ms = self.clock.now_millis();
        self.access_tokens
            .read()
            .await
            .get(account_id)
            .filter(|token| !token.should_refresh(now_ms))
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
            entry.next_poll_at_ms = self.clock.now_millis().saturating_add(
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
            entry.next_poll_at_ms = self.clock.now_millis().saturating_add(
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

    async fn resolve_inference_account_id(
        &self,
        account_id: Option<&str>,
    ) -> Result<String, KimiOAuthError> {
        match account_id {
            Some(account_id) => Ok(account_id.to_string()),
            None => self.resolve_default_account_id().await.ok_or_else(|| {
                KimiOAuthError::AccountNotFound("无可用的 Kimi 账号，请登录或重新登录".to_string())
            }),
        }
    }

    fn fallback_default_account_id(accounts: &HashMap<String, KimiAccountData>) -> Option<String> {
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

    fn is_usable_account(accounts: &HashMap<String, KimiAccountData>, id: &str) -> bool {
        accounts
            .get(id)
            .is_some_and(|account| !account.requires_reauth)
    }

    fn sorted_accounts(
        accounts: &HashMap<String, KimiAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<KimiOAuthAccount> {
        let mut result: Vec<_> = accounts.values().map(KimiOAuthAccount::from).collect();
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

    fn load_from_disk_sync(&self) -> Result<(), KimiOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)?;
        let store: KimiOAuthStore = serde_json::from_str(&content)
            .map_err(|error| KimiOAuthError::ParseError(error.to_string()))?;
        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }
        if let Some(device_id) = store.kimi_device_id.filter(|id| !id.trim().is_empty()) {
            if let Ok(mut stored_device_id) = self.kimi_device_id.try_write() {
                *stored_device_id = device_id;
            }
        }
        Ok(())
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), KimiOAuthError> {
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| KimiOAuthError::IoError("无效的存储路径".to_string()))?;
        fs::create_dir_all(parent)?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| KimiOAuthError::IoError("无效的存储文件名".to_string()))?
            .to_string_lossy();
        let nonce = self.id_source.next_id();
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

fn compute_token_timing(
    issued_at_ms: i64,
    expires_in_secs: i64,
) -> Result<TokenTiming, KimiOAuthError> {
    if expires_in_secs <= 0 {
        return Err(KimiOAuthError::ParseError(
            "Kimi OAuth response missing or invalid expires_in".to_string(),
        ));
    }
    let lifetime_ms = expires_in_secs.saturating_mul(1_000);
    let refresh_threshold_secs = MIN_TOKEN_REFRESH_THRESHOLD_SECS.max(expires_in_secs / 2);
    let expires_at_ms = issued_at_ms.saturating_add(lifetime_ms);
    let refresh_at_ms = expires_at_ms.saturating_sub(refresh_threshold_secs.saturating_mul(1_000));
    Ok(TokenTiming {
        expires_at_ms,
        refresh_at_ms,
    })
}

fn validate_access_token(access_token: &str) -> Result<(), KimiOAuthError> {
    if access_token.trim().is_empty() {
        return Err(KimiOAuthError::TokenFetchFailed(
            "成功响应缺少 access_token".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn parse_token_response(value: serde_json::Value) -> Result<OAuthTokenResponse, KimiOAuthError> {
    serde_json::from_value(value)
        .map_err(|_| KimiOAuthError::ParseError("OAuth Token 响应字段无效".to_string()))
}

#[cfg(test)]
fn refresh_response_requires_reauth(
    status: reqwest::StatusCode,
    response_body_is_invalid: bool,
) -> bool {
    matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) || (status == reqwest::StatusCode::BAD_REQUEST && response_body_is_invalid)
}

fn parse_jwt_claims(token: &str) -> Option<KimiTokenClaims> {
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
    let account_id = claims.user_id.or(claims.sub)?.trim().to_string();
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

#[cfg(test)]
fn oauth_error_code(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(sanitize_oauth_error_code)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
fn sanitize_oauth_error_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "_.-".contains(*character))
        .take(64)
        .collect()
}

#[cfg(test)]
fn format_oauth_error(status: reqwest::StatusCode, value: &serde_json::Value) -> String {
    match oauth_error_code(value) {
        Some(code) => format!("HTTP {status} ({code})"),
        None => format!("HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::kimi_oauth_api::{
        KimiHttpRequest, KimiHttpResponse, KimiHttpTransport, KimiSleeper,
    };
    use super::*;
    use futures::future::BoxFuture;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    const TEST_NOW_MS: i64 = 1_700_000_000_000;

    struct FixedClock;

    impl KimiClock for FixedClock {
        fn now_millis(&self) -> i64 {
            TEST_NOW_MS
        }
    }

    #[derive(Default)]
    struct SequenceIdSource {
        sequence: AtomicUsize,
    }

    impl KimiIdSource for SequenceIdSource {
        fn next_id(&self) -> String {
            format!("test-id-{}", self.sequence.fetch_add(1, Ordering::SeqCst))
        }
    }

    struct NoopSleeper;

    impl KimiSleeper for NoopSleeper {
        fn sleep<'a>(&'a self, _duration: Duration) -> BoxFuture<'a, ()> {
            Box::pin(async {})
        }
    }

    #[derive(Default)]
    struct RecordingTransport {
        requests: StdMutex<Vec<KimiHttpRequest>>,
        responses: StdMutex<VecDeque<Result<KimiHttpResponse, KimiOAuthError>>>,
        on_execute: StdMutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl RecordingTransport {
        fn with_responses(responses: Vec<Result<KimiHttpResponse, KimiOAuthError>>) -> Self {
            Self {
                requests: StdMutex::new(Vec::new()),
                responses: StdMutex::new(responses.into()),
                on_execute: StdMutex::new(None),
            }
        }

        fn set_on_execute(&self, hook: impl Fn() + Send + Sync + 'static) {
            *self.on_execute.lock().unwrap() = Some(Arc::new(hook));
        }
    }

    impl KimiHttpTransport for RecordingTransport {
        fn execute<'a>(
            &'a self,
            request: KimiHttpRequest,
        ) -> BoxFuture<'a, Result<KimiHttpResponse, KimiOAuthError>> {
            Box::pin(async move {
                if let Some(hook) = self.on_execute.lock().unwrap().clone() {
                    hook();
                }
                self.requests.lock().unwrap().push(request);
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("fake response queue exhausted")
            })
        }
    }

    fn test_manager(
        data_dir: PathBuf,
        responses: Vec<Result<KimiHttpResponse, KimiOAuthError>>,
    ) -> (KimiOAuthManager, Arc<RecordingTransport>) {
        let transport = Arc::new(RecordingTransport::with_responses(responses));
        let api = Arc::new(KimiOAuthApiClient::new(
            transport.clone(),
            Arc::new(NoopSleeper),
        ));
        let manager = KimiOAuthManager::with_dependencies(
            data_dir,
            api,
            Arc::new(FixedClock),
            Arc::new(SequenceIdSource::default()),
            KimiDeviceFacts {
                device_name: "test-host".to_string(),
                device_model: "Linux test x64".to_string(),
                os_version: "test".to_string(),
            },
        );
        (manager, transport)
    }

    fn json_response(status: u16, value: serde_json::Value) -> KimiHttpResponse {
        KimiHttpResponse {
            status,
            body: serde_json::to_vec(&value).unwrap(),
        }
    }

    fn cached_access(token: &str) -> CachedAccessToken {
        CachedAccessToken {
            token: token.to_string(),
            expires_at_ms: TEST_NOW_MS + 900_000,
            refresh_at_ms: TEST_NOW_MS + 450_000,
        }
    }

    fn unsigned_jwt(payload: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
        format!("{header}.{payload}.")
    }

    #[test]
    fn identity_requires_nonempty_user_id() {
        let tokens = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(&serde_json::json!({"email":"a@b.c"}))),
            expires_in: Some(900),
        };
        assert!(extract_identity_from_tokens(&tokens).is_none());
    }

    #[test]
    fn identity_uses_user_id_or_subject_but_never_oauth_client_id() {
        let tokens = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(
                &serde_json::json!({"user_id":"user-123","email":"a@b.c"}),
            )),
            expires_in: Some(900),
        };
        assert_eq!(
            extract_identity_from_tokens(&tokens),
            Some(("user-123".to_string(), Some("a@b.c".to_string())))
        );

        let tokens_client = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(
                &serde_json::json!({"client_id":"client-456","email":"a@b.c"}),
            )),
            expires_in: Some(900),
        };
        assert_eq!(extract_identity_from_tokens(&tokens_client), None);

        let tokens_subject = OAuthTokenResponse {
            access_token: "opaque".to_string(),
            refresh_token: Some("refresh".to_string()),
            id_token: Some(unsigned_jwt(
                &serde_json::json!({"sub":"subject-789","preferred_username":"moon"}),
            )),
            expires_in: Some(900),
        };
        assert_eq!(
            extract_identity_from_tokens(&tokens_subject),
            Some(("subject-789".to_string(), Some("moon".to_string())))
        );
    }

    #[test]
    fn token_refresh_threshold_matches_kimi_cli_half_life_with_five_minimum() {
        let issued_at_ms = 1_000_000;
        let regular = compute_token_timing(issued_at_ms, 900).unwrap();
        assert_eq!(regular.expires_at_ms, 1_900_000);
        assert_eq!(regular.refresh_at_ms, 1_450_000);
        assert!(1_449_999 < regular.refresh_at_ms);
        assert!(1_450_000 >= regular.refresh_at_ms);

        let short = compute_token_timing(issued_at_ms, 120).unwrap();
        assert_eq!(short.expires_at_ms, 1_120_000);
        assert!(issued_at_ms >= short.refresh_at_ms);
        assert!(compute_token_timing(issued_at_ms, 0).is_err());
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
    fn account_errors_do_not_render_pseudonymous_identifiers() {
        let account_id = "user-sensitive-123";
        for error in [
            KimiOAuthError::ReauthRequired(account_id.to_string()),
            KimiOAuthError::AccountNotFound(account_id.to_string()),
        ] {
            assert!(!error.to_string().contains(account_id));
        }
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
            KimiAccountData {
                account_id: "invalid".to_string(),
                login: None,
                avatar_url: None,
                refresh_token: "r1".to_string(),
                authenticated_at: 20,
                requires_reauth: true,
            },
        );
        accounts.insert(
            "valid".to_string(),
            KimiAccountData {
                account_id: "valid".to_string(),
                login: None,
                avatar_url: None,
                refresh_token: "r2".to_string(),
                authenticated_at: 10,
                requires_reauth: false,
            },
        );
        assert_eq!(
            KimiOAuthManager::fallback_default_account_id(&accounts),
            Some("valid".to_string())
        );
    }

    #[tokio::test]
    async fn account_store_round_trips_and_persists_reauth_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        let device_id_1 = manager.kimi_device_id.read().await.clone();
        assert!(!device_id_1.is_empty());

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

        let (reloaded, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        let device_id_reloaded = reloaded.kimi_device_id.read().await.clone();
        assert_eq!(device_id_reloaded, device_id_1);

        let status = reloaded.get_status().await;
        assert_eq!(status.accounts.len(), 2);
        assert_eq!(status.default_account_id.as_deref(), Some("account-one"));
        assert_eq!(status.accounts[0].github_domain, "kimi.com");
        assert!(status
            .accounts
            .iter()
            .all(|account| !account.requires_reauth));

        reloaded.mark_reauth_required("account-one").await.unwrap();
        let (after_reauth_manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        let after_reauth = after_reauth_manager.get_status().await;
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
            let mode = fs::metadata(data_dir.path().join("kimi_oauth_auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn rejecting_non_default_account_preserves_usable_default() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        for account_id in ["account-one", "account-two", "account-three"] {
            manager
                .add_account_internal(
                    account_id.to_string(),
                    None,
                    format!("refresh-{account_id}"),
                    None,
                    None,
                )
                .await
                .unwrap();
        }
        manager
            .accounts
            .write()
            .await
            .get_mut("account-three")
            .unwrap()
            .authenticated_at += 1;
        manager.set_default_account("account-one").await.unwrap();

        manager
            .require_reauthentication_after_inference_rejection("account-two", None)
            .await
            .unwrap();

        let status = manager.get_status().await;
        assert_eq!(status.default_account_id.as_deref(), Some("account-one"));
        let (reloaded, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        assert_eq!(
            reloaded.get_status().await.default_account_id.as_deref(),
            Some("account-one")
        );
    }

    #[tokio::test]
    async fn clearing_accounts_preserves_the_stable_device_identity() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-one".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        let device_id = manager.kimi_device_id.read().await.clone();

        manager.clear_auth().await.unwrap();

        assert!(manager.list_accounts().await.is_empty());
        let stored: KimiOAuthStore = serde_json::from_str(
            &fs::read_to_string(data_dir.path().join("kimi_oauth_auth.json")).unwrap(),
        )
        .unwrap();
        assert!(stored.accounts.is_empty());
        assert_eq!(stored.kimi_device_id.as_deref(), Some(device_id.as_str()));
    }

    #[tokio::test]
    async fn failed_persistence_does_not_commit_account_in_memory() {
        let data_dir = tempfile::tempdir().unwrap();
        let blocker = data_dir.path().join("not-a-directory");
        fs::write(&blocker, b"block").unwrap();
        let (manager, _) = test_manager(blocker, Vec::new());

        let result = manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-one".to_string(),
                None,
                None,
            )
            .await;
        assert!(matches!(result, Err(KimiOAuthError::IoError(_))));
        assert!(manager.list_accounts().await.is_empty());
    }

    #[tokio::test]
    async fn cached_token_cannot_bypass_account_state() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        let cached_token = cached_access("cached-access-token");

        manager
            .access_tokens
            .write()
            .await
            .insert("missing-account".to_string(), cached_token.clone());
        assert!(matches!(
            manager.get_valid_token_for_account("missing-account").await,
            Err(KimiOAuthError::AccountNotFound(_))
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
            Err(KimiOAuthError::ReauthRequired(_))
        ));
    }

    #[tokio::test]
    async fn refresh_commit_cannot_restore_removed_or_replaced_account() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
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
                    expires_in: Some(900),
                },
            )
            .await;
        assert!(matches!(
            removed_result,
            Err(KimiOAuthError::AccountNotFound(_))
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
                    expires_in: Some(900),
                },
            )
            .await;
        assert!(matches!(
            replaced_result,
            Err(KimiOAuthError::TokenFetchFailed(_))
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
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        let result = manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-token".to_string(),
                Some("cancelled-device-code"),
                Some(cached_access("access-token")),
            )
            .await;

        assert!(matches!(result, Err(KimiOAuthError::TokenFetchFailed(_))));
        assert!(manager.list_accounts().await.is_empty());
        assert!(manager.access_tokens.read().await.is_empty());
        assert!(!data_dir.path().join("kimi_oauth_auth.json").exists());
    }

    #[tokio::test]
    async fn cancelling_pending_login_drops_device_code_before_account_persist() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                200,
                serde_json::json!({
                    "device_code": "pending-device-code",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://auth.kimi.com/device",
                    "verification_uri_complete": "https://auth.kimi.com/device?code=ABCD-EFGH",
                    "expires_in": 900,
                    "interval": 5
                }),
            ))],
        );

        let device = manager.start_device_flow().await.unwrap();
        manager.cancel_pending_login(&device.device_code).await;

        let poll_result = manager.poll_for_token(&device.device_code).await;
        assert!(matches!(
            poll_result,
            Err(KimiOAuthError::TokenFetchFailed(_))
        ));

        let persist_result = manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-token".to_string(),
                Some(&device.device_code),
                Some(cached_access("access-token")),
            )
            .await;
        assert!(matches!(
            persist_result,
            Err(KimiOAuthError::TokenFetchFailed(_))
        ));
        assert!(manager.list_accounts().await.is_empty());
        assert!(manager.access_tokens.read().await.is_empty());
        assert!(!data_dir.path().join("kimi_oauth_auth.json").exists());
    }

    #[tokio::test]
    async fn management_auth_rejection_forces_one_refresh_and_retries() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![
                Ok(json_response(
                    200,
                    serde_json::json!({
                        "access_token":"first-access",
                        "refresh_token":"rotated-refresh",
                        "expires_in":900
                    }),
                )),
                Ok(json_response(401, serde_json::json!({}))),
                Ok(json_response(
                    200,
                    serde_json::json!({
                        "access_token":"second-access",
                        "refresh_token":"second-refresh",
                        "expires_in":900
                    }),
                )),
                Ok(json_response(
                    200,
                    serde_json::json!({"usage":{"used":"1","limit":"4"}}),
                )),
            ],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                None,
            )
            .await
            .unwrap();

        let usage = manager
            .fetch_usage_for_account("account-one")
            .await
            .unwrap();

        assert_eq!(usage.tiers[0].utilization, 25.0);
        {
            let requests = transport.requests.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert!(requests[2]
                .form
                .iter()
                .any(|(name, value)| name == "refresh_token" && value == "rotated-refresh"));
        }
        assert!(
            !manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
    }

    #[tokio::test]
    async fn repeated_management_auth_rejection_marks_account_for_reauthentication() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![
                Ok(json_response(403, serde_json::json!({}))),
                Ok(json_response(
                    200,
                    serde_json::json!({
                        "access_token":"refreshed-access",
                        "refresh_token":"refreshed-refresh",
                        "expires_in":900
                    }),
                )),
                Ok(json_response(401, serde_json::json!({}))),
            ],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("cached-access")),
            )
            .await
            .unwrap();

        let result = manager.fetch_usage_for_account("account-one").await;

        assert!(matches!(result, Err(KimiOAuthError::ReauthRequired(_))));
        assert_eq!(transport.requests.lock().unwrap().len(), 3);
        assert!(
            manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert!(!manager
            .access_tokens
            .read()
            .await
            .contains_key("account-one"));
    }

    #[tokio::test]
    async fn inference_auth_rejection_forces_refresh_of_a_fresh_cached_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                200,
                serde_json::json!({
                    "access_token":"refreshed-access",
                    "refresh_token":"refreshed-refresh",
                    "expires_in":900
                }),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("rejected-access")),
            )
            .await
            .unwrap();

        let refreshed_account = manager
            .refresh_after_inference_auth_rejection(None, Some("rejected-access"))
            .await
            .expect("upstream rejection should force a token refresh");

        assert_eq!(refreshed_account, "account-one");
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "refreshed-access"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn repeated_inference_auth_rejection_requires_reauthentication() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("rejected-access")),
            )
            .await
            .unwrap();

        manager
            .require_reauthentication_after_inference_rejection(
                "account-one",
                Some("rejected-access"),
            )
            .await
            .expect("repeated rejection should persist reauthentication state");

        assert!(
            manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert!(!manager
            .access_tokens
            .read()
            .await
            .contains_key("account-one"));
    }

    #[tokio::test]
    async fn transient_management_failure_preserves_usable_account_and_cache() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Err(KimiOAuthError::NetworkError(
                "deterministic transport failure".to_string(),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("cached-access")),
            )
            .await
            .unwrap();

        let result = manager.fetch_usage_for_account("account-one").await;

        assert!(matches!(result, Err(KimiOAuthError::NetworkError(_))));
        assert!(
            !manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "cached-access"
        );
    }

    #[tokio::test]
    async fn payment_required_usage_response_preserves_credentials_without_refresh() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                402,
                serde_json::json!({"error":"payment_required"}),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("cached-access")),
            )
            .await
            .unwrap();

        let result = manager.fetch_usage_for_account("account-one").await;

        assert!(matches!(
            result,
            Err(KimiOAuthError::UpstreamRejected { status: 402, .. })
        ));
        assert_eq!(
            transport.requests.lock().unwrap().len(),
            1,
            "a 402 must not trigger a token refresh"
        );
        assert!(
            !manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "cached-access"
        );
    }

    #[tokio::test]
    async fn losing_concurrent_auth_rejection_reuses_the_replacement_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                200,
                serde_json::json!({
                    "access_token":"refreshed-access",
                    "refresh_token":"refreshed-refresh",
                    "expires_in":900
                }),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("rejected-access")),
            )
            .await
            .unwrap();

        // Two concurrent requests both saw "rejected-access" fail; the winner
        // of the refresh-lock race performs the real rotation.
        let winner = manager
            .force_refresh_for_account("account-one", Some("rejected-access"))
            .await
            .unwrap();
        // The loser must reuse the freshly issued replacement instead of
        // rotating it again and burning another refresh grant.
        let loser = manager
            .force_refresh_for_account("account-one", Some("rejected-access"))
            .await
            .unwrap();

        assert_eq!(winner, "refreshed-access");
        assert_eq!(loser, "refreshed-access");
        assert_eq!(
            transport.requests.lock().unwrap().len(),
            1,
            "only the first rejection may perform a real token rotation"
        );
    }

    #[tokio::test]
    async fn delayed_inference_rejection_reuses_already_refreshed_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                200,
                serde_json::json!({
                    "access_token":"refreshed-access",
                    "refresh_token":"refreshed-refresh",
                    "expires_in":900
                }),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("rejected-access")),
            )
            .await
            .unwrap();

        manager
            .refresh_after_inference_auth_rejection(None, Some("rejected-access"))
            .await
            .unwrap();
        manager
            .refresh_after_inference_auth_rejection(None, Some("rejected-access"))
            .await
            .unwrap();

        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "refreshed-access"
        );
        assert_eq!(
            transport.requests.lock().unwrap().len(),
            1,
            "a delayed 401 for the old token must not rotate the replacement"
        );
    }

    #[tokio::test]
    async fn inference_rejection_refreshes_the_account_that_sent_the_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                200,
                serde_json::json!({
                    "access_token":"refreshed-one",
                    "refresh_token":"rotated-one",
                    "expires_in":900
                }),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "refresh-one".to_string(),
                None,
                Some(cached_access("rejected-access")),
            )
            .await
            .unwrap();
        manager
            .add_account_internal(
                "account-two".to_string(),
                None,
                "refresh-two".to_string(),
                None,
                Some(cached_access("other-access")),
            )
            .await
            .unwrap();
        manager.set_default_account("account-two").await.unwrap();

        let refreshed = manager
            .refresh_after_inference_auth_rejection(Some("account-one"), Some("rejected-access"))
            .await
            .unwrap();

        assert_eq!(refreshed, "account-one");
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "refreshed-one"
        );
        assert_eq!(
            manager.access_tokens.read().await["account-two"].token,
            "other-access"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn stale_retry_rejection_does_not_invalidate_a_newer_cached_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(data_dir.path().to_path_buf(), Vec::new());
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("newer-access")),
            )
            .await
            .unwrap();

        manager
            .require_reauthentication_after_inference_rejection(
                "account-one",
                Some("stale-retry-access"),
            )
            .await
            .unwrap();

        assert!(
            !manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "newer-access"
        );
    }

    #[tokio::test]
    async fn stale_management_retry_rejection_does_not_invalidate_a_newer_cached_token() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![
                Ok(json_response(403, serde_json::json!({}))),
                Ok(json_response(
                    200,
                    serde_json::json!({
                        "access_token":"refreshed-access",
                        "refresh_token":"refreshed-refresh",
                        "expires_in":900
                    }),
                )),
                Ok(json_response(401, serde_json::json!({}))),
            ],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "initial-refresh".to_string(),
                None,
                Some(cached_access("cached-access")),
            )
            .await
            .unwrap();

        let tokens = Arc::clone(&manager.access_tokens);
        let calls = Arc::new(AtomicUsize::new(0));
        transport.set_on_execute({
            let tokens = Arc::clone(&tokens);
            let calls = Arc::clone(&calls);
            move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 2 {
                    tokens
                        .try_write()
                        .expect("token cache should be uncontended during the retry")
                        .insert("account-one".to_string(), cached_access("newer-access"));
                }
            }
        });

        let result = manager.fetch_usage_for_account("account-one").await;

        assert!(matches!(result, Err(KimiOAuthError::ReauthRequired(_))));
        assert!(
            !manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "newer-access"
        );
    }

    #[tokio::test]
    async fn invalid_refresh_grant_marks_the_current_account_for_reauthentication() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, _) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                401,
                serde_json::json!({"error":"invalid_grant"}),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "current-grant".to_string(),
                None,
                None,
            )
            .await
            .unwrap();

        let result = manager.get_valid_token_for_account("account-one").await;

        assert!(matches!(result, Err(KimiOAuthError::ReauthRequired(_))));
        assert!(
            manager
                .accounts
                .read()
                .await
                .get("account-one")
                .unwrap()
                .requires_reauth
        );
        assert!(!manager
            .access_tokens
            .read()
            .await
            .contains_key("account-one"));
    }

    #[tokio::test]
    async fn stale_refresh_failure_does_not_expire_a_replaced_grant() {
        let data_dir = tempfile::tempdir().unwrap();
        let (manager, transport) = test_manager(
            data_dir.path().to_path_buf(),
            vec![Ok(json_response(
                401,
                serde_json::json!({"error":"invalid_grant"}),
            ))],
        );
        manager
            .add_account_internal(
                "account-one".to_string(),
                None,
                "old-grant".to_string(),
                None,
                None,
            )
            .await
            .unwrap();

        let accounts = Arc::clone(&manager.accounts);
        let tokens = Arc::clone(&manager.access_tokens);
        transport.set_on_execute(move || {
            accounts
                .try_write()
                .expect("account store should be uncontended during refresh")
                .get_mut("account-one")
                .expect("account exists")
                .refresh_token = "new-grant".to_string();
            tokens
                .try_write()
                .expect("token cache should be uncontended during refresh")
                .insert("account-one".to_string(), cached_access("new-access"));
        });

        let result = manager.get_valid_token_for_account("account-one").await;

        assert!(matches!(result, Err(KimiOAuthError::ReauthRequired(_))));
        let account = manager.accounts.read().await["account-one"].clone();
        assert!(!account.requires_reauth);
        assert_eq!(account.refresh_token, "new-grant");
        assert_eq!(
            manager.access_tokens.read().await["account-one"].token,
            "new-access"
        );
    }
}
