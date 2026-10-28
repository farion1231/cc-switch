//! Account identity and quota for Kimi official logins.
//!
//! Identities (`/me`: user id, nickname, avatar) are cached per provider id in
//! `kimi_accounts.json` (0600, device-local, never cloud-synced). The cache only
//! labels accounts; Kimi's config and credential files stay the source of truth.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    account_provider, account_region, credential_path, identify, lock, network_error,
    parse_credential, read_config, request, response_json, Endpoints, KimiRegion, PROVIDER_ID,
};
use crate::config::atomic_write_private;
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};

const STORE_FILENAME: &str = "kimi_accounts.json";
/// Serializes read-modify-write of the identity cache across async callers.
static STORE_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn store_path() -> PathBuf {
    crate::config::get_app_config_dir().join(STORE_FILENAME)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) struct Identity {
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// Digest of the refresh token the identity was read with. A different
    /// token may be a different login, so the account is identified again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    pub accounts: BTreeMap<String, Identity>,
    /// Native logins the user removed from the list (user id, or the provider
    /// id when the identity was unknown). CC Switch never logs Kimi out.
    #[serde(default)]
    pub dismissed: Vec<String>,
}

impl Store {
    pub fn load(path: &Path) -> Store {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write_private(path, &bytes).map_err(|e| e.to_string())
    }

    pub fn is_dismissed(&self, id: &str) -> bool {
        id == PROVIDER_ID
            && (self.dismissed.iter().any(|key| key == PROVIDER_ID)
                || self
                    .accounts
                    .get(id)
                    .is_some_and(|identity| self.dismissed.contains(&identity.user_id)))
    }
}

/// Apply one change to the cache under the store lock.
pub(super) fn update_store(path: &Path, change: impl FnOnce(&mut Store)) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    let mut store = Store::load(path);
    store.version = 1;
    change(&mut store);
    store.save(path)
}

/// Remember who holds `id` after its token changed (Kimi rotates the refresh
/// token on every refresh). A hidden native login stays hidden while the same
/// account holds the slot; another account taking it over shows again.
pub(super) fn remember(path: &Path, id: &str, identity: Identity) -> Result<(), String> {
    update_store(path, |store| {
        if id == PROVIDER_ID && store.dismissed.iter().any(|key| key == PROVIDER_ID) {
            // Hidden before we knew who it was: pin the hide to that account.
            store.dismissed.retain(|key| key != PROVIDER_ID);
            let previous = store.accounts.get(id).map(|known| known.user_id.as_str());
            if previous.is_none_or(|user| user == identity.user_id)
                && !store.dismissed.contains(&identity.user_id)
            {
                store.dismissed.push(identity.user_id.clone());
            }
        }
        store.accounts.insert(id.into(), identity);
    })
}

/// Remember an account the user just signed in to through CC Switch, which
/// always brings a hidden native login back.
pub(super) fn remember_login(path: &Path, id: &str, identity: Identity) -> Result<(), String> {
    update_store(path, |store| {
        if id == PROVIDER_ID {
            store
                .dismissed
                .retain(|key| key != PROVIDER_ID && key != &identity.user_id);
        }
        store.accounts.insert(id.into(), identity);
    })
}

/// Stable separate-slot suffix for one account in one region, so the same
/// login maps to the same provider id on every device.
pub(super) fn account_suffix(region: KimiRegion, user_id: &str) -> String {
    let region = match region {
        KimiRegion::MainlandCn => "cn",
        KimiRegion::Global => "global",
    };
    let digest = Sha256::digest(format!("{region}:{user_id}").as_bytes());
    format!("{digest:x}")[..8].to_string()
}

