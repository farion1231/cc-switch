//! Official Kimi Code device login. Credentials stay in Kimi's native file
//! store; Kimi owns refresh-token rotation and subsequent model requests.
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{config::atomic_write_private, kimi_config};

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiOAuthAccount {
    pub id: String,
    pub name: String,
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
    account_id: String,
    oauth: Value,
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
    Ok(KimiOAuthAccount {
        id: id.into(),
        name: kimi_config::official_provider_name(id),
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

fn new_account(home: &Path, endpoints: &Endpoints, id: &str) -> Result<(String, Value), String> {
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
        let id = format!("kimi-code-ccs-{id}");
        let oauth = json!({"storage": "file", "key": format!("oauth/{id}"), "oauth_host": endpoints.oauth_host});
        Ok((id, oauth))
    } else {
        Ok((PROVIDER_ID.into(), native_oauth))
    }
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

fn network_error(context: &str, endpoint: &str, error: reqwest::Error) -> String {
    format!(
        "{context} ({}): {}",
        crate::proxy::http_client::mask_url(endpoint),
        crate::proxy::error::describe_reqwest_error(error)
    )
}

fn accounts_status(home: &Path) -> Result<KimiOAuthStatus, String> {
    let config = read_config(home)?;
    let mut accounts = Vec::new();
    for (id, provider) in config
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if !kimi_config::is_official_provider(&json!({"provider": provider})) {
            continue;
        }
        let region = account_region(provider);
        accounts.push(
            account_status(home, id, region, &provider["oauth"]).unwrap_or_else(|error| {
                KimiOAuthAccount {
                    id: id.clone(),
                    name: kimi_config::official_provider_name(id),
                    authenticated: false,
                    region,
                    expires_at: None,
                    error: Some(error),
                }
            }),
        );
    }
    let current_account_id = config["default_model"]
        .as_str()
        .and_then(|key| config["models"][key]["provider"].as_str())
        .filter(|id| accounts.iter().any(|account| account.id == *id))
        .map(str::to_owned);
    Ok(KimiOAuthStatus {
        accounts,
        current_account_id,
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
    pub fn status(&self) -> Result<KimiOAuthStatus, String> {
        accounts_status(&kimi_config::data_dir())
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
        let (account_id, oauth) = match (account_id, oauth) {
            (Some(account_id), Some(oauth)) => (account_id, oauth),
            (None, None) => new_account(&home, &endpoints, &id)?,
            _ => return Err("Missing Kimi account OAuth reference".into()),
        };
        *pending = Some(PendingLogin {
            polling: false,
            id: id.clone(),
            account_id,
            oauth,
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
                let credential = flow.credential.as_ref().unwrap();
                let oauth = flow.oauth.clone();
                let path = credential_path(&flow.home, &oauth)?;
                let models = scope_models(models, &flow.account_id);
                let result = kimi_config::save_oauth_login(
                    &flow.home,
                    &flow.account_id,
                    &path,
                    &serde_json::to_vec_pretty(credential).map_err(|e| e.to_string())?,
                    &flow.endpoints.base_url,
                    oauth,
                    models,
                )
                .map_err(|e| e.to_string());
                if let Err(error) = result {
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

    pub async fn logout(&self, account_id: &str) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        if pending
            .as_ref()
            .is_some_and(|flow| flow.account_id == account_id)
        {
            *pending = None;
        }
        logout_at(&kimi_config::data_dir(), account_id)
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
        )?
        .authenticated
        {
            return Err("Sign in to Kimi Code first".into());
        }
        kimi_config::use_oauth_account(&home, account_id).map_err(|e| e.to_string())
    }
}

fn logout_at(home: &Path, account_id: &str) -> Result<(), String> {
    let config = read_config(home)?;
    let provider = account_provider(&config, account_id)?;
    let path = credential_path(home, &provider["oauth"])?;
    // Leave the provider and model references available for reauthentication.
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Cannot remove Kimi Code credentials: {e}")),
    }
}

fn scope_models(models: Map<String, Value>, account_id: &str) -> Map<String, Value> {
    models
        .into_iter()
        .map(|(key, mut model)| {
            model["provider"] = json!(account_id);
            let key = if account_id == PROVIDER_ID {
                key
            } else {
                format!("{account_id}/{}", model["model"].as_str().unwrap())
            };
            (key, model)
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
                assert_eq!(form["grant_type"], "urn:ietf:params:oauth:grant-type:device_code");
                assert_eq!(form["device_code"], "private-device-code");
                let attempt = state.tokens.fetch_add(1, Ordering::SeqCst);
                let error = match (state.mode.load(Ordering::SeqCst), attempt) {
                    (1, 0) => Some("authorization_pending"), (1, 1) => Some("slow_down"),
                    (2, _) => Some("access_denied"), (4, _) => Some("expired_token"), _ => None,
                };
                if let Some(error) = error { (StatusCode::BAD_REQUEST, Json(json!({"error":error}))) }
                else { (StatusCode::OK, Json(json!({"access_token":"private-access", "refresh_token":"private-refresh", "expires_in":3600}))) }
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
            new_account(dir.path(), &endpoints, "first").unwrap(),
            (PROVIDER_ID.into(), native_oauth.clone())
        );

        let path = credential_path(dir.path(), &native_oauth).unwrap();
        atomic_write_private(&path, b"existing-native-credential").unwrap();
        // An orphaned credential does not prevent restoring the native provider.
        assert_eq!(
            new_account(dir.path(), &endpoints, "restore").unwrap().0,
            PROVIDER_ID
        );
        let config = json!({"providers":{PROVIDER_ID:{"type":"kimi","base_url":endpoints.base_url,"oauth":native_oauth}}});
        fs::write(
            dir.path().join("config.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
        let original = fs::read(dir.path().join("config.toml")).unwrap();
        let (id, oauth) = new_account(dir.path(), &endpoints, "second").unwrap();
        assert_eq!(id, "kimi-code-ccs-second");
        assert_eq!(oauth["key"], "oauth/kimi-code-ccs-second");
        assert_eq!(fs::read(&path).unwrap(), b"existing-native-credential");
        // Choosing another region must not replace the occupied native account.
        assert_eq!(
            new_account(dir.path(), &KimiRegion::Global.endpoints(), "global")
                .unwrap()
                .0,
            "kimi-code-ccs-global"
        );

        fs::remove_file(&path).unwrap();
        assert_eq!(
            new_account(dir.path(), &endpoints, "missing-token").unwrap(),
            (PROVIDER_ID.into(), native_oauth)
        );
        assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), original);
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

    #[tokio::test]
    async fn multiple_logins_isolate_credentials_models_reauthentication_and_logout() {
        let server = server(0).await;
        let dir = tempfile::tempdir().unwrap();
        let manager = KimiOAuthManager::default();
        for _ in 0..2 {
            let flow = manager
                .start_with(dir.path().into(), server.endpoints.clone())
                .await
                .unwrap();
            due(&manager).await;
            assert!(manager.poll(&flow.flow_id).await.unwrap().authenticated);
        }
        let status = accounts_status(dir.path()).unwrap();
        assert_eq!(status.accounts.len(), 2);
        let first = status.current_account_id.as_ref().unwrap();
        let second = &status
            .accounts
            .iter()
            .find(|account| &account.id != first)
            .unwrap()
            .id;
        assert_eq!(first, PROVIDER_ID);
        assert!(second.starts_with("kimi-code-ccs-"));
        assert_ne!(first, second);
        assert_eq!(status.current_account_id.as_ref(), Some(first));
        let config = read_config(dir.path()).unwrap();
        for (id, model) in [
            (first, "kimi-code/k3".to_string()),
            (second, format!("{second}/k3")),
        ] {
            assert!(config["models"][model]["provider"] == id.as_str());
        }
        let first_path = credential_path(dir.path(), &config["providers"][first]["oauth"]).unwrap();
        let second_path =
            credential_path(dir.path(), &config["providers"][second]["oauth"]).unwrap();
        assert_ne!(first_path, second_path);
        let first_bytes = fs::read(&first_path).unwrap();
        kimi_config::use_oauth_account(dir.path(), second).unwrap();
        let selected = read_config(dir.path()).unwrap();
        assert_eq!(selected["default_model"], format!("{second}/k3"));
        assert_eq!(selected["services"], config["services"]);
        let flow = manager
            .start_login(
                dir.path().into(),
                server.endpoints.clone(),
                Some(second.clone()),
                Some(config["providers"][second]["oauth"].clone()),
            )
            .await
            .unwrap();
        due(&manager).await;
        assert!(manager.poll(&flow.flow_id).await.unwrap().authenticated);
        assert_eq!(accounts_status(dir.path()).unwrap().accounts.len(), 2);
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
        let mut custom = read_config(dir.path()).unwrap();
        custom["services"]["moonshot_fetch"]["base_url"] = json!("https://custom.example/fetch");
        fs::write(
            dir.path().join("config.toml"),
            toml::to_string(&custom).unwrap(),
        )
        .unwrap();
        kimi_config::use_oauth_account(dir.path(), first).unwrap();
        let selected = read_config(dir.path()).unwrap();
        assert_eq!(
            selected["services"]["moonshot_fetch"],
            custom["services"]["moonshot_fetch"]
        );
        assert_eq!(selected["services"], custom["services"]);
        let before = fs::read(dir.path().join("config.toml")).unwrap();
        logout_at(dir.path(), second).unwrap();
        assert_eq!(fs::read(&first_path).unwrap(), first_bytes);
        assert!(!second_path.exists());
        assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), before);
        let status = accounts_status(dir.path()).unwrap();
        assert!(
            status
                .accounts
                .iter()
                .find(|account| &account.id == first)
                .unwrap()
                .authenticated
        );
        assert!(
            !status
                .accounts
                .iter()
                .find(|account| &account.id == second)
                .unwrap()
                .authenticated
        );
        assert!(logout_at(dir.path(), "missing").is_err());
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
                scope_models(models, id),
            ).unwrap();
        }
        fs::write(
            dir.path().join("credentials/kimi-code-ccs-new.json"),
            "invalid credential",
        )
        .unwrap();
        let status = accounts_status(dir.path()).unwrap();
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
        let manager = KimiOAuthManager::default();
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
        let status = accounts_status(dir.path()).unwrap();
        let account = &status.accounts[0];
        assert!(account.authenticated);
        assert!(!serde_json::to_string(&status).unwrap().contains("private-"));
        let credential: Value = serde_json::from_slice(
            &fs::read(credential_path(dir.path(), &server.endpoints.oauth_ref()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(credential["refresh_token"], "private-refresh");
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
        let manager = Arc::new(KimiOAuthManager::default());
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
        let manager = KimiOAuthManager::default();
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
            let manager = KimiOAuthManager::default();
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
                &json!({"key":"oauth/kimi-code"})
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
                &json!({"key":"oauth/kimi-code"})
            )
            .unwrap()
            .authenticated
        );
    }
}
