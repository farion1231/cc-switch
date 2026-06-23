//! xAI Grok OAuth Authentication Module
//!
//! Manages local xAI OAuth account state for the `xai_oauth` provider. The
//! provider uses managed account auth: provider rows bind to an account id and
//! the proxy resolves a fresh bearer token only when forwarding to xAI.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use url::Url;

use super::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};

const XAI_CLIENT_ID: &str = "grok-build";
const XAI_AUTH_URL: &str = "https://accounts.x.ai/oauth/authorize";
const XAI_TOKEN_URL: &str = "https://accounts.x.ai/oauth/token";
const XAI_REDIRECT_URI: &str = "http://127.0.0.1:56121/callback";
const XAI_SCOPE: &str = "openid profile email offline_access";
const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;
const DEFAULT_LOGIN_EXPIRES_IN: u64 = 300;
const DEFAULT_LOGIN_INTERVAL: u64 = 3;
const XAI_USER_AGENT: &str = "cc-switch-xai-oauth";

#[derive(Debug, thiserror::Error)]
pub enum XaiOAuthError {
    #[error("Waiting for xAI authorization")]
    AuthorizationPending,

    #[error("xAI OAuth authorization expired")]
    ExpiredToken,

    #[error("xAI OAuth access was denied")]
    AccessDenied,

    #[error("xAI OAuth token request failed: {0}")]
    TokenFetchFailed(String),

    #[error("xAI OAuth refresh token is invalid or expired")]
    RefreshTokenInvalid,

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("xAI OAuth account not found: {0}")]
    AccountNotFound(String),
}

impl From<reqwest::Error> for XaiOAuthError {
    fn from(err: reqwest::Error) -> Self {
        XaiOAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for XaiOAuthError {
    fn from(err: std::io::Error) -> Self {
        XaiOAuthError::IoError(err.to_string())
    }
}

#[derive(Clone, Deserialize)]
struct XaiOAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

impl fmt::Debug for XaiOAuthTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XaiOAuthTokenResponse")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct XaiAccountData {
    account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    refresh_token: String,
    authenticated_at: i64,
}

impl fmt::Debug for XaiAccountData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XaiAccountData")
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .field("refresh_token", &"<redacted>")
            .field("authenticated_at", &self.authenticated_at)
            .finish()
    }
}

impl From<&XaiAccountData> for GitHubAccount {
    fn from(data: &XaiAccountData) -> Self {
        GitHubAccount {
            id: data.account_id.clone(),
            login: data
                .email
                .clone()
                .unwrap_or_else(|| format!("xAI ({})", data.account_id)),
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: "x.ai".to_string(),
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
struct PendingXaiLogin {
    authorization_url: String,
    code_verifier: String,
    expires_at_ms: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct XaiIdClaims {
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

pub struct XaiOAuthManager {
    accounts: Arc<RwLock<HashMap<String, XaiAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    pending_logins: Arc<RwLock<HashMap<String, PendingXaiLogin>>>,
    http_client: Client,
    storage_path: PathBuf,
}

impl XaiOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("xai_oauth_auth.json");
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            pending_logins: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path,
        };

        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[XaiOAuth] failed to load store: {e}");
        }

        manager
    }

    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, XaiOAuthError> {
        let state = generate_pkce_value("state");
        let code_verifier = generate_pkce_value("verifier");
        let code_challenge = pkce_challenge(&code_verifier);
        let authorization_url = build_authorization_url(&state, &code_challenge)?;
        let expires_at_ms =
            chrono::Utc::now().timestamp_millis() + (DEFAULT_LOGIN_EXPIRES_IN as i64) * 1000;

        {
            let mut pending = self.pending_logins.write().await;
            let now_ms = chrono::Utc::now().timestamp_millis();
            pending.retain(|_, entry| entry.expires_at_ms > now_ms);
            pending.insert(
                state.clone(),
                PendingXaiLogin {
                    authorization_url: authorization_url.clone(),
                    code_verifier,
                    expires_at_ms,
                },
            );
        }

        Ok(GitHubDeviceCodeResponse {
            device_code: state,
            user_code: "browser-oauth".to_string(),
            verification_uri: authorization_url,
            expires_in: DEFAULT_LOGIN_EXPIRES_IN,
            interval: DEFAULT_LOGIN_INTERVAL,
        })
    }

    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<GitHubAccount>, XaiOAuthError> {
        let pending = {
            let pending = self.pending_logins.read().await;
            pending.get(device_code).cloned()
        };

        let Some(entry) = pending else {
            return Err(XaiOAuthError::ExpiredToken);
        };

        if entry.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            let mut pending = self.pending_logins.write().await;
            pending.remove(device_code);
            return Err(XaiOAuthError::ExpiredToken);
        }

        let _ = (&entry.authorization_url, &entry.code_verifier);
        Err(XaiOAuthError::AuthorizationPending)
    }

