//! Official Kimi Code device login. Credentials stay in Kimi's native file
//! store. Kimi rotates refresh tokens during model requests; CC Switch only
//! refreshes (for quota) under the same cross-process lock, see [`lock`].
mod account;
mod lock;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, Instant},
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{config::atomic_write_private, kimi_config, services::subscription::SubscriptionQuota};

const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const PROVIDER_ID: &str = kimi_config::OFFICIAL_PROVIDER_ID;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum KimiRegion {
    #[default]
    MainlandCn,
    Global,
}

impl KimiRegion {
    fn endpoints(self) -> Endpoints {
        let (oauth_host, base_url) = match self {
            Self::MainlandCn => ("https://auth.kimi.com", "https://api.kimi.com/coding/v1"),
            Self::Global => ("https://auth.kimi.ai", "https://api.kimi.ai/coding/v1"),
        };
        Endpoints {
            oauth_host: oauth_host.into(),
            base_url: base_url.into(),
        }
    }
}

#[derive(Clone)]
struct Endpoints {
    oauth_host: String,
    base_url: String,
}

impl Endpoints {
    fn region(&self) -> KimiRegion {
        if self.oauth_host == "https://auth.kimi.ai" {
            KimiRegion::Global
        } else {
            KimiRegion::MainlandCn
        }
    }

    fn oauth_ref(&self) -> Value {
        // Match Kimi Code's native default slot for this environment.
        if self.oauth_host == "https://auth.kimi.com"
            && self.base_url == "https://api.kimi.com/coding/v1"
        {
            json!({"storage": "file", "key": "oauth/kimi-code"})
        } else {
            let environment = json!({"oauthHost": self.oauth_host, "baseUrl": self.base_url});
            let digest = format!("{:x}", Sha256::digest(environment.to_string().as_bytes()));
            json!({"storage": "file", "key": format!("oauth/kimi-code-env-{}", &digest[..16]), "oauth_host": self.oauth_host})
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiOAuthStatus {
    pub accounts: Vec<KimiOAuthAccount>,
    pub current_account_id: Option<String>,
    /// Kimi's own login is still there but was removed from the list, so an
    /// empty list does not mean Kimi is signed out.
    pub primary_hidden: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiOAuthAccount {
    pub id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    /// Kimi's own default login slot: web search and fetch always use it, and
    /// removing it from the list never logs Kimi out.
    pub primary: bool,
    pub authenticated: bool,
    pub region: KimiRegion,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiLoginStart {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiLoginPoll {
    pub authenticated: bool,
    pub interval: u64,
    pub ended: Option<&'static str>,
}

impl KimiLoginPoll {
    fn pending(interval: u64) -> Self {
        Self {
            authenticated: false,
            interval,
            ended: None,
        }
    }
    fn ended(reason: &'static str) -> Self {
        Self {
            authenticated: false,
            interval: 0,
            ended: Some(reason),
        }
    }
}

enum PollOutcome {
    Pending,
    Ready(Map<String, Value>),
    Ended(&'static str),
}

#[derive(Clone)]
struct PendingLogin {
    polling: bool,
    id: String,
    /// Explicit re-login target. New logins pick their slot only when they
    /// commit, once the account identity is known.
    target: Option<(String, Value)>,
    identity: Option<account::Identity>,
    store: PathBuf,
    device_code: String,
    expires: Instant,
    next_poll: Instant,
    interval: u64,
    home: PathBuf,
    endpoints: Endpoints,
    device_id: String,
    // Keep a successful token in memory if model discovery or a disk write
    // fails, so retry never consumes the single-use device grant again.
    credential: Option<Value>,
}

#[derive(Default)]
pub struct KimiOAuthManager {
    pending: Mutex<Option<PendingLogin>>,
    /// Identity cache location; tests point it at a temporary directory.
    store: Option<PathBuf>,
}

fn credential_path(home: &Path, oauth: &Value) -> Result<PathBuf, String> {
    let key = oauth
        .get("key")
        .and_then(Value::as_str)
        .ok_or("Missing Kimi OAuth reference")?;
    let name = key.strip_prefix("oauth/").unwrap_or(key);
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid Kimi OAuth credential name".into());
    }
    if oauth
        .get("storage")
        .and_then(Value::as_str)
        .is_some_and(|s| s != "file")
    {
        return Err("This Kimi login uses a keyring; manage it in Kimi Code".into());
    }
    Ok(home.join("credentials").join(format!("{name}.json")))
}

fn account_status(
    home: &Path,
    id: &str,
    region: KimiRegion,
    oauth: &Value,
    identity: Option<&account::Identity>,
) -> Result<KimiOAuthAccount, String> {
    let path = credential_path(home, oauth)?;
    let credential = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| "Invalid Kimi Code credentials; sign in again".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("Cannot read Kimi Code credentials: {e}")),
    };
    let expires_at = credential["expires_at"].as_i64();
    let has_access = credential["access_token"]
        .as_str()
        .is_some_and(|s| !s.is_empty());
    let can_refresh = credential["refresh_token"]
        .as_str()
        .is_some_and(|s| !s.is_empty());
    // An expired access token with a refresh token is still a usable native
    // login. Never rotate it here: Kimi coordinates refresh across processes.
    let nickname = identity.and_then(|identity| identity.nickname.clone());
    Ok(KimiOAuthAccount {
        id: id.into(),
        name: kimi_config::official_provider_name(id, nickname.as_deref()),
        nickname,
        avatar_url: identity.and_then(|identity| identity.avatar.clone()),
        primary: id == PROVIDER_ID,
        authenticated: can_refresh
            || (has_access && expires_at.is_some_and(|t| t > chrono::Utc::now().timestamp())),
        region,
        expires_at,
        error: None,
    })
}

fn read_config(home: &Path) -> Result<Value, String> {
    kimi_config::read(&home.join("config.toml")).map_err(|error| error.to_string())
}

/// A new account goes into Kimi's default slot while that is free; otherwise it
/// gets its own credential slot `kimi-code-ccs-<suffix>`. Kimi shows a
/// provider's key as its name, so a separate account's key is
/// `Kimi 订阅 <nickname>` when the nickname is known; the slot name stays the
/// stable identity.
fn new_account(
    home: &Path,
    endpoints: &Endpoints,
    suffix: &str,
    nickname: Option<&str>,
) -> Result<(String, Value), String> {
    let config = read_config(home)?;
    let provider = &config["providers"][PROVIDER_ID];
    let native_oauth = endpoints.oauth_ref();
    let existing_oauth = provider.get("oauth").unwrap_or(&native_oauth);
    // Kimi's native tools resolve their own default credential slot. Keep the
    // first login compatible without replacing an existing default account.
    let occupied = !provider.is_null()
        && (existing_oauth["storage"].as_str() == Some("keyring")
            || credential_path(home, existing_oauth)?
                .try_exists()
                .map_err(|e| format!("Cannot check Kimi Code credentials: {e}"))?);
    if occupied {
        let slot = format!("kimi-code-ccs-{suffix}");
        let oauth = json!({"storage": "file", "key": format!("oauth/{slot}"), "oauth_host": endpoints.oauth_host});
        let id = readable_provider_id(&config, &subscription_label(), nickname);
        Ok((id.unwrap_or(slot), oauth))
    } else {
        Ok((PROVIDER_ID.into(), native_oauth))
    }
}

/// Kimi's own label for its default login ("Kimi 订阅" / "Kimi Subscription"),
/// in CC Switch's language, so separate accounts sit next to it in the model
/// picker as `Kimi 订阅 <nickname>`. Kimi only ships Chinese and English.
fn subscription_label() -> String {
    if crate::tray::tray_language().starts_with("zh") {
        "Kimi 订阅".into()
    } else {
        "Kimi Subscription".into()
    }
}

/// `<label> <nickname>` limited to what Kimi accepts as a provider name
/// (letters, digits, `-`, `_` and spaces), numbered when another provider
/// has the name.
fn readable_provider_id(config: &Value, label: &str, nickname: Option<&str>) -> Option<String> {
    static DISALLOWED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\-_ ]+").unwrap());
    let nickname = DISALLOWED.replace_all(nickname?, " ");
    let nickname = nickname.split_whitespace().collect::<Vec<_>>().join(" ");
    if nickname.is_empty() {
        return None;
    }
    let base = format!("{label} {nickname}");
    let taken = |id: &str| config["providers"].get(id).is_some();
    (1..)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|id| !taken(id))
}

fn account_provider<'a>(config: &'a Value, id: &str) -> Result<&'a Value, String> {
    let provider = &config["providers"][id];
    if !kimi_config::is_official_provider(&json!({"provider": provider})) {
        return Err("Kimi official account not found".into());
    }
    Ok(provider)
}

