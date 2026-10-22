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
    fn with_env(mut self) -> Result<Self, String> {
        if let Some(host) = std::env::var("KIMI_CODE_OAUTH_HOST")
            .ok()
            .or_else(|| std::env::var("KIMI_OAUTH_HOST").ok())
        {
            self.oauth_host = host.trim_end_matches('/').into();
        }
        if let Ok(base) = std::env::var("KIMI_CODE_BASE_URL") {
            self.base_url = base.trim_end_matches('/').into();
        }
        for endpoint in [&self.oauth_host, &self.base_url] {
            let url =
                url::Url::parse(endpoint).map_err(|_| "Invalid Kimi Code endpoint override")?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err("Invalid Kimi Code endpoint override".into());
            }
        }
        Ok(self)
    }

    fn oauth_ref(&self) -> Value {
        // Match resolveKimiCodeOAuthKey in @moonshot-ai/kimi-code-oauth,
        // including JSON property order and the 16-digit environment digest.
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
    pub authenticated: bool,
    pub region: KimiRegion,
    pub expires_at: Option<i64>,
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

fn native_login(home: &Path) -> Result<(KimiRegion, Value), String> {
    let path = home.join("config.toml");
    let config = match fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<Value>(&text)
            .map_err(|_| "Invalid Kimi Code TOML configuration".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("Cannot read Kimi Code configuration: {e}")),
    };
    let provider = &config["providers"][PROVIDER_ID];
    let global = provider["base_url"]
        .as_str()
        .is_some_and(|url| url.starts_with("https://api.kimi.ai/"))
        || provider["oauth"]["oauth_host"].as_str() == Some("https://auth.kimi.ai");
    let region = if global
        || (provider.is_null()
            && fs::read_to_string(home.join("region")).is_ok_and(|s| s.trim() == "global"))
    {
        KimiRegion::Global
    } else {
        KimiRegion::MainlandCn
    };
    let has_env = [
        "KIMI_CODE_OAUTH_HOST",
        "KIMI_OAUTH_HOST",
        "KIMI_CODE_BASE_URL",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some());
    let oauth = if has_env {
        let mut endpoints = region.endpoints();
        if let Some(base) = provider["base_url"].as_str() {
            endpoints.base_url = base.into();
        }
        endpoints.with_env()?.oauth_ref()
    } else {
        provider
            .get("oauth")
            .cloned()
            .unwrap_or_else(|| region.endpoints().oauth_ref())
    };
    Ok((region, oauth))
}

fn local_status(home: &Path) -> Result<KimiOAuthStatus, String> {
    let (region, oauth) = native_login(home)?;
    let path = credential_path(home, &oauth)?;
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
    Ok(KimiOAuthStatus {
        authenticated: can_refresh
            || (has_access && expires_at.is_some_and(|t| t > chrono::Utc::now().timestamp())),
        region,
        expires_at,
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
        local_status(&kimi_config::data_dir())
    }

    pub async fn start(&self, region: KimiRegion) -> Result<KimiLoginStart, String> {
        self.start_with(kimi_config::data_dir(), region.endpoints().with_env()?)
            .await
    }

    async fn start_with(
        &self,
        home: PathBuf,
        endpoints: Endpoints,
    ) -> Result<KimiLoginStart, String> {
        let mut pending = self.pending.lock().await;
        // Starting again always invalidates the previous flow before any I/O.
        *pending = None;
        let identity = device_id(&home)?;
        let response = request(&endpoints, "/api/oauth/device_authorization", &identity)
            .form(&[("client_id", CLIENT_ID)])
            .send()
            .await
            .map_err(|e| format!("Kimi authorization request failed: {e}"))?;
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
        *pending = Some(PendingLogin {
            polling: false,
            id: id.clone(),
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
                let oauth = flow.endpoints.oauth_ref();
                let path = credential_path(&flow.home, &oauth)?;
                let result = kimi_config::save_oauth_login(
                    &flow.home,
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
                .map_err(|e| format!("Kimi token request failed: {e}"))?;
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
        .map_err(|e| format!("Kimi model discovery failed: {e}"))?;
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

    pub async fn logout(&self) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        *pending = None;
        let home = kimi_config::data_dir();
        let (_, oauth) = native_login(&home)?;
        let path = credential_path(&home, &oauth)?;
        // Keep model selections and references intact. Kimi recognizes the
        // missing credential as signed out and can provision it on next login.
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Cannot remove Kimi Code credentials: {e}")),
        }
    }

    pub async fn use_account(&self) -> Result<(), String> {
        let _pending = self.pending.lock().await;
        let home = kimi_config::data_dir();
        if !local_status(&home)?.authenticated {
            return Err("Sign in to Kimi Code first".into());
        }
        kimi_config::use_oauth_account(&home).map_err(|e| e.to_string())
    }
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
        let status = local_status(dir.path()).unwrap();
        assert!(status.authenticated);
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
        assert_eq!(
            KimiRegion::MainlandCn.endpoints().oauth_ref()["key"],
            "oauth/kimi-code"
        );
        assert_eq!(
            KimiRegion::Global.endpoints().oauth_ref()["key"],
            "oauth/kimi-code-env-0e4f99c69cc27850"
        );
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
        assert!(local_status(dir.path()).unwrap().authenticated);
        atomic_write_private(
            &path,
            br#"{"access_token":"","refresh_token":"","expires_at":0}"#,
        )
        .unwrap();
        assert!(!local_status(dir.path()).unwrap().authenticated);
    }
}