    pub async fn complete_authorization_code(
        &self,
        state: &str,
        code: &str,
    ) -> Result<GitHubAccount, XaiOAuthError> {
        let pending = {
            let mut pending = self.pending_logins.write().await;
            pending.remove(state)
        }
        .ok_or(XaiOAuthError::ExpiredToken)?;

        let tokens = self
            .exchange_code_for_tokens(code, &pending.code_verifier)
            .await?;
        self.persist_tokens(tokens).await
    }

    async fn exchange_code_for_tokens(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<XaiOAuthTokenResponse, XaiOAuthError> {
        let response = self
            .http_client
            .post(XAI_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", XAI_USER_AGENT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", XAI_CLIENT_ID),
                ("code", code),
                ("code_verifier", code_verifier),
                ("redirect_uri", XAI_REDIRECT_URI),
            ])
            .send()
            .await?;

        self.parse_token_response(response).await
    }

    async fn refresh_with_token(
        &self,
        refresh_token: &str,
    ) -> Result<XaiOAuthTokenResponse, XaiOAuthError> {
        let response = self
            .http_client
            .post(XAI_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", XAI_USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", XAI_CLIENT_ID),
                ("refresh_token", refresh_token),
                ("scope", XAI_SCOPE),
            ])
            .send()
            .await?;

        self.parse_token_response(response).await
    }

    async fn parse_token_response(
        &self,
        response: reqwest::Response,
    ) -> Result<XaiOAuthTokenResponse, XaiOAuthError> {
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(XaiOAuthError::RefreshTokenInvalid);
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(XaiOAuthError::TokenFetchFailed(format!(
                "{status} - {text}"
            )));
        }
        response
            .json()
            .await
            .map_err(|e| XaiOAuthError::ParseError(e.to_string()))
    }

    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, XaiOAuthError> {
        {
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.token.clone());
                }
            }
        }

        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;

        {
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.token.clone());
                }
            }
        }

        let refresh_token = {
            let accounts = self.accounts.read().await;
            accounts
                .get(account_id)
                .map(|account| account.refresh_token.clone())
                .ok_or_else(|| XaiOAuthError::AccountNotFound(account_id.to_string()))?
        };

        let tokens = self.refresh_with_token(&refresh_token).await?;
        let access_token = tokens.access_token.clone();

        if let Some(new_refresh) = tokens.refresh_token.clone() {
            if new_refresh != refresh_token {
                let mut accounts = self.accounts.write().await;
                if let Some(account) = accounts.get_mut(account_id) {
                    account.refresh_token = new_refresh;
                }
                drop(accounts);
                self.save_to_disk().await?;
            }
        }

        {
            let mut cache = self.access_tokens.write().await;
            cache.insert(
                account_id.to_string(),
                CachedAccessToken {
                    token: access_token.clone(),
                    expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                },
            );
        }

        Ok(access_token)
    }

    pub async fn get_valid_token(&self) -> Result<String, XaiOAuthError> {
        match self.default_account_id().await {
            Some(id) => self.get_valid_token_for_account(&id).await,
            None => Err(XaiOAuthError::AccountNotFound(
                "no xAI OAuth account available".to_string(),
            )),
        }
    }

    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_id.as_deref())
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), XaiOAuthError> {
        {
            let mut accounts = self.accounts.write().await;
            if accounts.remove(account_id).is_none() {
                return Err(XaiOAuthError::AccountNotFound(account_id.to_string()));
            }
        }
        self.access_tokens.write().await.remove(account_id);
        self.refresh_locks.write().await.remove(account_id);

        {
            let accounts = self.accounts.read().await;
            let mut default = self.default_account_id.write().await;
            if default.as_deref() == Some(account_id) {
                *default = Self::fallback_default_account_id(&accounts);
            }
        }

        self.save_to_disk().await
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), XaiOAuthError> {
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(XaiOAuthError::AccountNotFound(account_id.to_string()));
            }
        }
        *self.default_account_id.write().await = Some(account_id.to_string());
        self.save_to_disk().await
    }

    pub async fn clear_auth(&self) -> Result<(), XaiOAuthError> {
        self.accounts.write().await.clear();
        *self.default_account_id.write().await = None;
        self.access_tokens.write().await.clear();
        self.refresh_locks.write().await.clear();
        self.pending_logins.write().await.clear();

        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }

        Ok(())
    }

    pub async fn is_authenticated(&self) -> bool {
        !self.accounts.read().await.is_empty()
    }

    pub async fn get_status(&self) -> XaiOAuthStatus {
        let accounts_map = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        let accounts = Self::sorted_accounts(&accounts_map, default_id.as_deref());
        let authenticated = !accounts.is_empty();
        let username = default_id
            .as_ref()
            .and_then(|id| accounts_map.get(id))
            .and_then(|account| account.email.clone())
            .or_else(|| accounts.first().map(|account| account.login.clone()));

        XaiOAuthStatus {
            accounts,
            default_account_id: default_id,
            authenticated,
            username,
        }
    }

    async fn persist_tokens(
        &self,
        tokens: XaiOAuthTokenResponse,
    ) -> Result<GitHubAccount, XaiOAuthError> {
        let refresh_token = tokens.refresh_token.clone().ok_or_else(|| {
            XaiOAuthError::TokenFetchFailed("xAI token response omitted refresh_token".to_string())
        })?;
        let (account_id, email) = extract_identity_from_tokens(&tokens);
        let account_id = account_id.ok_or_else(|| {
            XaiOAuthError::ParseError("unable to identify xAI OAuth account".to_string())
        })?;

        self.access_tokens.write().await.insert(
            account_id.clone(),
            CachedAccessToken {
                token: tokens.access_token,
                expires_at_ms: compute_expires_at_ms(tokens.expires_in),
            },
        );

        self.add_account_internal(account_id, refresh_token, email)
            .await
    }

    async fn add_account_internal(
        &self,
        account_id: String,
        refresh_token: String,
        email: Option<String>,
    ) -> Result<GitHubAccount, XaiOAuthError> {
        let data = XaiAccountData {
            account_id: account_id.clone(),
            email,
            refresh_token,
            authenticated_at: chrono::Utc::now().timestamp(),
        };
        let account = GitHubAccount::from(&data);

        self.accounts.write().await.insert(account_id.clone(), data);
        {
            let mut default = self.default_account_id.write().await;
            if default.is_none() {
                *default = Some(account_id);
            }
        }

        self.save_to_disk().await?;
        Ok(account)
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

    fn fallback_default_account_id(accounts: &HashMap<String, XaiAccountData>) -> Option<String> {
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
        accounts: &HashMap<String, XaiAccountData>,
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

    fn write_store_atomic(&self, content: &str) -> Result<(), XaiOAuthError> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| XaiOAuthError::IoError("invalid xAI store path".to_string()))?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| XaiOAuthError::IoError("invalid xAI store filename".to_string()))?
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

    fn load_from_disk_sync(&self) -> Result<(), XaiOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)?;
        let store: XaiOAuthStore =
            serde_json::from_str(&content).map_err(|e| XaiOAuthError::ParseError(e.to_string()))?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
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

    async fn save_to_disk(&self) -> Result<(), XaiOAuthError> {
        let store = XaiOAuthStore {
            version: 1,
            accounts: self.accounts.read().await.clone(),
            default_account_id: self.resolve_default_account_id().await,
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|e| XaiOAuthError::ParseError(e.to_string()))?;
        self.write_store_atomic(&content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaiOAuthStatus {
    pub accounts: Vec<GitHubAccount>,
    pub default_account_id: Option<String>,
    pub authenticated: bool,
    pub username: Option<String>,
}

fn compute_expires_at_ms(expires_in: Option<i64>) -> i64 {
    chrono::Utc::now().timestamp_millis() + expires_in.unwrap_or(3600) * 1000
}

fn generate_pkce_value(prefix: &str) -> String {
    let seed = format!(
        "{prefix}-{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn build_authorization_url(state: &str, code_challenge: &str) -> Result<String, XaiOAuthError> {
    let mut url = Url::parse(XAI_AUTH_URL).map_err(|e| XaiOAuthError::ParseError(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", XAI_CLIENT_ID)
        .append_pair("redirect_uri", XAI_REDIRECT_URI)
        .append_pair("scope", XAI_SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.to_string())
}

fn parse_jwt_claims(token: &str) -> Option<XaiIdClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn extract_identity_from_tokens(
    tokens: &XaiOAuthTokenResponse,
) -> (Option<String>, Option<String>) {
    let claims = tokens
        .id_token
        .as_deref()
        .and_then(parse_jwt_claims)
        .or_else(|| parse_jwt_claims(&tokens.access_token));
    let account_id = claims
        .as_ref()
        .and_then(|claims| claims.sub.clone())
        .or_else(|| Some("xai-default".to_string()));
    let email = claims.and_then(|claims| claims.email);
    (account_id, email)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_jwt(payload: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        format!("{header}.{payload}.")
    }

    #[test]
    fn xai_oauth_pkce_challenge_is_stable_and_url_safe() {
        let challenge = pkce_challenge("verifier");
        assert_eq!(challenge, pkce_challenge("verifier"));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));
    }

    #[test]
    fn xai_oauth_authorization_url_contains_loopback_pkce_contract() {
        let url = build_authorization_url("state-123", "challenge-456").unwrap();
        assert!(url.starts_with("https://accounts.x.ai/oauth/authorize?"));
        assert!(url.contains("client_id=grok-build"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A56121%2Fcallback"));
        assert!(url.contains("code_challenge=challenge-456"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn xai_oauth_cached_token_refresh_needed_inside_buffer() {
        let now = chrono::Utc::now().timestamp_millis();
        let expiring = CachedAccessToken {
            token: "fake-access".to_string(),
            expires_at_ms: now + 30_000,
        };
        assert!(expiring.is_expiring_soon());

        let valid = CachedAccessToken {
            token: "fake-access".to_string(),
            expires_at_ms: now + 3_600_000,
        };
        assert!(!valid.is_expiring_soon());
    }

    #[test]
    fn xai_oauth_token_response_debug_redacts_secrets() {
        let response = XaiOAuthTokenResponse {
            access_token: "access-secret".to_string(),
            refresh_token: Some("refresh-secret".to_string()),
            id_token: Some("id-secret".to_string()),
            expires_in: Some(3600),
        };
        let rendered = format!("{response:?}");
        assert!(!rendered.contains("access-secret"));
        assert!(!rendered.contains("refresh-secret"));
        assert!(!rendered.contains("id-secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn xai_oauth_account_debug_redacts_refresh_token() {
        let account = XaiAccountData {
            account_id: "sub-123".to_string(),
            email: Some("user@example.com".to_string()),
            refresh_token: "refresh-secret".to_string(),
            authenticated_at: 1,
        };
        let rendered = format!("{account:?}");
        assert!(!rendered.contains("refresh-secret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn xai_oauth_extract_identity_from_id_token() {
        let id_token = fake_jwt(r#"{"sub":"sub-123","email":"user@example.com"}"#);
        let tokens = XaiOAuthTokenResponse {
            access_token: "fake-access".to_string(),
            refresh_token: Some("fake-refresh".to_string()),
            id_token: Some(id_token),
            expires_in: Some(3600),
        };
        let (account_id, email) = extract_identity_from_tokens(&tokens);
        assert_eq!(account_id.as_deref(), Some("sub-123"));
        assert_eq!(email.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn xai_oauth_manager_initial_state() {
        let temp = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(temp.path().to_path_buf());
        assert!(!manager.is_authenticated().await);
        assert!(manager.list_accounts().await.is_empty());
    }

    #[tokio::test]
    async fn xai_oauth_store_round_trip_and_mapping() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();

        {
            let manager = XaiOAuthManager::new(path.clone());
            manager
                .add_account_internal(
                    "sub-123".to_string(),
                    "fake-refresh".to_string(),
                    Some("user@example.com".to_string()),
                )
                .await
                .unwrap();
        }

        let manager = XaiOAuthManager::new(path);
        let status = manager.get_status().await;
        assert!(status.authenticated);
        assert_eq!(status.default_account_id.as_deref(), Some("sub-123"));
        assert_eq!(status.accounts[0].provider_login(), "user@example.com");
    }

    #[tokio::test]
    async fn xai_oauth_remove_default_falls_back_to_latest_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_account_internal("old".to_string(), "refresh-1".to_string(), None)
            .await
            .unwrap();
        manager
            .add_account_internal("new".to_string(), "refresh-2".to_string(), None)
            .await
            .unwrap();
        manager.set_default_account("old").await.unwrap();
        manager.remove_account("old").await.unwrap();
        assert_eq!(manager.default_account_id().await.as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn xai_oauth_missing_account_is_failure_path_without_network() {
        let temp = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(temp.path().to_path_buf());
        let err = manager
            .get_valid_token_for_account("missing")
            .await
            .unwrap_err();
        assert!(matches!(err, XaiOAuthError::AccountNotFound(_)));
    }

    trait ProviderLogin {
        fn provider_login(&self) -> &str;
    }

    impl ProviderLogin for GitHubAccount {
        fn provider_login(&self) -> &str {
            &self.login
        }
    }
}