fn account_region(provider: &Value) -> KimiRegion {
    if provider["oauth"]["oauth_host"].as_str() == Some("https://auth.kimi.ai")
        || provider["base_url"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://api.kimi.ai/"))
    {
        KimiRegion::Global
    } else {
        KimiRegion::MainlandCn
    }
}

fn login_target(
    home: &Path,
    region: KimiRegion,
    account_id: Option<&str>,
) -> Result<(Endpoints, Option<Value>), String> {
    let Some(id) = account_id else {
        // The explicit region selection owns both endpoints. Process-wide Kimi
        // overrides must not send one region's device flow to another host.
        return Ok((region.endpoints(), None));
    };
    let config = read_config(home)?;
    let provider = account_provider(&config, id)?;
    credential_path(home, &provider["oauth"])?;
    let region = account_region(provider);
    let endpoints = region.endpoints();
    let mut oauth = provider["oauth"].clone();
    // Keep the account's credential slot and region while restoring its fixed
    // official refresh host when an older config contains an endpoint override.
    if oauth.get("oauth_host").is_some() || region == KimiRegion::Global {
        oauth["oauth_host"] = json!(endpoints.oauth_host);
    }
    Ok((endpoints, Some(oauth)))
}

/// Pick where a finished login is saved, now that its identity is known:
/// the same account (same region) signs in again in place, an explicit
/// re-login keeps its slot unless a different account was used, and anything
/// else is a new account (Kimi's default slot first, then a separate slot).
fn resolve_target(
    home: &Path,
    store: &Path,
    endpoints: &Endpoints,
    explicit: Option<(String, Value)>,
    identity: Option<&account::Identity>,
) -> Result<(String, Value), String> {
    let Some(identity) = identity else {
        return match explicit {
            Some(target) => Ok(target),
            None => new_account(home, endpoints, &uuid::Uuid::new_v4().to_string(), None),
        };
    };
    let region = endpoints.region();
    let config = read_config(home)?;
    let known = account::Store::load(store).accounts;
    let suffix = account::account_suffix(region, &identity.user_id);
    let separate = format!("kimi-code-ccs-{suffix}");
    let separate_key = format!("oauth/{separate}");
    let same_account = config["providers"]
        .as_object()
        .into_iter()
        .flatten()
        .find(|(id, provider)| {
            kimi_config::is_official_provider(&json!({"provider": provider}))
                && account_region(provider) == region
                && (known
                    .get(*id)
                    .is_some_and(|known| known.user_id == identity.user_id)
                    || **id == separate
                    || provider["oauth"]["key"].as_str() == Some(separate_key.as_str()))
        })
        .map(|(id, _)| id.clone());
    if let Some(id) = same_account {
        let (_, oauth) = login_target(home, region, Some(&id))?;
        return Ok((id, oauth.unwrap()));
    }
    if let Some((id, oauth)) = explicit {
        if known
            .get(&id)
            .is_none_or(|known| known.user_id == identity.user_id)
        {
            return Ok((id, oauth));
        }
    }
    new_account(home, endpoints, &suffix, identity.nickname.as_deref())
}

fn network_error(context: &str, endpoint: &str, error: reqwest::Error) -> String {
    format!(
        "{context} ({}): {}",
        crate::proxy::http_client::mask_url(endpoint),
        crate::proxy::error::describe_reqwest_error(error)
    )
}

fn accounts_status(home: &Path, store: &Path) -> Result<KimiOAuthStatus, String> {
    let config = read_config(home)?;
    let store = account::Store::load(store);
    let mut accounts = Vec::new();
    for (id, provider) in config
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if !kimi_config::is_official_provider(&json!({"provider": provider}))
            || store.is_dismissed(id)
        {
            continue;
        }
        let region = account_region(provider);
        let identity = store.accounts.get(id);
        accounts.push(
            account_status(home, id, region, &provider["oauth"], identity).unwrap_or_else(
                |error| {
                    let nickname = identity.and_then(|identity| identity.nickname.clone());
                    KimiOAuthAccount {
                        id: id.clone(),
                        name: kimi_config::official_provider_name(id, nickname.as_deref()),
                        nickname,
                        avatar_url: identity.and_then(|identity| identity.avatar.clone()),
                        primary: id == PROVIDER_ID,
                        authenticated: false,
                        region,
                        expires_at: None,
                        error: Some(error),
                    }
                },
            ),
        );
    }
    let current_account_id = config["default_model"]
        .as_str()
        .and_then(|key| config["models"][key]["provider"].as_str())
        .filter(|id| accounts.iter().any(|account| account.id == *id))
        .map(str::to_owned);
    let primary_hidden = store.is_dismissed(PROVIDER_ID)
        && config["providers"]
            .get(PROVIDER_ID)
            .is_some_and(|provider| {
                kimi_config::is_official_provider(&json!({"provider": provider}))
            });
    Ok(KimiOAuthStatus {
        accounts,
        current_account_id,
        primary_hidden,
    })
}

fn device_id(home: &Path) -> Result<String, String> {
    let path = home.join("device_id");
    match fs::read_to_string(&path) {
        Ok(id) if !id.trim().is_empty() => Ok(id.trim().to_string()),
        Ok(_) => create_device_id(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => create_device_id(&path),
        Err(e) => Err(format!("Cannot read Kimi device identity: {e}")),
    }
}

fn create_device_id(path: &Path) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    atomic_write_private(path, id.as_bytes()).map_err(|e| e.to_string())?;
    Ok(id)
}

fn request(endpoints: &Endpoints, path: &str, device_id: &str) -> reqwest::RequestBuilder {
    identify(
        crate::proxy::http_client::get().post(format!("{}{path}", endpoints.oauth_host)),
        device_id,
    )
}

fn identify(request: reqwest::RequestBuilder, device_id: &str) -> reqwest::RequestBuilder {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Unknown".into());
    let hostname: String = hostname
        .chars()
        .filter(char::is_ascii)
        .filter(|c| !c.is_ascii_control())
        .take(128)
        .collect();
    request
        .timeout(Duration::from_secs(30))
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            concat!("cc-switch/", env!("CARGO_PKG_VERSION")),
        )
        .header("X-Msh-Platform", "kimi_code_cli")
        .header("X-Msh-Version", env!("CARGO_PKG_VERSION"))
        .header("X-Msh-Device-Id", device_id)
        .header(
            "X-Msh-Device-Name",
            if hostname.is_empty() {
                "Unknown"
            } else {
                &hostname
            },
        )
        .header(
            "X-Msh-Device-Model",
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        )
        .header("X-Msh-Os-Version", std::env::consts::OS)
}

async fn response_json(response: reqwest::Response) -> Result<(u16, Value), String> {
    let status = response.status().as_u16();
    let payload = response
        .json::<Value>()
        .await
        .map_err(|_| format!("Unexpected Kimi response (HTTP {status})"))?;
    Ok((status, payload))
}

fn required_string<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Kimi response missing {key}"))
}

fn seconds(payload: &Value, key: &str, fallback: Option<u64>) -> Result<u64, String> {
    payload[key]
        .as_u64()
        .or_else(|| payload[key].as_str()?.parse().ok())
        .or(fallback)
        .filter(|n| *n > 0 && *n <= 31_536_000)
        .ok_or_else(|| format!("Kimi response has invalid {key}"))
}

fn parse_credential(payload: &Value) -> Result<Value, String> {
    let access = required_string(payload, "access_token")?;
    let refresh = required_string(payload, "refresh_token")?;
    let expires = seconds(payload, "expires_in", None)?;
    Ok(json!({
        "access_token": access, "refresh_token": refresh,
        "expires_at": chrono::Utc::now().timestamp() + expires as i64,
        "expires_in": expires, "token_type": payload["token_type"].as_str().unwrap_or("Bearer"),
        "scope": payload["scope"].as_str().unwrap_or("")
    }))
}