pub(super) fn token_marker(credential: &Value) -> Option<String> {
    let refresh = credential["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())?;
    Some(format!("{:x}", Sha256::digest(refresh.as_bytes()))[..16].to_string())
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    match &payload[key] {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(super) async fn fetch_identity(
    base_url: &str,
    access_token: &str,
    device_id: &str,
) -> Result<Identity, String> {
    let endpoint = format!("{base_url}/me");
    let response = identify(crate::proxy::http_client::get().get(&endpoint), device_id)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| network_error("Kimi account lookup failed", &endpoint, e))?;
    let (status, payload) = response_json(response).await?;
    if status != 200 {
        return Err(format!("Kimi account lookup failed (HTTP {status})"));
    }
    Ok(Identity {
        user_id: string_field(&payload, "user_id")
            .or_else(|| string_field(&payload, "id"))
            .ok_or("Kimi account lookup returned no user id")?,
        nickname: string_field(&payload, "nickname"),
        avatar: string_field(&payload, "avatar"),
        token: None,
    })
}

pub(super) enum Credential {
    Missing,
    Revoked,
    Valid(Value),
}

pub(super) fn read_credential(path: &Path) -> Result<Credential, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Credential::Missing),
        Err(e) => return Err(format!("Cannot read Kimi Code credentials: {e}")),
    };
    let credential: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid Kimi Code credentials; sign in again".to_string())?;
    let empty = |key: &str| credential[key].as_str().is_none_or(str::is_empty);
    // Kimi writes this tombstone after the server rejects a refresh token.
    if empty("access_token") && empty("refresh_token") {
        return Ok(Credential::Revoked);
    }
    Ok(Credential::Valid(credential))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn access_valid(credential: &Value) -> bool {
    credential["access_token"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && credential["expires_at"]
            .as_i64()
            .is_some_and(|expires| expires > now())
}

/// Kimi's own rule: refresh once less than max(5 min, half the lifetime) remains.
fn should_refresh(credential: &Value) -> bool {
    let expires_at = credential["expires_at"].as_i64().unwrap_or(0);
    if expires_at == 0 {
        return false;
    }
    let expires_in = credential["expires_in"].as_i64().unwrap_or(0);
    let threshold = if expires_in > 0 {
        300.max(expires_in / 2)
    } else {
        300
    };
    expires_at - now() < threshold
}

pub(super) enum Access {
    Token(String),
    /// The access token expired and this machine cannot refresh it safely.
    Expired,
    NeedsReauth,
}

fn tombstone(prior: &Value) -> Value {
    json!({
        "access_token": "", "refresh_token": "", "expires_at": 0, "expires_in": 0,
        "scope": prior["scope"].as_str().unwrap_or(""),
        "token_type": prior["token_type"].as_str().unwrap_or("Bearer"),
    })
}

fn write_credential(path: &Path, credential: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(credential).map_err(|e| e.to_string())?;
    atomic_write_private(path, &bytes).map_err(|e| e.to_string())
}

/// A usable access token for `provider`, refreshing it under Kimi's lock with
/// Kimi's protocol (re-read inside the lock; on rejection, adopt a token another
/// process rotated meanwhile, otherwise write the revoked tombstone).
pub(super) async fn access_token(
    home: &Path,
    provider: &Value,
    device_id: &str,
) -> Result<Access, String> {
    let path = credential_path(home, &provider["oauth"])?;
    let credential = match read_credential(&path)? {
        Credential::Missing | Credential::Revoked => return Ok(Access::NeedsReauth),
        Credential::Valid(credential) => credential,
    };
    if !should_refresh(&credential) {
        return Ok(Access::Token(
            credential["access_token"].as_str().unwrap_or("").into(),
        ));
    }
    if !lock::refresh_supported() {
        return Ok(if access_valid(&credential) {
            Access::Token(credential["access_token"].as_str().unwrap().into())
        } else {
            Access::Expired
        });
    }
    let name = slot_name(&path);
    let _lock = lock::acquire(home, &name).await?;
    let current = match read_credential(&path)? {
        Credential::Missing | Credential::Revoked => return Ok(Access::NeedsReauth),
        Credential::Valid(credential) => credential,
    };
    if !should_refresh(&current) {
        return Ok(Access::Token(
            current["access_token"].as_str().unwrap_or("").into(),
        ));
    }
    let Some(refresh_token) = current["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
    else {
        return Ok(Access::NeedsReauth);
    };
    let endpoints = Endpoints {
        oauth_host: provider["oauth"]["oauth_host"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| account_region(provider).endpoints().oauth_host),
        base_url: String::new(),
    };
    let response = request(&endpoints, "/api/oauth/token", device_id)
        .form(&[
            ("client_id", super::CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|e| network_error("Kimi token refresh failed", &endpoints.oauth_host, e))?;
    let (status, payload) = response_json(response).await?;
    if status == 200 {
        let refreshed = parse_credential(&payload)?;
        write_credential(&path, &refreshed)?;
        return Ok(Access::Token(
            refreshed["access_token"].as_str().unwrap().into(),
        ));
    }
    if matches!(status, 401 | 403) || payload["error"] == "invalid_grant" {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Credential::Valid(recovered) = read_credential(&path)? {
            if recovered["refresh_token"].as_str() != Some(refresh_token.as_str()) {
                return Ok(Access::Token(
                    recovered["access_token"].as_str().unwrap_or("").into(),
                ));
            }
        }
        write_credential(&path, &tombstone(&current))?;
        return Ok(Access::NeedsReauth);
    }
    Err(format!("Kimi token refresh failed (HTTP {status})"))
}

/// Credential file stem, which is also Kimi's lock name.
pub(super) fn slot_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Identify accounts whose current login has not been looked up yet. Only
/// uses access tokens that are still valid; never refreshes just for a name.
pub(super) async fn identify_accounts(home: &Path, store: &Path) {
    let Ok(config) = read_config(home) else {
        return;
    };
    // Created lazily: listing accounts must not write into Kimi's home when
    // there is nothing to identify.
    let mut device: Option<String> = None;
    let known = Store::load(store).accounts;
    for (id, provider) in config["providers"].as_object().into_iter().flatten() {
        if !crate::kimi_config::is_official_provider(&json!({"provider": provider})) {
            continue;
        }
        let Ok(path) = credential_path(home, &provider["oauth"]) else {
            continue;
        };
        let Ok(Credential::Valid(credential)) = read_credential(&path) else {
            continue;
        };
        let marker = token_marker(&credential);
        if known
            .get(id)
            .is_some_and(|identity| identity.token == marker)
            || !access_valid(&credential)
        {
            continue;
        }
        let Some(base_url) = provider["base_url"].as_str() else {
            continue;
        };
        let access = credential["access_token"].as_str().unwrap_or_default();
        if device.is_none() {
            let Ok(id) = super::device_id(home) else {
                return;
            };
            device = Some(id);
        }
        let device_id = device.as_deref().unwrap();
        if let Ok(mut identity) = fetch_identity(base_url, access, device_id).await {
            identity.token = marker;
            let _ = remember(store, id, identity);
        }
    }
}

fn quota_status(status: CredentialStatus, message: &str) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "kimi_oauth".into(),
        credential_status: status,
        credential_message: Some(message.into()),
        success: false,
        tiers: vec![],
        extra_usage: None,
        reset_credits: None,
        credits_balance: None,
        error: None,
        queried_at: Some(chrono::Utc::now().timestamp_millis()),
    }
}

pub(super) async fn quota(
    home: &Path,
    store: &Path,
    account_id: &str,
    device_id: &str,
) -> Result<SubscriptionQuota, String> {
    let config = read_config(home)?;
    // A card synced from another device has no node or no login here: show
    // nothing instead of an error.
    let not_here = || SubscriptionQuota {
        credential_message: None,
        ..quota_status(CredentialStatus::NotFound, "")
    };
    let Ok(provider) = account_provider(&config, account_id) else {
        return Ok(not_here());
    };
    if matches!(
        read_credential(&credential_path(home, &provider["oauth"])?)?,
        Credential::Missing
    ) {
        return Ok(not_here());
    }
    let base_url = provider["base_url"]
        .as_str()
        .ok_or("Kimi account has no base URL")?;
    let token = match access_token(home, provider, device_id).await? {
        Access::Token(token) => token,
        Access::Expired => {
            return Ok(quota_status(
                CredentialStatus::RefreshPending,
                "Use this account once in Kimi Code, then refresh",
            ))
        }
        Access::NeedsReauth => return Ok(quota_status(CredentialStatus::Expired, "Sign in again")),
    };
    // A refresh may have rotated the token; label the account with it too.
    if let Ok(Credential::Valid(credential)) =
        read_credential(&credential_path(home, &provider["oauth"])?)
    {
        let marker = token_marker(&credential);
        if Store::load(store)
            .accounts
            .get(account_id)
            .is_none_or(|identity| identity.token != marker)
        {
            if let Ok(mut identity) = fetch_identity(base_url, &token, device_id).await {
                identity.token = marker;
                let _ = remember(store, account_id, identity);
            }
        }
    }
    let mut quota = crate::services::coding_plan::query_kimi_at(base_url, &token).await?;
    quota.tool = "kimi_oauth".into();
    Ok(quota)
}

/// Fail early when a Kimi official card points at a login this machine does
/// not have (e.g. a profile synced from another device).
pub(crate) fn ensure_signed_in(home: &Path, oauth: &Value) -> Result<(), String> {
    match read_credential(&credential_path(home, oauth)?)? {
        Credential::Valid(_) => Ok(()),
        Credential::Missing | Credential::Revoked => Err("not signed in".into()),
    }
}

/// Delete a separate-slot credential under Kimi's lock. The native default
/// slot (`kimi-code`, `kimi-code-env-*`) belongs to Kimi and is never deleted.
pub(crate) fn remove_separate_credential_blocking(
    home: &Path,
    oauth: &Value,
) -> Result<(), String> {
    let path = credential_path(home, oauth)?;
    let name = slot_name(&path);
    if !name.starts_with("kimi-code-ccs-") {
        return Ok(());
    }
    let _lock = lock::acquire_blocking(home, &name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Cannot remove Kimi Code credentials: {e}")),
    }
}
