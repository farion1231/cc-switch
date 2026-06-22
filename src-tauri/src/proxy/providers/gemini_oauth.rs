//! Gemini (Google) OAuth token refresh for the proxy request path.
//!
//! Google OAuth `access_token`s issued for the Gemini CLI expire after ~1h. A
//! provider can be configured with OAuth credentials — either a bare `ya29.`
//! access token or an `oauth_creds.json` blob (the file the Gemini CLI writes
//! after `gemini` login). When only an expired access token plus a long-lived
//! `refresh_token` are available, the proxy must exchange the refresh token for
//! a fresh access token so routed traffic keeps working across sessions.
//!
//! The refresh result is cached in-memory keyed by refresh token so concurrent
//! requests do not each hit Google's token endpoint.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

/// Public installed-app OAuth client used by the Gemini CLI
/// (`google-gemini/gemini-cli`). Per the OAuth spec installed-app client
/// secrets are not confidential; these match the credentials the CLI ships and
/// are required to refresh tokens minted for that client.
pub const GEMINI_OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
pub const GEMINI_OAUTH_CLIENT_SECRET: &str = "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";

const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Refresh slightly before the real expiry so a token never expires mid-flight.
const EXPIRY_SKEW: Duration = Duration::from_secs(60);

/// Google OAuth credentials extracted from a provider's key field.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct GoogleOAuthCredentials {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// Absolute expiry of `access_token` in epoch milliseconds (Gemini CLI
    /// writes this as `expiry_date`). `None` when unknown.
    pub expiry_date_ms: Option<i64>,
}

impl fmt::Debug for GoogleOAuthCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleOAuthCredentials")
            .field("access_token", &redact_optional_token(&self.access_token))
            .field("refresh_token", &redact_optional_token(&self.refresh_token))
            .field("client_id", &self.client_id)
            .field("client_secret", &redact_optional_token(&self.client_secret))
            .field("expiry_date_ms", &self.expiry_date_ms)
            .finish()
    }
}

impl GoogleOAuthCredentials {
    #[allow(dead_code)]
    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token
            .as_deref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }
}

fn redact_optional_token(value: &Option<String>) -> Option<String> {
    value.as_deref().map(redact_token)
}

fn redact_token(value: &str) -> String {
    if value.chars().count() <= 8 {
        return "***".to_string();
    }
    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

/// Parse OAuth credentials from a provider key value.
///
/// Accepts either a bare `ya29.` access token or a JSON object such as the
/// Gemini CLI `oauth_creds.json`
/// (`{ "access_token", "refresh_token", "client_id"?, "client_secret"?, "expiry_date"? }`).
/// Returns `None` for plain API keys (handled as `x-goog-api-key` elsewhere).
pub fn parse_credentials(key: &str) -> Option<GoogleOAuthCredentials> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    if key.starts_with("ya29.") {
        return Some(GoogleOAuthCredentials {
            access_token: Some(key.to_string()),
            ..Default::default()
        });
    }

    if key.starts_with('{') {
        let json: serde_json::Value = serde_json::from_str(key).ok()?;
        let str_field = |name: &str| {
            json.get(name)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let access_token = str_field("access_token");
        let refresh_token = str_field("refresh_token");
        if access_token.is_none() && refresh_token.is_none() {
            return None;
        }
        return Some(GoogleOAuthCredentials {
            access_token,
            refresh_token,
            client_id: str_field("client_id"),
            client_secret: str_field("client_secret"),
            expiry_date_ms: json.get("expiry_date").and_then(|v| v.as_i64()),
        });
    }

    None
}

/// Whether an access token with the given absolute expiry (epoch ms) should be
/// treated as expired. Unknown expiry → treat as expired so we refresh when a
/// refresh token is available.
fn access_token_expired(expiry_date_ms: Option<i64>, now_ms: i64) -> bool {
    match expiry_date_ms {
        Some(expiry) => now_ms + EXPIRY_SKEW.as_millis() as i64 >= expiry,
        None => true,
    }
}

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    refresh_at: Instant,
}

/// Token response from Google's OAuth token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// In-memory cache + refresher for Google OAuth access tokens, keyed by refresh
/// token. Registered as Tauri-managed state and shared across proxy requests.
#[derive(Default)]
pub struct GeminiOAuthManager {
    cache: RwLock<HashMap<String, CachedToken>>,
    refresh_locks: RwLock<HashMap<String, Arc<Mutex<()>>>>,
}