fn parse_models(payload: &Value) -> Result<Map<String, Value>, String> {
    let data = payload["data"]
        .as_array()
        .filter(|models| !models.is_empty())
        .ok_or("No models available for Kimi Code")?;
    let mut models = Map::new();
    for model in data {
        let id = required_string(model, "id")?;
        let context = model["limit"]["context"]
            .as_u64()
            .or_else(|| model["limit"]["context"].as_str()?.parse().ok())
            .filter(|n| *n > 0 && *n <= i64::MAX as u64)
            .ok_or_else(|| format!("Kimi model '{id}' has no valid context length"))?;
        let mut capabilities = Vec::new();
        if model["supports_tool_use"] != false {
            capabilities.push("tool_use");
        }
        for (modality, capability) in [
            ("image", "image_in"),
            ("video", "video_in"),
            ("audio", "audio_in"),
        ] {
            if model["modalities"]["input"]
                .as_array()
                .is_some_and(|inputs| inputs.iter().any(|v| v == modality))
            {
                capabilities.push(capability);
            }
        }
        let thinking = model["reasoning"]["type"].as_str();
        if matches!(thinking, Some("only" | "both")) {
            capabilities.push("thinking");
        }
        if thinking == Some("only") {
            capabilities.push("always_thinking");
        }
        if model["tools"]["dynamic_load"] == true {
            capabilities.push("dynamically_loaded_tools");
        }
        let mut alias = json!({"provider": PROVIDER_ID, "model": id, "max_context_size": context, "capabilities": capabilities});
        if let Some(name) = model["display_name"].as_str() {
            alias["display_name"] = json!(name);
        }
        let protocol = match model["protocol"].as_str() {
            Some("response") => Some("openai_responses"),
            Some("anthropic") => Some("anthropic"),
            _ => None,
        };
        if let Some(protocol) = protocol {
            alias["protocol"] = json!(protocol);
            if protocol == "anthropic" {
                alias["beta_api"] = json!(true);
                if matches!(thinking, Some("only" | "both")) {
                    alias["adaptive_thinking"] = json!(true);
                }
            }
        }
        if let Some(efforts) = model["reasoning"]["effort"]["valid"].as_array() {
            let efforts: Vec<&str> = efforts
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .collect();
            if !efforts.is_empty() {
                if let Some(default) = model["reasoning"]["effort"]["default"]
                    .as_str()
                    .filter(|s| efforts.contains(s))
                {
                    alias["default_effort"] = json!(default);
                }
                alias["support_efforts"] = json!(efforts);
            }
        }
        models.insert(format!("kimi-code/{id}"), alias);
    }
    Ok(models)
}

impl KimiOAuthManager {
    fn store(&self) -> PathBuf {
        self.store.clone().unwrap_or_else(account::store_path)
    }

    #[cfg(test)]
    fn with_store(store: PathBuf) -> Self {
        Self {
            pending: Mutex::new(None),
            store: Some(store),
        }
    }

    pub async fn status(&self) -> Result<KimiOAuthStatus, String> {
        let home = kimi_config::data_dir();
        let store = self.store();
        account::identify_accounts(&home, &store).await;
        accounts_status(&home, &store)
    }

    pub async fn quota(&self, account_id: &str) -> Result<SubscriptionQuota, String> {
        let home = kimi_config::data_dir();
        let device = device_id(&home)?;
        account::quota(&home, &self.store(), account_id, &device).await
    }

    /// The account's current model catalog, keyed and shaped as signing in
    /// would write it, for the card editor. Nothing is saved here.
    pub async fn models(&self, account_id: &str) -> Result<Map<String, Value>, String> {
        models_at(&kimi_config::data_dir(), &self.store(), account_id).await
    }

    pub async fn start(
        &self,
        region: KimiRegion,
        account_id: Option<String>,
    ) -> Result<KimiLoginStart, String> {
        let home = kimi_config::data_dir();
        let (endpoints, oauth) = login_target(&home, region, account_id.as_deref())?;
        self.start_login(home, endpoints, account_id, oauth).await
    }

    #[cfg(test)]
    async fn start_with(
        &self,
        home: PathBuf,
        endpoints: Endpoints,
    ) -> Result<KimiLoginStart, String> {
        self.start_login(home, endpoints, None, None).await
    }

    async fn start_login(
        &self,
        home: PathBuf,
        endpoints: Endpoints,
        account_id: Option<String>,
        oauth: Option<Value>,
    ) -> Result<KimiLoginStart, String> {
        let mut pending = self.pending.lock().await;
        // Starting again always invalidates the previous flow before any I/O.
        *pending = None;
        let identity = device_id(&home)?;
        let response = request(&endpoints, "/api/oauth/device_authorization", &identity)
            .form(&[("client_id", CLIENT_ID)])
            .send()
            .await
            .map_err(|e| {
                network_error(
                    "Kimi authorization request failed",
                    &endpoints.oauth_host,
                    e,
                )
            })?;
        let (status, payload) = response_json(response).await?;
        if status != 200 {
            return Err(format!("Kimi device authorization failed (HTTP {status})"));
        }
        let device_code = required_string(&payload, "device_code")?.to_string();
        let user_code = required_string(&payload, "user_code")?.to_string();
        let uri = payload["verification_uri_complete"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| payload["verification_uri"].as_str())
            .ok_or("Kimi response missing verification URI")?;
        let url = url::Url::parse(uri).map_err(|_| "Invalid Kimi verification URL")?;
        if url.scheme() != "https" || url.host_str().is_none() {
            return Err("Invalid Kimi verification URL".into());
        }
        let expires_in = seconds(&payload, "expires_in", Some(600))?;
        let interval = seconds(&payload, "interval", Some(5))?;
        let id = uuid::Uuid::new_v4().to_string();
        let target = match (account_id, oauth) {
            (Some(account_id), Some(oauth)) => Some((account_id, oauth)),
            (None, None) => None,
            _ => return Err("Missing Kimi account OAuth reference".into()),
        };
        *pending = Some(PendingLogin {
            polling: false,
            id: id.clone(),
            target,
            identity: None,
            store: self.store(),
            device_code,
            expires: Instant::now() + Duration::from_secs(expires_in),
            next_poll: Instant::now() + Duration::from_secs(interval),
            interval,
            home,
            endpoints,
            device_id: identity,
            credential: None,
        });
        Ok(KimiLoginStart {
            flow_id: id,
            user_code,
            verification_uri: uri.to_string(),
            expires_in,
            interval,
        })
    }

    pub async fn poll(&self, flow_id: &str) -> Result<KimiLoginPoll, String> {
        let mut flow = {
            let mut pending = self.pending.lock().await;
            let Some(flow) = pending.as_mut().filter(|p| p.id == flow_id) else {
                return Ok(KimiLoginPoll::ended("cancelled"));
            };
            if Instant::now() >= flow.expires {
                *pending = None;
                return Ok(KimiLoginPoll::ended("expired"));
            }
            if flow.polling || Instant::now() < flow.next_poll {
                return Ok(KimiLoginPoll::pending(flow.interval));
            }
            flow.polling = true;
            flow.clone()
        };
        // No lock across HTTP: cancellation immediately removes the flow, even
        // while token/model requests are in flight. Check its identity again
        // before committing anything to disk.
        let outcome = Self::advance(&mut flow).await;
        let mut pending = self.pending.lock().await;
        if pending.as_ref().is_none_or(|p| p.id != flow_id) {
            return Ok(KimiLoginPoll::ended("cancelled"));
        }
        flow.polling = false;
        match outcome {
            Ok(PollOutcome::Ready(models)) => {
                if let Err(error) = Self::commit(&flow, models).await {
                    *pending = Some(flow);
                    return Err(error);
                }
                *pending = None;
                Ok(KimiLoginPoll {
                    authenticated: true,
                    interval: 0,
                    ended: None,
                })
            }
            Ok(PollOutcome::Ended(reason)) => {
                *pending = None;
                Ok(KimiLoginPoll::ended(reason))
            }
            Ok(PollOutcome::Pending) => {
                let interval = flow.interval;
                *pending = Some(flow);
                Ok(KimiLoginPoll::pending(interval))
            }
            Err(error) => {
                // Preserve the token after a successful exchange, so a retry
                // only repeats discovery/persistence, never the device grant.
                *pending = Some(flow);
                Err(error)
            }
        }
    }

    async fn advance(flow: &mut PendingLogin) -> Result<PollOutcome, String> {
        flow.next_poll = Instant::now() + Duration::from_secs(flow.interval);
        if flow.credential.is_none() {
            let response = request(&flow.endpoints, "/api/oauth/token", &flow.device_id)
                .form(&[
                    ("client_id", CLIENT_ID),
                    ("device_code", flow.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await
                .map_err(|e| {
                    network_error("Kimi token request failed", &flow.endpoints.oauth_host, e)
                })?;
            let (status, payload) = response_json(response).await?;
            if status == 200 {
                flow.credential = Some(parse_credential(&payload)?);
            } else {
                match payload["error"].as_str() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => {
                        flow.interval += 5;
                        flow.next_poll = Instant::now() + Duration::from_secs(flow.interval);
                    }
                    Some("expired_token") => return Ok(PollOutcome::Ended("expired")),
                    Some("access_denied") => return Ok(PollOutcome::Ended("denied")),
                    _ => return Err(format!("Kimi token request failed (HTTP {status})")),
                }
                return Ok(PollOutcome::Pending);
            }
        }
        let credential = flow.credential.as_ref().unwrap();
        if flow.identity.is_none() {
            // Best effort: without an identity the login still works, it just
            // cannot be matched to an account that is already signed in.
            let access = required_string(credential, "access_token")?;
            flow.identity =
                account::fetch_identity(&flow.endpoints.base_url, access, &flow.device_id)
                    .await
                    .ok();
            if flow.identity.is_some() {
                account::identify_accounts(&flow.home, &flow.store).await;
            }
        }
        let response = identify(
            crate::proxy::http_client::get().get(format!("{}/models", flow.endpoints.base_url)),
            &flow.device_id,
        )
        .bearer_auth(required_string(credential, "access_token")?)
        .send()
        .await
        .map_err(|e| network_error("Kimi model discovery failed", &flow.endpoints.base_url, e))?;
        let (status, payload) = response_json(response).await?;
        if status != 200 {
            return Err(format!("Kimi model discovery failed (HTTP {status})"));
        }
        Ok(PollOutcome::Ready(parse_models(&payload)?))
    }

    pub async fn cancel(&self, flow_id: &str) -> bool {
        let mut pending = self.pending.lock().await;
        if pending.as_ref().is_some_and(|p| p.id == flow_id) {
            *pending = None;
            true
        } else {
            false
        }
    }

    /// Save a finished login under Kimi's credential lock, then remember who
    /// signed in. Runs with the pending flow locked, so it cannot race a new start.
    async fn commit(flow: &PendingLogin, models: Map<String, Value>) -> Result<(), String> {
        let (account_id, oauth) = resolve_target(
            &flow.home,
            &flow.store,
            &flow.endpoints,
            flow.target.clone(),
            flow.identity.as_ref(),
        )?;
        let credential = flow.credential.as_ref().unwrap();
        let path = credential_path(&flow.home, &oauth)?;
        let nickname = flow
            .identity
            .as_ref()
            .and_then(|identity| identity.nickname.as_deref());
        let slot = account::slot_name(&path);
        let models = scope_models(models, &account_id, &slot, nickname);
        let _lock = lock::acquire(&flow.home, &slot).await?;
        kimi_config::save_oauth_login(
            &flow.home,
            &account_id,
            &path,
            &serde_json::to_vec_pretty(credential).map_err(|e| e.to_string())?,
            &flow.endpoints.base_url,
            oauth,
            models,
        )
        .map_err(|e| e.to_string())?;
        if let Some(identity) = &flow.identity {
            let identity = account::Identity {
                token: account::token_marker(credential),
                ..identity.clone()
            };
            // The login is saved either way; a cache failure only loses the label.
            let _ = account::remember_login(&flow.store, &account_id, identity);
        }
        Ok(())
    }

    /// Remove an account from the Authorization Center. Separate accounts lose
    /// their provider, model aliases, credential and saved profile
    /// (`delete_profile`). Kimi's own default login is only hidden from the
    /// list: CC Switch never logs Kimi out.
    pub async fn remove(
        &self,
        account_id: &str,
        delete_profile: impl FnOnce() -> Result<(), crate::error::AppError>,
    ) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        if pending
            .as_ref()
            .and_then(|flow| flow.target.as_ref())
            .is_some_and(|(id, _)| id == account_id)
        {
            *pending = None;
        }
        remove_at(
            &kimi_config::data_dir(),
            &self.store(),
            account_id,
            delete_profile,
        )
        .await
    }

    pub async fn use_account(&self, account_id: &str) -> Result<(), String> {
        let _pending = self.pending.lock().await;
        let home = kimi_config::data_dir();
        let config = read_config(&home)?;
        let provider = account_provider(&config, account_id)?;
        if !account_status(
            &home,
            account_id,
            account_region(provider),
            &provider["oauth"],
            None,
        )?
        .authenticated
        {
            return Err("Sign in to Kimi Code first".into());
        }
        kimi_config::use_oauth_account(&home, account_id).map_err(|e| e.to_string())
    }
}

async fn models_at(
    home: &Path,
    store: &Path,
    account_id: &str,
) -> Result<Map<String, Value>, String> {
    let device = device_id(home)?;
    let config = read_config(home)?;
    let provider = account_provider(&config, account_id)?;
    let base_url = provider["base_url"]
        .as_str()
        .ok_or("Kimi account has no base URL")?;
    let token = match account::access_token(home, provider, &device).await? {
        account::Access::Token(token) => token,
        account::Access::Expired => {
            return Err("Use this account once in Kimi Code, then try again".into())
        }
        account::Access::NeedsReauth => {
            return Err("Sign in to this Kimi account again in the Authorization Center".into())
        }
    };
    let response = identify(
        crate::proxy::http_client::get().get(format!("{base_url}/models")),
        &device,
    )
    .bearer_auth(token)
    .send()
    .await
    .map_err(|e| network_error("Kimi model discovery failed", base_url, e))?;
    let (status, payload) = response_json(response).await?;
    if status != 200 {
        return Err(format!("Kimi model discovery failed (HTTP {status})"));
    }
    let slot = account::slot_name(&credential_path(home, &provider["oauth"])?);
    let nickname = account::Store::load(store)
        .accounts
        .get(account_id)
        .and_then(|identity| identity.nickname.clone());
    Ok(scope_models(
        parse_models(&payload)?,
        account_id,
        &slot,
        nickname.as_deref(),
    ))
}

async fn remove_at(
    home: &Path,
    store: &Path,
    account_id: &str,
    delete_profile: impl FnOnce() -> Result<(), crate::error::AppError>,
) -> Result<(), String> {
    let config = read_config(home)?;
    let provider = account_provider(&config, account_id)?;
    if account_id == PROVIDER_ID {
        let known = account::Store::load(store);
        let key = known
            .accounts
            .get(account_id)
            .map(|identity| identity.user_id.clone())
            .unwrap_or_else(|| PROVIDER_ID.into());
        return account::update_store(store, |store| {
            if !store.dismissed.contains(&key) {
                store.dismissed.push(key);
            }
        });
    }
    let path = credential_path(home, &provider["oauth"])?;
    let name = account::slot_name(&path);
    // Only CC Switch's own slots are deleted; a hand-written node pointing at
    // Kimi's default slot must not log Kimi out.
    let owned = name.starts_with("kimi-code-ccs-");
    let _lock = if owned {
        lock::acquire(home, &name).await?
    } else {
        None
    };
    kimi_config::remove_oauth_account(home, account_id, || {
        delete_profile()?;
        if owned {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(crate::error::AppError::io(&path, e)),
            }
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    account::update_store(store, |store| {
        store.accounts.remove(account_id);
    })
}

/// Nicknames of Kimi official accounts, keyed by provider id, for card names.
pub(crate) fn account_nicknames() -> std::collections::HashMap<String, String> {
    account::Store::load(&account::store_path())
        .accounts
        .into_iter()
        .filter_map(|(id, identity)| identity.nickname.map(|nickname| (id, nickname)))
        .collect()
}

/// Switching to an official account card needs its login on this machine; a
/// profile synced from another device would otherwise point Kimi at nothing.
pub(crate) fn ensure_profile_signed_in(settings: &Value) -> Result<(), crate::error::AppError> {
    if !kimi_config::is_official_provider(settings) {
        return Ok(());
    }
    account::ensure_signed_in(&kimi_config::data_dir(), &settings["provider"]["oauth"]).map_err(
        |_| {
            crate::error::AppError::localized(
                "kimi.account.not_signed_in",
                "这个 Kimi 账号没有在本机登录，请到授权中心登录后再切换",
                "This Kimi account is not signed in on this computer; sign in from the Authorization Center first",
            )
        },
    )
}

/// After deleting a profile whose node Kimi no longer has, also delete the
/// credential of a CC Switch-owned slot so it does not linger on disk.
pub(crate) fn forget_removed_profile(id: &str, settings: &Value) -> Result<(), String> {
    if !kimi_config::is_official_provider(settings) {
        return Ok(());
    }
    account::remove_separate_credential_blocking(
        &kimi_config::data_dir(),
        &settings["provider"]["oauth"],
    )?;
    account::update_store(&account::store_path(), |store| {
        store.accounts.remove(id);
    })
}

/// Move discovered models under the account. Separate accounts also name
/// themselves in `display_name`, so Kimi's model picker can tell them apart.
/// Point the catalog at `account_id`. Separate accounts' aliases are prefixed
/// with their credential slot, not the provider name: the slot never changes,
/// so `default_model` stays a plain ASCII key.
fn scope_models(
    models: Map<String, Value>,
    account_id: &str,
    slot: &str,
    nickname: Option<&str>,
) -> Map<String, Value> {
    models
        .into_iter()
        .map(|(key, mut model)| {
            model["provider"] = json!(account_id);
            if account_id == PROVIDER_ID {
                return (key, model);
            }
            let id = model["model"].as_str().unwrap().to_string();
            if let Some(nickname) = nickname.map(str::trim).filter(|name| !name.is_empty()) {
                let name = model["display_name"].as_str().unwrap_or(&id).to_string();
                model["display_name"] = json!(format!("{name} ({nickname})"));
            }
            (format!("{slot}/{id}"), model)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::StatusCode,
        routing::{get, post},
        Form, Json, Router,
    };
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::sync::Notify;

    #[derive(Default)]
    struct MockState {
        mode: AtomicUsize,
        tokens: AtomicUsize,
        models: AtomicUsize,
        /// Which account the next device login belongs to (`/me` → `user-N`).
        user: AtomicUsize,
        refreshes: AtomicUsize,
        /// 0: refresh succeeds, 1: refresh token rejected, 2: identity lookup fails.
        account_mode: AtomicUsize,
        discovery_started: Notify,
        resume: Notify,
    }

    struct Server {
        state: Arc<MockState>,
        endpoints: Endpoints,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server(mode: usize) -> Server {
        let state = Arc::new(MockState::default());
        state.mode.store(mode, Ordering::SeqCst);
        let router = Router::new()
            .route("/api/oauth/device_authorization", post(|Form(form): Form<HashMap<String, String>>| async move {
                assert_eq!(form["client_id"], CLIENT_ID);
                Json(json!({"device_code":"private-device-code", "user_code":"ABCD-EFGH",
                    "verification_uri_complete":"https://auth.kimi.com/verify?user_code=ABCD-EFGH", "expires_in":600,"interval":1}))
            }))
            .route("/api/oauth/token", post(|State(state): State<Arc<MockState>>, Form(form): Form<HashMap<String, String>>| async move {
                assert_eq!(form["client_id"], CLIENT_ID);
                if form["grant_type"] == "refresh_token" {
                    let n = state.refreshes.fetch_add(1, Ordering::SeqCst);
                    if state.account_mode.load(Ordering::SeqCst) == 1 {
                        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_grant"})));
                    }
                    assert!(form["refresh_token"].starts_with("private-refresh"));
                    return (StatusCode::OK, Json(json!({"access_token":format!("refreshed-access-{n}"),
                        "refresh_token":format!("private-refresh-rotated-{n}"), "expires_in":900})));
                }
                assert_eq!(form["grant_type"], "urn:ietf:params:oauth:grant-type:device_code");
                assert_eq!(form["device_code"], "private-device-code");
                let attempt = state.tokens.fetch_add(1, Ordering::SeqCst);
                let error = match (state.mode.load(Ordering::SeqCst), attempt) {
                    (1, 0) => Some("authorization_pending"), (1, 1) => Some("slow_down"),
                    (2, _) => Some("access_denied"), (4, _) => Some("expired_token"), _ => None,
                };
                if let Some(error) = error { (StatusCode::BAD_REQUEST, Json(json!({"error":error}))) }
                else {
                    let user = state.user.load(Ordering::SeqCst);
                    (StatusCode::OK, Json(json!({"access_token":format!("private-access-{user}"),
                        "refresh_token":format!("private-refresh-{user}"), "expires_in":3600})))
                }
            }))
            .route("/me", get(|State(state): State<Arc<MockState>>, headers: axum::http::HeaderMap| async move {
                if state.account_mode.load(Ordering::SeqCst) == 2 {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({})));
                }
                let auth = headers["authorization"].to_str().unwrap().to_string();
                let user = auth.rsplit('-').next().unwrap().to_string();
                (StatusCode::OK, Json(json!({"user_id":format!("user-{user}"), "nickname":format!("Nick{user}"), "phone":"private-phone"})))
            }))
            .route("/usages", get(|| async {
                Json(json!({"usage":{"limit":"100","remaining":"40","resetTime":"2030-01-01T00:00:00Z"}}))
            }))
            .route("/models", get(|State(state): State<Arc<MockState>>| async move {
                let attempt = state.models.fetch_add(1, Ordering::SeqCst);
                if state.mode.load(Ordering::SeqCst) == 3 {
                    state.discovery_started.notify_one();
                    state.resume.notified().await;
                }
                if state.mode.load(Ordering::SeqCst) == 5 && attempt == 0 {
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"temporary"})));
                }
                (StatusCode::OK, Json(json!({"data":[{"id":"k3","limit":{"context":1048576},"protocol":"response",
                    "reasoning":{"type":"only","effort":{"valid":["max"],"default":"max"}},"modalities":{"input":["text","image"]}}]})))
            }))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Server {
            state,
            endpoints: Endpoints {
                oauth_host: host.clone(),
                base_url: host,
            },
            task,
        }
    }

    async fn due(manager: &KimiOAuthManager) {
        manager.pending.lock().await.as_mut().unwrap().next_poll = Instant::now();
    }

    #[test]
    fn selected_region_uses_fixed_endpoints_even_with_process_overrides() {
        struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
        let keys = [
            "KIMI_CODE_OAUTH_HOST",
            "KIMI_OAUTH_HOST",
            "KIMI_CODE_BASE_URL",
        ];
        let _restore = RestoreEnv(
            keys.iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        );
        for key in keys {
            std::env::set_var(key, "https://override.example/wrong-region");
        }
        let dir = tempfile::tempdir().unwrap();
        for (region, oauth_host, base_url) in [
            (
                KimiRegion::MainlandCn,
                "https://auth.kimi.com",
                "https://api.kimi.com/coding/v1",
            ),
            (
                KimiRegion::Global,
                "https://auth.kimi.ai",
                "https://api.kimi.ai/coding/v1",
            ),
        ] {
            let (endpoints, oauth) = login_target(dir.path(), region, None).unwrap();
            assert_eq!(endpoints.oauth_host, oauth_host);
            assert_eq!(endpoints.base_url, base_url);
            assert!(oauth.is_none());
        }
    }

    #[test]
    fn reauthentication_keeps_account_region_and_slot_with_fixed_official_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let config = json!({"providers":{
            "cn":{"type":"kimi","base_url":"https://api.kimi.com/coding/v1","oauth":{"storage":"file","key":"oauth/account-cn","oauth_host":"https://old-cn.example"}},
            "global":{"type":"kimi","base_url":"https://api.kimi.ai/coding/v1","oauth":{"storage":"file","key":"oauth/account-global","oauth_host":"https://old-global.example"}}
        }});
        let path = dir.path().join("config.toml");
        fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        for (id, selected, host, base, key) in [
            (
                "cn",
                KimiRegion::Global,
                "https://auth.kimi.com",
                "https://api.kimi.com/coding/v1",
                "oauth/account-cn",
            ),
            (
                "global",
                KimiRegion::MainlandCn,
                "https://auth.kimi.ai",
                "https://api.kimi.ai/coding/v1",
                "oauth/account-global",
            ),
        ] {
            let (endpoints, oauth) = login_target(dir.path(), selected, Some(id)).unwrap();
            assert_eq!(endpoints.oauth_host, host);
            assert_eq!(endpoints.base_url, base);
            let oauth = oauth.unwrap();
            assert_eq!(oauth["key"], key);
            assert_eq!(oauth["oauth_host"], host);
        }
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[tokio::test]
    async fn network_failure_reports_its_cause_without_url_credentials() {
        use std::error::Error;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let endpoint = format!("http://{address}/authorize?token=private-credential");
        let error = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .post(&endpoint)
            .send()
            .await
            .unwrap_err();
        let cause = error.source().unwrap().to_string();
        let detail = network_error("Kimi authorization request failed", &endpoint, error);
        assert!(detail.contains(&cause), "{detail}");
        assert!(detail.contains(&address.to_string()), "{detail}");
        assert!(!detail.contains("private-credential"));
        assert!(!detail.contains("/authorize"));
    }

    #[test]
    fn first_login_uses_native_slot_and_later_accounts_preserve_it() {
        let dir = tempfile::tempdir().unwrap();
        let endpoints = KimiRegion::MainlandCn.endpoints();
        let native_oauth = json!({"storage":"file","key":"oauth/kimi-code"});
        assert_eq!(endpoints.oauth_ref(), native_oauth);
        assert_eq!(
            new_account(dir.path(), &endpoints, "first", None).unwrap(),
            (PROVIDER_ID.into(), native_oauth.clone())
        );

        let path = credential_path(dir.path(), &native_oauth).unwrap();
        atomic_write_private(&path, b"existing-native-credential").unwrap();
        // An orphaned credential does not prevent restoring the native provider.
        assert_eq!(
            new_account(dir.path(), &endpoints, "restore", None)
                .unwrap()
                .0,
            PROVIDER_ID
        );
        let config = json!({"providers":{PROVIDER_ID:{"type":"kimi","base_url":endpoints.base_url,"oauth":native_oauth}}});
        fs::write(
            dir.path().join("config.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
        let original = fs::read(dir.path().join("config.toml")).unwrap();
        let (id, oauth) = new_account(dir.path(), &endpoints, "second", None).unwrap();
        assert_eq!(id, "kimi-code-ccs-second");
        assert_eq!(oauth["key"], "oauth/kimi-code-ccs-second");
        assert_eq!(fs::read(&path).unwrap(), b"existing-native-credential");
        // Choosing another region must not replace the occupied native account.
        assert_eq!(
            new_account(dir.path(), &KimiRegion::Global.endpoints(), "global", None)
                .unwrap()
                .0,
            "kimi-code-ccs-global"
        );

        fs::remove_file(&path).unwrap();
        assert_eq!(
            new_account(dir.path(), &endpoints, "missing-token", None).unwrap(),
            (PROVIDER_ID.into(), native_oauth)
        );
        assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), original);
    }

    #[test]
    fn readable_provider_id_follows_kimi_name_rules() {
        let label = "Kimi 订阅";
        let config =
            json!({"providers": {"Kimi 订阅 登月者9287": {}, "Kimi 订阅 登月者9287 2": {}}});
        // Kimi only accepts letters, digits, "-", "_" and spaces in a provider name.
        assert_eq!(
            readable_provider_id(&json!({}), label, Some("  麦田里的守望 ")).as_deref(),
            Some("Kimi 订阅 麦田里的守望")
        );
        assert_eq!(
            readable_provider_id(&json!({}), "Kimi Subscription", Some("john.doe@x")).as_deref(),
            Some("Kimi Subscription john doe x")
        );
        assert_eq!(
            readable_provider_id(&config, label, Some("登月者9287")).as_deref(),
            Some("Kimi 订阅 登月者9287 3")
        );
        assert_eq!(readable_provider_id(&json!({}), label, Some("..")), None);
        assert_eq!(readable_provider_id(&json!({}), label, None), None);
    }

    #[test]
    fn global_native_slot_matches_kimi_environment_key() {
        assert_eq!(
            KimiRegion::Global.endpoints().oauth_ref(),
            json!({
                "storage":"file", "key":"oauth/kimi-code-env-0e4f99c69cc27850", "oauth_host":"https://auth.kimi.ai"
            })
        );
    }

    async fn login_as(manager: &KimiOAuthManager, server: &Server, dir: &Path, user: usize) {
        server.state.user.store(user, Ordering::SeqCst);
        let flow = manager
            .start_with(dir.into(), server.endpoints.clone())
            .await
            .unwrap();
        due(manager).await;
        assert!(manager.poll(&flow.flow_id).await.unwrap().authenticated);
    }

    fn store_of(dir: &Path) -> account::Store {
        account::Store::load(&dir.join("accounts.json"))
    }

    #[tokio::test]
    async fn multiple_logins_isolate_credentials_models_reauthentication_and_removal() {
        let server = server(0).await;
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("accounts.json");
        let manager = KimiOAuthManager::with_store(store.clone());
        login_as(&manager, &server, dir.path(), 1).await;
        login_as(&manager, &server, dir.path(), 2).await;
        let status = accounts_status(dir.path(), &store).unwrap();
        assert_eq!(status.accounts.len(), 2);
        let first = status.current_account_id.as_ref().unwrap();
        assert_eq!(first, PROVIDER_ID);
        let second = &status
            .accounts
            .iter()
            .find(|account| &account.id != first)
            .unwrap()
            .id;
        // Kimi shows the provider key as its name, so a separate account is
        // named after its nickname; the credential slot and model aliases stay
        // derived from the account and never change.
        let suffix = account::account_suffix(KimiRegion::MainlandCn, "user-2");
        let slot = format!("kimi-code-ccs-{suffix}");
        assert_eq!(second, &format!("{} Nick2", subscription_label()));
        let named = |id: &str| {
            status
                .accounts
                .iter()
                .find(|account| account.id == id)
                .unwrap()
        };
        assert_eq!(named(first).name, "Kimi Official · Nick1");
        assert!(named(first).primary);
        assert_eq!(named(second).nickname.as_deref(), Some("Nick2"));
        assert!(!named(second).primary);
        assert!(!serde_json::to_string(&status).unwrap().contains("private-"));
        assert!(!fs::read_to_string(&store).unwrap().contains("private-"));
        let config = read_config(dir.path()).unwrap();
        assert_eq!(config["models"]["kimi-code/k3"]["provider"], first.as_str());
        assert_eq!(
            config["providers"][second]["oauth"]["key"],
            format!("oauth/{slot}")
        );
        let second_model = &config["models"][format!("{slot}/k3")];
        assert_eq!(second_model["provider"], second.as_str());
        assert_eq!(second_model["display_name"], "k3 (Nick2)");
        let first_path = credential_path(dir.path(), &config["providers"][first]["oauth"]).unwrap();
        let second_path =
            credential_path(dir.path(), &config["providers"][second]["oauth"]).unwrap();
        assert_ne!(first_path, second_path);
        let first_bytes = fs::read(&first_path).unwrap();
        // No lock directory is left behind once a login is saved.
        assert!(!dir
            .path()
            .join(format!("oauth/kimi-code-ccs-{suffix}.lock"))
            .exists());

        kimi_config::use_oauth_account(dir.path(), second).unwrap();
        let selected = read_config(dir.path()).unwrap();
        assert_eq!(selected["default_model"], format!("{slot}/k3"));
        assert_eq!(selected["services"], config["services"]);

        // Signing in to the same account again (without asking for a re-login)
        // updates it in place instead of adding a duplicate slot.
        login_as(&manager, &server, dir.path(), 2).await;
        assert_eq!(
            accounts_status(dir.path(), &store).unwrap().accounts.len(),
            2
        );
        // The same account is found by its credential slot even without the
        // local identity cache (e.g. cleared), so it is not added twice.
        account::update_store(&store, |store| {
            store.accounts.remove(second.as_str());
        })
        .unwrap();
        login_as(&manager, &server, dir.path(), 2).await;
        let ids: Vec<_> = accounts_status(dir.path(), &store)
            .unwrap()
            .accounts
            .into_iter()
            .map(|account| account.id)
            .collect();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);

        // The account in use cannot be removed.
        let error = remove_at(dir.path(), &store, second, || Ok(()))
            .await
            .unwrap_err();
        assert!(
            error.contains("in use") || error.contains("正在使用"),
            "{error}"
        );
        kimi_config::use_oauth_account(dir.path(), first).unwrap();
        // Web search pointed at the account being removed moves to Kimi's own
        // login instead of keeping a reference to a deleted credential.
        let mut pointed = read_config(dir.path()).unwrap();
        pointed["services"]["moonshot_search"] = json!({"base_url": "https://example.test/search", "api_key": "", "oauth": pointed["providers"][second]["oauth"].clone()});
        pointed["services"]["moonshot_fetch"] = json!({"base_url": "https://example.test/fetch", "api_key": "", "oauth": pointed["providers"][first]["oauth"].clone()});
        fs::write(
            dir.path().join("config.toml"),
            toml::to_string(&pointed).unwrap(),
        )
        .unwrap();

        let mut profile_deleted = false;
        remove_at(dir.path(), &store, second, || {
            profile_deleted = true;
            Ok(())
        })
        .await
        .unwrap();
        assert!(profile_deleted);
        assert!(!second_path.exists());
        let after = read_config(dir.path()).unwrap();
        assert!(after["providers"].get(second).is_none());
        assert!(after["models"].get(format!("{slot}/k3")).is_none());
        assert!(!store_of(dir.path()).accounts.contains_key(second));
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
        assert_eq!(
            after["services"]["moonshot_search"]["oauth"],
            after["providers"][first]["oauth"]
        );
        assert_eq!(
            after["services"]["moonshot_search"]["base_url"],
            "https://example.test/search"
        );
        assert_eq!(
            after["services"]["moonshot_fetch"]["oauth"],
            after["providers"][first]["oauth"]
        );
        assert!(!accounts_status(dir.path(), &store).unwrap().primary_hidden);

        // Kimi's own default login is only hidden, never logged out.
        let before = fs::read(dir.path().join("config.toml")).unwrap();
        remove_at(dir.path(), &store, first, || panic!("profile must stay"))
            .await
            .unwrap();
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
        assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), before);
        assert!(accounts_status(dir.path(), &store)
            .unwrap()
            .accounts
            .is_empty());
        assert_eq!(store_of(dir.path()).dismissed, vec!["user-1".to_string()]);
        assert!(accounts_status(dir.path(), &store).unwrap().primary_hidden);
        // Kimi refreshing its own token (same account) keeps it hidden...
        let known = store_of(dir.path()).accounts[first].clone();
        let rotated = account::Identity {
            token: Some("rotated".into()),
            ..known.clone()
        };
        account::remember(&store, first, rotated).unwrap();
        assert!(store_of(dir.path()).is_dismissed(first));
        // ...while another account taking over the slot shows again.
        let other = account::Identity {
            user_id: "user-9".into(),
            ..known.clone()
        };
        account::remember(&store, first, other).unwrap();
        assert!(!store_of(dir.path()).is_dismissed(first));
        account::remember(&store, first, known).unwrap();
        assert!(store_of(dir.path()).is_dismissed(first));
        // Signing in to it again through CC Switch brings it back.
        fs::remove_file(&first_path).unwrap();
        login_as(&manager, &server, dir.path(), 1).await;
        let status = accounts_status(dir.path(), &store).unwrap();
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(status.accounts[0].id, PROVIDER_ID);
        assert!(!status.primary_hidden);
        assert!(remove_at(dir.path(), &store, "missing", || Ok(()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn relogin_with_a_different_account_never_overwrites_the_target() {
        let server = server(0).await;
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("accounts.json");
        let manager = KimiOAuthManager::with_store(store.clone());
        login_as(&manager, &server, dir.path(), 1).await;
        let config = read_config(dir.path()).unwrap();
        let native_path =
            credential_path(dir.path(), &config["providers"][PROVIDER_ID]["oauth"]).unwrap();
        let native = fs::read(&native_path).unwrap();
        // "Sign in again" on account 1, but the browser is signed in as 2.
        server.state.user.store(2, Ordering::SeqCst);
        let flow = manager
            .start_login(
                dir.path().into(),
                server.endpoints.clone(),
                Some(PROVIDER_ID.into()),
                Some(config["providers"][PROVIDER_ID]["oauth"].clone()),
            )
            .await
            .unwrap();
        due(&manager).await;
        assert!(manager.poll(&flow.flow_id).await.unwrap().authenticated);
        assert_eq!(fs::read(&native_path).unwrap(), native);
        assert_eq!(
            accounts_status(dir.path(), &store).unwrap().accounts.len(),
            2
        );
    }

    #[tokio::test]
    async fn login_without_identity_still_saves_the_account() {
        let server = server(0).await;
        server.state.account_mode.store(2, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("accounts.json");
        let manager = KimiOAuthManager::with_store(store.clone());
        login_as(&manager, &server, dir.path(), 1).await;
        let status = accounts_status(dir.path(), &store).unwrap();
        assert_eq!(status.accounts[0].name, "Kimi Official");
        assert!(status.accounts[0].nickname.is_none());
    }

    fn separate_account(server: &Server, dir: &Path, credential: Value) -> Value {
        let oauth = json!({"storage":"file","key":"oauth/kimi-code-ccs-test","oauth_host":server.endpoints.oauth_host});
        let models =
            parse_models(&json!({"data":[{"id":"k3","limit":{"context":1048576}}]})).unwrap();
        kimi_config::save_oauth_login(
            dir,
            "kimi-code-ccs-test",
            &dir.join("credentials/kimi-code-ccs-test.json"),
            &serde_json::to_vec(&credential).unwrap(),
            &server.endpoints.base_url,
            oauth,
            scope_models(models, "kimi-code-ccs-test", "kimi-code-ccs-test", None),
        )
        .unwrap();
        read_config(dir).unwrap()["providers"]["kimi-code-ccs-test"].clone()
    }

    #[tokio::test]
    async fn model_catalog_for_the_editor_uses_the_accounts_slot_and_saves_nothing() {
        let server = server(0).await;
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("accounts.json");
        let valid = json!({"access_token":"a","refresh_token":"r","expires_at":4_102_444_800i64});
        separate_account(&server, dir.path(), valid);
        account::remember(
            &store,
            "kimi-code-ccs-test",
            account::Identity {
                user_id: "u".into(),
                nickname: Some("Nick".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let before = fs::read(dir.path().join("config.toml")).unwrap();
        let catalog = models_at(dir.path(), &store, "kimi-code-ccs-test")
            .await
            .unwrap();
        let model = &catalog["kimi-code-ccs-test/k3"];
        assert_eq!(model["provider"], "kimi-code-ccs-test");
        assert_eq!(model["display_name"], "k3 (Nick)");
        assert_eq!(model["max_context_size"], 1048576);
        assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), before);
        assert!(models_at(dir.path(), &store, "custom").await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn quota_refreshes_an_expired_token_under_kimis_lock() {
        let server = server(0).await;
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("accounts.json");
        let expired = json!({"access_token":"private-access-old","refresh_token":"private-refresh-old",
            "expires_at":1,"expires_in":900,"scope":"kimi","token_type":"Bearer"});
        separate_account(&server, dir.path(), expired);
        let path = dir.path().join("credentials/kimi-code-ccs-test.json");

        let quota = account::quota(dir.path(), &store, "kimi-code-ccs-test", "device")
            .await
            .unwrap();
        assert!(quota.success, "{quota:?}");
        assert_eq!(quota.tool, "kimi_oauth");
        assert_eq!(server.state.refreshes.load(Ordering::SeqCst), 1);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["refresh_token"], "private-refresh-rotated-0");
        assert!(saved["expires_at"].as_i64().unwrap() > chrono::Utc::now().timestamp());
        assert!(!dir.path().join("oauth/kimi-code-ccs-test.lock").exists());

        // A fresh token is used as is.
        account::quota(dir.path(), &store, "kimi-code-ccs-test", "device")
            .await
            .unwrap();
        assert_eq!(server.state.refreshes.load(Ordering::SeqCst), 1);

        // A card synced from another device: no node, or a node without a login here.
        let quota = account::quota(dir.path(), &store, "kimi-code-ccs-elsewhere", "device")
            .await
            .unwrap();
        assert!(matches!(
            quota.credential_status,
            crate::services::subscription::CredentialStatus::NotFound
        ));
        fs::remove_file(&path).unwrap();
        let quota = account::quota(dir.path(), &store, "kimi-code-ccs-test", "device")
            .await
            .unwrap();
        assert!(matches!(
            quota.credential_status,
            crate::services::subscription::CredentialStatus::NotFound
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejected_refresh_writes_kimis_tombstone_unless_another_process_rotated_it() {
        let server = server(0).await;
        server.state.account_mode.store(1, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let expired = json!({"access_token":"private-access-old","refresh_token":"private-refresh-old",
            "expires_at":1,"expires_in":900,"scope":"kimi","token_type":"Bearer"});
        let provider = separate_account(&server, dir.path(), expired);
        let path = dir.path().join("credentials/kimi-code-ccs-test.json");
        assert!(matches!(
            account::access_token(dir.path(), &provider, "device")
                .await
                .unwrap(),
            account::Access::NeedsReauth
        ));
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["access_token"], "");
        assert_eq!(saved["refresh_token"], "");
        assert_eq!(saved["scope"], "kimi");
        let status = accounts_status(dir.path(), &dir.path().join("accounts.json")).unwrap();
        assert!(!status.accounts[0].authenticated);
        assert!(account::ensure_signed_in(dir.path(), &provider["oauth"]).is_err());
        assert!(account::ensure_signed_in(dir.path(), &json!({"key":"oauth/missing"})).is_err());

        // Kimi rotated the token while our request was in flight: adopt its
        // token instead of revoking the login.
        let expired = json!({"access_token":"private-access-old","refresh_token":"private-refresh-old",
            "expires_at":1,"expires_in":900});
        fs::write(&path, serde_json::to_vec(&expired).unwrap()).unwrap();
        let state = server.state.clone();
        let rotated = path.clone();
        let other = tokio::spawn(async move {
            while state.refreshes.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let token = json!({"access_token":"other-access","refresh_token":"private-refresh-other",
                "expires_at":chrono::Utc::now().timestamp() + 900,"expires_in":900});
            fs::write(&rotated, serde_json::to_vec(&token).unwrap()).unwrap();
        });
        match account::access_token(dir.path(), &provider, "device")
            .await
            .unwrap()
        {
            account::Access::Token(token) => assert_eq!(token, "other-access"),
            _ => panic!("expected the token rotated by the other process"),
        }
        other.await.unwrap();
        assert!(account::ensure_signed_in(dir.path(), &provider["oauth"]).is_ok());
    }

    #[test]
    fn only_cc_switch_slots_are_deleted_with_a_removed_profile() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["kimi-code", "kimi-code-ccs-abc"] {
            atomic_write_private(&dir.path().join(format!("credentials/{name}.json")), b"{}")
                .unwrap();
            account::remove_separate_credential_blocking(
                dir.path(),
                &json!({"storage":"file","key":format!("oauth/{name}")}),
            )
            .unwrap();
        }
        assert!(dir.path().join("credentials/kimi-code.json").exists());
        assert!(!dir
            .path()
            .join("credentials/kimi-code-ccs-abc.json")
            .exists());
    }

    #[test]
    fn native_account_and_other_accounts_remain_visible_when_one_credential_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        for (id, key) in [
            (PROVIDER_ID, "kimi-code"),
            ("kimi-code-ccs-new", "kimi-code-ccs-new"),
        ] {
            let models =
                parse_models(&json!({"data":[{"id":"k3","limit":{"context":1048576}}]})).unwrap();
            kimi_config::save_oauth_login(
                dir.path(), id, &dir.path().join(format!("credentials/{key}.json")),
                br#"{"access_token":"private-access","refresh_token":"private-refresh","expires_at":1}"#,
                "https://api.kimi.com/coding/v1", json!({"storage":"file","key":format!("oauth/{key}")}),
                scope_models(models, id, id, None),
            ).unwrap();
        }
        fs::write(
            dir.path().join("credentials/kimi-code-ccs-new.json"),
            "invalid credential",
        )
        .unwrap();
        let status = accounts_status(dir.path(), &dir.path().join("accounts.json")).unwrap();
        assert_eq!(status.accounts.len(), 2);
        assert_eq!(status.current_account_id.as_deref(), Some(PROVIDER_ID));
        let native = status
            .accounts
            .iter()
            .find(|account| account.id == PROVIDER_ID)
            .unwrap();
        assert!(native.authenticated && native.error.is_none());
        let broken = status
            .accounts
            .iter()
            .find(|account| account.id != PROVIDER_ID)
            .unwrap();
        assert!(!broken.authenticated && broken.error.is_some());
        assert!(!serde_json::to_string(&status).unwrap().contains("private-"));
    }

    #[test]
    fn anthropic_models_write_beta_api_and_clear_it_after_protocol_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut payload = json!({"data": [{
            "id": "k3", "limit": {"context": 1048576},
            "protocol": "anthropic", "reasoning": {"type": "both"}
        }]});
        let save_models = |payload: &Value| {
            kimi_config::save_oauth_login(
                dir.path(),
                PROVIDER_ID,
                &dir.path().join("credentials/kimi-code.json"),
                b"test-credential",
                "https://api.kimi.com/coding/v1",
                json!({"storage": "file", "key": "oauth/kimi-code"}),
                parse_models(payload).unwrap(),
            )
            .unwrap();
        };

        save_models(&payload);
        let native = fs::read_to_string(&path).unwrap();
        let config: Value = toml::from_str(&native).unwrap();
        let model = &config["models"]["kimi-code/k3"];
        assert_eq!(model["protocol"], "anthropic");
        assert_eq!(model["beta_api"], true);
        assert_eq!(model["adaptive_thinking"], true);
        assert!(model.get("use_beta").is_none());
        assert!(native.contains("beta_api = true"));

        payload["data"][0]["protocol"] = json!("response");
        save_models(&payload);
        let config: Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        let model = &config["models"]["kimi-code/k3"];
        assert_eq!(model["protocol"], "openai_responses");
        assert!(model.get("beta_api").is_none());
        assert!(model.get("adaptive_thinking").is_none());
    }

    #[tokio::test]
    async fn official_device_flow_obeys_interval_slowdown_and_provisions_native_login() {
        let server = server(1).await;
        let dir = tempfile::tempdir().unwrap();
        let manager = KimiOAuthManager::with_store(dir.path().join("accounts.json"));
        let start = manager
            .start_with(dir.path().into(), server.endpoints.clone())
            .await
            .unwrap();
        assert!(!serde_json::to_string(&start)
            .unwrap()
            .contains("private-device-code"));
        assert_eq!(start.user_code, "ABCD-EFGH");
        assert!(!manager.poll(&start.flow_id).await.unwrap().authenticated);
        assert_eq!(server.state.tokens.load(Ordering::SeqCst), 0);
        due(&manager).await;
        assert_eq!(manager.poll(&start.flow_id).await.unwrap().interval, 1);
        due(&manager).await;
        assert_eq!(manager.poll(&start.flow_id).await.unwrap().interval, 6);
        assert_eq!(manager.poll(&start.flow_id).await.unwrap().interval, 6);
        assert_eq!(server.state.tokens.load(Ordering::SeqCst), 2);
        due(&manager).await;
        assert!(manager.poll(&start.flow_id).await.unwrap().authenticated);
        let status = accounts_status(dir.path(), &dir.path().join("accounts.json")).unwrap();
        let account = &status.accounts[0];
        assert!(account.authenticated);
        assert!(!serde_json::to_string(&status).unwrap().contains("private-"));
        let credential: Value = serde_json::from_slice(
            &fs::read(credential_path(dir.path(), &server.endpoints.oauth_ref()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(credential["refresh_token"], "private-refresh-0");
        let config: Value =
            toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
        assert_eq!(
            config["models"]["kimi-code/k3"]["protocol"],
            "openai_responses"
        );
        assert_eq!(account.id, PROVIDER_ID);
        assert_eq!(config["default_model"], "kimi-code/k3");
        assert_eq!(
            config["services"]["moonshot_search"]["oauth"],
            server.endpoints.oauth_ref()
        );
    }

    #[tokio::test]
    async fn cancellation_during_model_discovery_prevents_all_login_writes() {
        let server = server(3).await;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(KimiOAuthManager::with_store(
            dir.path().join("accounts.json"),
        ));
        let start = manager
            .start_with(dir.path().into(), server.endpoints.clone())
            .await
            .unwrap();
        due(&manager).await;
        let poller = manager.clone();
        let id = start.flow_id.clone();
        let task = tokio::spawn(async move { poller.poll(&id).await });
        server.state.discovery_started.notified().await;
        assert!(manager.cancel(&start.flow_id).await);
        server.state.resume.notify_one();
        assert_eq!(task.await.unwrap().unwrap().ended, Some("cancelled"));
        assert!(!dir.path().join("config.toml").exists());
        assert!(!dir.path().join("credentials").exists());
    }

    #[tokio::test]
    async fn retry_after_model_failure_does_not_redeem_device_grant_twice() {
        let server = server(5).await;
        let dir = tempfile::tempdir().unwrap();
        let manager = KimiOAuthManager::with_store(dir.path().join("accounts.json"));
        let start = manager
            .start_with(dir.path().into(), server.endpoints.clone())
            .await
            .unwrap();
        due(&manager).await;
        assert!(manager
            .poll(&start.flow_id)
            .await
            .unwrap_err()
            .contains("503"));
        assert!(!dir.path().join("config.toml").exists());
        due(&manager).await;
        assert!(manager.poll(&start.flow_id).await.unwrap().authenticated);
        assert_eq!(server.state.tokens.load(Ordering::SeqCst), 1);
        assert_eq!(server.state.models.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn denied_expired_and_replaced_flows_do_not_save_credentials() {
        for (mode, reason) in [(2, "denied"), (4, "expired")] {
            let server = server(mode).await;
            let dir = tempfile::tempdir().unwrap();
            let manager = KimiOAuthManager::with_store(dir.path().join("accounts.json"));
            let old = manager
                .start_with(dir.path().into(), server.endpoints.clone())
                .await
                .unwrap();
            let start = manager
                .start_with(dir.path().into(), server.endpoints.clone())
                .await
                .unwrap();
            assert_eq!(
                manager.poll(&old.flow_id).await.unwrap().ended,
                Some("cancelled")
            );
            due(&manager).await;
            assert_eq!(
                manager.poll(&start.flow_id).await.unwrap().ended,
                Some(reason)
            );
            assert!(!dir.path().join("credentials").exists());
        }
    }

    #[test]
    fn scoped_global_credentials_match_official_client_and_reject_path_traversal() {
        let home = Path::new("kimi");
        for key in [
            "../outside",
            "oauth/../../outside",
            "oauth/.hidden",
            "C:\\outside",
        ] {
            assert!(credential_path(home, &json!({"key":key})).is_err());
        }
        assert_eq!(
            credential_path(home, &json!({"key":"oauth/kimi-code"})).unwrap(),
            home.join("credentials/kimi-code.json")
        );
        assert!(parse_credential(&json!({"access_token":"x", "expires_in":3600})).is_err());
        assert!(parse_models(&json!({"data":[{"id":"bad","limit":{"context":0}}]})).is_err());
    }

    #[test]
    fn existing_expired_access_with_refresh_is_signed_in_and_tombstone_is_signed_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials/kimi-code.json");
        atomic_write_private(
            &path,
            br#"{"access_token":"old","refresh_token":"refresh","expires_at":1}"#,
        )
        .unwrap();
        assert!(
            account_status(
                dir.path(),
                PROVIDER_ID,
                KimiRegion::MainlandCn,
                &json!({"key":"oauth/kimi-code"}),
                None,
            )
            .unwrap()
            .authenticated
        );
        atomic_write_private(
            &path,
            br#"{"access_token":"","refresh_token":"","expires_at":0}"#,
        )
        .unwrap();
        assert!(
            !account_status(
                dir.path(),
                PROVIDER_ID,
                KimiRegion::MainlandCn,
                &json!({"key":"oauth/kimi-code"}),
                None,
            )
            .unwrap()
            .authenticated
        );
    }
}