/// Tauri-managed state holding the shared Gemini OAuth token manager.
#[derive(Clone)]
pub struct GeminiOAuthState(pub Arc<GeminiOAuthManager>);

impl GeminiOAuthManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a valid access token for the given credentials, refreshing it via
    /// the refresh token when the configured access token is missing or expired.
    pub async fn get_valid_access_token(
        &self,
        creds: &GoogleOAuthCredentials,
    ) -> Result<String, String> {
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Use the configured access token when it is known to be still valid.
        if let Some(token) = creds.access_token.as_deref() {
            if !token.is_empty() && !access_token_expired(creds.expiry_date_ms, now_ms) {
                return Ok(token.to_string());
            }
        }

        let refresh_token = creds.refresh_token.as_deref().filter(|t| !t.is_empty());
        let Some(refresh_token) = refresh_token else {
            // No refresh token: fall back to whatever access token we have.
            return creds
                .access_token
                .clone()
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    "Gemini OAuth credentials have no usable access token or refresh token"
                        .to_string()
                });
        };

        // Serve from cache when the cached token is still valid.
        if let Some(cached) = self.cache.read().await.get(refresh_token) {
            if Instant::now() < cached.refresh_at {
                return Ok(cached.access_token.clone());
            }
        }

        let refresh_lock = self.get_refresh_lock(refresh_token).await;
        let _refresh_guard = refresh_lock.lock().await;

        // Another request may have refreshed while this one waited for the lock.
        if let Some(cached) = self.cache.read().await.get(refresh_token) {
            if Instant::now() < cached.refresh_at {
                return Ok(cached.access_token.clone());
            }
        }

        let client_id = creds.client_id.as_deref().unwrap_or(GEMINI_OAUTH_CLIENT_ID);
        let client_secret = creds
            .client_secret
            .as_deref()
            .unwrap_or(GEMINI_OAUTH_CLIENT_SECRET);
        let refreshed = refresh_access_token(refresh_token, client_id, client_secret).await?;

        let ttl = Duration::from_secs(refreshed.expires_in.unwrap_or(3600));
        let refresh_at = Instant::now() + ttl.saturating_sub(EXPIRY_SKEW);
        self.cache.write().await.insert(
            refresh_token.to_string(),
            CachedToken {
                access_token: refreshed.access_token.clone(),
                refresh_at,
            },
        );
        Ok(refreshed.access_token)
    }

    async fn get_refresh_lock(&self, refresh_token: &str) -> Arc<Mutex<()>> {
        if let Some(lock) = self.refresh_locks.read().await.get(refresh_token) {
            return lock.clone();
        }

        let mut locks = self.refresh_locks.write().await;
        locks
            .entry(refresh_token.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

async fn refresh_access_token(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenResponse, String> {
    let client = crate::proxy::http_client::get();
    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Google OAuth token refresh request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Google OAuth token refresh returned HTTP {}",
            resp.status()
        ));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("failed to parse Google OAuth token response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_credentials_rejects_plain_api_key() {
        assert_eq!(parse_credentials("AIzaSyPlainApiKey"), None);
        assert_eq!(parse_credentials(""), None);
        assert_eq!(parse_credentials("   "), None);
    }

    #[test]
    fn parse_credentials_reads_bare_access_token() {
        let creds = parse_credentials("  ya29.abc123  ").expect("creds");
        assert_eq!(creds.access_token.as_deref(), Some("ya29.abc123"));
        assert!(!creds.has_refresh_token());
    }

    #[test]
    fn parse_credentials_reads_oauth_creds_json() {
        let json = r#"{
            "access_token": "ya29.token",
            "refresh_token": "1//refresh",
            "expiry_date": 1700000000000
        }"#;
        let creds = parse_credentials(json).expect("creds");
        assert_eq!(creds.access_token.as_deref(), Some("ya29.token"));
        assert_eq!(creds.refresh_token.as_deref(), Some("1//refresh"));
        assert_eq!(creds.expiry_date_ms, Some(1_700_000_000_000));
        assert!(creds.client_id.is_none());
        assert!(creds.has_refresh_token());
    }

    #[test]
    fn parse_credentials_reads_custom_client() {
        let json = r#"{
            "refresh_token": "1//refresh",
            "client_id": "custom-id",
            "client_secret": "custom-secret"
        }"#;
        let creds = parse_credentials(json).expect("creds");
        assert!(creds.access_token.is_none());
        assert_eq!(creds.client_id.as_deref(), Some("custom-id"));
        assert_eq!(creds.client_secret.as_deref(), Some("custom-secret"));
    }

    #[test]
    fn parse_credentials_trims_json_fields_and_rejects_blank_tokens() {
        let json = r#"{
            "access_token": "  ya29.token  ",
            "refresh_token": "   ",
            "client_id": " custom-id ",
            "client_secret": " custom-secret "
        }"#;
        let creds = parse_credentials(json).expect("creds");
        assert_eq!(creds.access_token.as_deref(), Some("ya29.token"));
        assert_eq!(creds.refresh_token, None);
        assert_eq!(creds.client_id.as_deref(), Some("custom-id"));
        assert_eq!(creds.client_secret.as_deref(), Some("custom-secret"));
        assert!(!creds.has_refresh_token());
    }

    #[test]
    fn parse_credentials_requires_a_token_field() {
        let json = r#"{ "scope": "https://www.googleapis.com/auth/cloud-platform" }"#;
        assert_eq!(parse_credentials(json), None);
    }

    #[test]
    fn access_token_expiry_uses_skew() {
        let now = 1_700_000_000_000;
        // Expires comfortably in the future → still valid.
        assert!(!access_token_expired(Some(now + 5 * 60 * 1000), now));
        // Within the skew window → treat as expired.
        assert!(access_token_expired(Some(now + 30 * 1000), now));
        // Already past → expired.
        assert!(access_token_expired(Some(now - 1000), now));
        // Unknown expiry → expired so we refresh when possible.
        assert!(access_token_expired(None, now));
    }

    #[test]
    fn debug_redacts_oauth_tokens_and_client_secret() {
        let creds = GoogleOAuthCredentials {
            access_token: Some("access-token-value".to_string()),
            refresh_token: Some("refresh-token-value".to_string()),
            client_id: Some("public-client-id".to_string()),
            client_secret: Some("client-private-value".to_string()),
            expiry_date_ms: Some(1_700_000_000_000),
        };

        let debug = format!("{creds:?}");
        assert!(debug.contains("acce...alue"));
        assert!(debug.contains("refr...alue"));
        assert!(debug.contains("clie...alue"));
        assert!(debug.contains("public-client-id"));
        assert!(!debug.contains("access-token-value"));
        assert!(!debug.contains("refresh-token-value"));
        assert!(!debug.contains("client-private-value"));
    }

    #[tokio::test]
    async fn get_valid_access_token_returns_unexpired_token_without_refresh() {
        let manager = GeminiOAuthManager::new();
        let future_ms = chrono::Utc::now().timestamp_millis() + 10 * 60 * 1000;
        let creds = GoogleOAuthCredentials {
            access_token: Some("ya29.valid".to_string()),
            refresh_token: Some("1//refresh".to_string()),
            expiry_date_ms: Some(future_ms),
            ..Default::default()
        };
        let token = manager.get_valid_access_token(&creds).await.expect("token");
        assert_eq!(token, "ya29.valid");
    }

    #[tokio::test]
    async fn get_valid_access_token_without_refresh_falls_back_to_access_token() {
        let manager = GeminiOAuthManager::new();
        let creds = GoogleOAuthCredentials {
            access_token: Some("ya29.only".to_string()),
            ..Default::default()
        };
        let token = manager.get_valid_access_token(&creds).await.expect("token");
        assert_eq!(token, "ya29.only");
    }

    #[tokio::test]
    async fn get_valid_access_token_errors_when_nothing_usable() {
        let manager = GeminiOAuthManager::new();
        let creds = GoogleOAuthCredentials::default();
        assert!(manager.get_valid_access_token(&creds).await.is_err());
    }

    #[tokio::test]
    async fn refresh_locks_are_reused_per_refresh_token() {
        let manager = GeminiOAuthManager::new();
        let first = manager.get_refresh_lock("1//same").await;
        let second = manager.get_refresh_lock("1//same").await;
        let other = manager.get_refresh_lock("1//other").await;

        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));
    }
}
