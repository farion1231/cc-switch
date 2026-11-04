//! Native DimAgent public-client OAuth, catalog, and account usage.
//! Credentials never cross the IPC boundary and redirects are never followed.

use axum::{
    extract::{RawQuery, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

const CALLBACK: &str = "http://127.0.0.1:54322/oauth/callback";
const LOGIN_SECONDS: u64 = 300;
const MAX_BODY: usize = 2 * 1024 * 1024;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DimSite {
    Cn,
    Global,
}
impl DimSite {
    pub fn issuer(self) -> &'static str {
        match self {
            Self::Cn => "https://dimagent.cn",
            Self::Global => "https://dimagent.com",
        }
    }
    fn client_id(self) -> &'static str {
        match self {
            Self::Cn => "271706aab9b74b15a7140b228a419e4a",
            Self::Global => "91975f99125f42bc88859100a1769ea4",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimOAuthAccount {
    pub id: String,
    pub site: DimSite,
    pub login: String,
    pub authenticated_at: i64,
    pub requires_reauth: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct DimOAuthStatus {
    pub accounts: Vec<DimOAuthAccount>,
    pub default_account_id: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DimLoginStart {
    pub flow_id: String,
    pub auth_url: String,
    pub expires_in: u64,
    pub interval: u64,
}
// Intentionally neither Serialize nor Debug: this is a Rust-only routing credential.
pub struct DimRequestAuth {
    pub access_token: String,
    pub issuer: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct DimModel {
    pub id: String,
    pub name: String,
    pub api_formats: Vec<String>,
    pub efforts: Vec<String>,
    pub zdr: bool,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DimUsageWindow {
    pub name: String,
    pub used: f64,
    pub total: f64,
    pub unit: String,
    pub resets_at: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DimUsage {
    pub plan_name: Option<String>,
    pub remaining_units: Option<f64>,
    pub windows: Vec<DimUsageWindow>,
    pub subscription_url: String,
    pub pricing_url: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedAccount {
    account: DimOAuthAccount,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    accounts: BTreeMap<String, SavedAccount>,
    default_account_id: Option<String>,
}
#[derive(Default)]
struct Memory {
    store: Store,
    dirty: bool,
    flows: HashMap<String, Arc<BrowserFlow>>,
}
#[derive(Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}
struct Registration {
    issuer: String,
    client_id: String,
    callback: String,
}
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}
#[derive(Clone)]
struct CallbackState {
    state: String,
    result: Arc<AsyncMutex<Option<Result<String>>>>,
}
struct BrowserFlow {
    site: DimSite,
    registration: Registration,
    discovery: Discovery,
    verifier: String,
    callback: CallbackState,
    deadline: tokio::time::Instant,
    task: tokio::task::AbortHandle,
    shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    join: AsyncMutex<Option<tokio::task::JoinHandle<()>>>,
    polling: AsyncMutex<()>,
}
impl BrowserFlow {
    async fn stop(&self) {
        if let Some(shutdown) = self.shutdown.lock().unwrap().take() {
            let _ = shutdown.send(());
        }
        if let Some(mut join) = self.join.lock().await.take() {
            if tokio::time::timeout(Duration::from_secs(2), &mut join)
                .await
                .is_err()
            {
                self.task.abort();
                let _ = join.await;
            }
        }
    }
}
impl Drop for BrowserFlow {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct DimOAuthManager {
    storage_path: PathBuf,
    client: reqwest::Client,
    memory: Arc<Mutex<Memory>>,
    mutation: AsyncMutex<()>,
    start: AsyncMutex<()>,
    #[cfg(test)]
    registration: Option<(String, String)>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs()
}
fn random_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn nonempty(value: &Value, field: &str) -> Result<String> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("Invalid Dim {field}"))
}
fn number(value: &Value, field: &str) -> Result<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| format!("Invalid Dim {field}"))
}
fn strings(value: &Value) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or("Invalid Dim string list")?
        .iter()
        .map(|v| nonempty(v, "list entry"))
        .collect()
}
fn endpoint(value: &str, issuer: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| "Invalid Dim endpoint")?;
    let base = Url::parse(issuer).map_err(|_| "Invalid Dim issuer")?;
    if url.origin() != base.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Dim endpoint must belong to the configured issuer".into());
    }
    Ok(url)
}

impl DimOAuthManager {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        let storage_path = data_dir.join("dim_oauth_auth.json");
        let store: Store = if storage_path.exists() {
            serde_json::from_slice(
                &fs::read(&storage_path).map_err(|_| "Cannot read Dim account storage")?,
            )
            .map_err(|_| "Invalid Dim account storage")?
        } else {
            Store::default()
        };
        for (id, saved) in &store.accounts {
            if id != &saved.account.id
                || saved.access_token.is_empty()
                || saved.refresh_token.is_empty()
            {
                return Err("Invalid Dim account storage".into());
            }
        }
        if store
            .default_account_id
            .as_ref()
            .is_some_and(|id| !store.accounts.contains_key(id))
        {
            return Err("Invalid Dim default account".into());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent("CC-Switch-DimAgent")
            .default_headers(reqwest::header::HeaderMap::from_iter([(
                reqwest::header::HeaderName::from_static("x-title"),
                reqwest::header::HeaderValue::from_static("CC Switch DimAgent"),
            )]))
            .build()
            .map_err(|_| "Cannot initialize Dim HTTP client")?;
        Ok(Self {
            storage_path,
            client,
            memory: Arc::new(Mutex::new(Memory {
                store,
                ..Memory::default()
            })),
            mutation: AsyncMutex::new(()),
            start: AsyncMutex::new(()),
            #[cfg(test)]
            registration: None,
        })
    }
    fn registration(&self, site: DimSite) -> Registration {
        #[cfg(test)]
        if let Some((issuer, callback)) = &self.registration {
            return Registration {
                issuer: issuer.clone(),
                callback: callback.clone(),
                client_id: site.client_id().into(),
            };
        }
        Registration {
            issuer: site.issuer().into(),
            client_id: site.client_id().into(),
            callback: CALLBACK.into(),
        }
    }
    pub async fn status(&self) -> DimOAuthStatus {
        let memory = self.memory.lock().unwrap();
        DimOAuthStatus {
            accounts: memory
                .store
                .accounts
                .values()
                .map(|saved| saved.account.clone())
                .collect(),
            default_account_id: memory.store.default_account_id.clone(),
        }
    }
    pub async fn default_account_id(&self) -> Option<String> {
        self.memory.lock().unwrap().store.default_account_id.clone()
    }
    pub async fn resolve_account_id(&self, account_id: Option<&str>) -> Result<String> {
        match account_id {
            Some(id) => Ok(id.to_owned()),
            None => self
                .default_account_id()
                .await
                .ok_or_else(|| "Sign in to DimAgent first".into()),
        }
    }

    // Refresh rotation must survive disk errors in memory, but no caller receives a
    // credential until the replacement is durable. Every later mutation retries it.
    fn persist(&self, memory: &mut Memory) -> Result<()> {
        if !memory.dirty {
            return Ok(());
        }
        let parent = self
            .storage_path
            .parent()
            .ok_or("Invalid Dim storage directory")?;
        fs::create_dir_all(parent).map_err(|_| "Cannot create Dim account storage")?;
        let temporary = parent.join(format!("dim_oauth_auth.{}.tmp", uuid::Uuid::new_v4()));
        let write = (|| -> std::io::Result<()> {
            let mut options = fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            let bytes = serde_json::to_vec(&memory.store)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.storage_path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        })();
        if write.is_err() {
            let _ = fs::remove_file(temporary);
            return Err("Cannot save Dim accounts; retry before using the account".into());
        }
        memory.dirty = false;
        Ok(())
    }
    pub async fn remove_account(&self, account_id: &str) -> Result<()> {
        let _mutation = self.mutation.lock().await;
        let mut memory = self.memory.lock().unwrap();
        self.persist(&mut memory)?;
        memory.store.accounts.remove(account_id);
        if memory.store.default_account_id.as_deref() == Some(account_id) {
            memory.store.default_account_id = memory.store.accounts.keys().next().cloned();
        }
        // Invalidate pending logins too: an in-flight exchange cannot restore a removed account.
        memory.flows.clear();
        memory.dirty = true;
        self.persist(&mut memory)
    }
    pub async fn set_default_account(&self, account_id: &str) -> Result<()> {
        let _mutation = self.mutation.lock().await;
        let mut memory = self.memory.lock().unwrap();
        self.persist(&mut memory)?;
        if !memory.store.accounts.contains_key(account_id) {
            return Err("Dim account was not found".into());
        }
        memory.store.default_account_id = Some(account_id.to_owned());
        memory.dirty = true;
        self.persist(&mut memory)
    }
    async fn json(&self, response: reqwest::Response, label: &str) -> Result<Value> {
        let status = response.status();
        if !status.is_success() {
            return Err(format!("Dim {label} failed (HTTP {})", status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            return Err(format!("Dim {label} response is too large"));
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| format!("Cannot read Dim {label} response"))?
        {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(format!("Dim {label} response is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let payload: Value =
            serde_json::from_slice(&bytes).map_err(|_| format!("Invalid Dim {label} response"))?;
        if !payload.is_object() || payload["success"] == false {
            return Err(format!("Invalid or refused Dim {label} response"));
        }
        Ok(payload)
    }
    async fn discovery(&self, registration: &Registration) -> Result<Discovery> {
        let response = self
            .client
            .get(format!(
                "{}/.well-known/openid-configuration",
                registration.issuer
            ))
            .send()
            .await
            .map_err(|_| "Cannot contact Dim discovery")?;
        let discovery: Discovery = serde_json::from_value(self.json(response, "discovery").await?)
            .map_err(|_| "Invalid Dim discovery metadata")?;
        if discovery.issuer.trim_end_matches('/') != registration.issuer {
            return Err("Dim discovery issuer mismatch".into());
        }
        endpoint(&discovery.authorization_endpoint, &registration.issuer)?;
        endpoint(&discovery.token_endpoint, &registration.issuer)?;
        endpoint(&discovery.userinfo_endpoint, &registration.issuer)?;
        Ok(discovery)
    }
    async fn exchange(
        &self,
        discovery: &Discovery,
        fields: &[(&str, &str)],
        previous: Option<&str>,
    ) -> Result<Tokens> {
        let response = self
            .client
            .post(&discovery.token_endpoint)
            .form(fields)
            .send()
            .await
            .map_err(|_| "Cannot contact Dim token endpoint")?;
        let status = response.status();
        if !status.is_success() {
            let body = self.json_error(response).await;
            if status == reqwest::StatusCode::UNAUTHORIZED
                || matches!(body.as_deref(), Some("invalid_grant" | "invalid_token"))
            {
                return Err("Dim authorization expired; sign in again".into());
            }
            return Err(format!(
                "Dim token exchange failed (HTTP {})",
                status.as_u16()
            ));
        }
        let raw = self.json(response, "token").await?;
        if !raw["token_type"]
            .as_str()
            .is_some_and(|v| v.eq_ignore_ascii_case("bearer"))
        {
            return Err("Dim must issue a Bearer token".into());
        }
        let lifetime = raw["expires_in"]
            .as_u64()
            .filter(|n| *n > 0 && *n < u64::MAX - now())
            .ok_or("Invalid Dim token lifetime")?;
        Ok(Tokens {
            access_token: nonempty(&raw["access_token"], "access token")?,
            refresh_token: match raw.get("refresh_token") {
                Some(value) => nonempty(value, "refresh token")?,
                None => previous
                    .map(str::to_owned)
                    .ok_or("Dim did not issue a refresh token")?,
            },
            expires_at: now() + lifetime,
        })
    }
    async fn json_error(&self, mut response: reqwest::Response) -> Option<String> {
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            if bytes.len() + chunk.len() > 8192 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&bytes)
            .ok()?
            .get("error")?
            .as_str()
            .map(str::to_owned)
    }
    pub async fn start_login(&self, site: DimSite) -> Result<DimLoginStart> {
        self.start_login_with_lifetime(site, Duration::from_secs(LOGIN_SECONDS))
            .await
    }
    async fn start_login_with_lifetime(
        &self,
        site: DimSite,
        lifetime: Duration,
    ) -> Result<DimLoginStart> {
        let _start = self.start.lock().await;
        let previous = std::mem::take(&mut self.memory.lock().unwrap().flows);
        for flow in previous.values() {
            flow.stop().await;
        }
        let registration = self.registration(site);
        let discovery = self.discovery(&registration).await?;
        let redirect = Url::parse(&registration.callback).expect("fixed callback URL");
        let listener = tokio::net::TcpListener::bind((
            std::net::Ipv4Addr::LOCALHOST,
            redirect.port().unwrap(),
        ))
        .await
        .map_err(|_| {
            "Cannot open Dim login callback on 127.0.0.1:54322; finish another login and retry"
        })?;
        let verifier = random_secret();
        let state = random_secret();
        let callback = CallbackState {
            state: state.clone(),
            result: Arc::new(AsyncMutex::new(None)),
        };
        let app = Router::new()
            .route(redirect.path(), get(callback_handler))
            .with_state(callback.clone());
        let deadline = tokio::time::Instant::now() + lifetime;
        let flow_id = uuid::Uuid::new_v4().to_string();
        let cleanup_id = flow_id.clone();
        let weak_memory = Arc::downgrade(&self.memory);
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            use std::future::IntoFuture;
            let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let deadline_expired = expired.clone();
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                tokio::select! { _ = stopped => {}, _ = tokio::time::sleep_until(deadline) => { deadline_expired.store(true, std::sync::atomic::Ordering::Release); } }
            });
            let _ =
                tokio::time::timeout_at(deadline + Duration::from_secs(2), server.into_future())
                    .await;
            if expired.load(std::sync::atomic::Ordering::Acquire) {
                if let Some(memory) = weak_memory.upgrade() {
                    memory.lock().unwrap().flows.remove(&cleanup_id);
                }
            }
        });
        let mut url = endpoint(&discovery.authorization_endpoint, &registration.issuer)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", registration.client_id.as_str()),
            ("redirect_uri", registration.callback.as_str()),
            ("scope", "openid profile email"),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        self.memory.lock().unwrap().flows.insert(
            flow_id.clone(),
            Arc::new(BrowserFlow {
                site,
                registration,
                discovery,
                verifier,
                callback,
                deadline,
                task: task.abort_handle(),
                shutdown: Mutex::new(Some(shutdown)),
                join: AsyncMutex::new(Some(task)),
                polling: AsyncMutex::new(()),
            }),
        );
        Ok(DimLoginStart {
            flow_id,
            auth_url: url.into(),
            expires_in: lifetime.as_secs(),
            interval: 1,
        })
    }
    pub async fn cancel_login(&self, flow_id: &str) -> Result<()> {
        let flow = self.memory.lock().unwrap().flows.remove(flow_id);
        if let Some(flow) = flow {
            flow.stop().await;
        }
        Ok(())
    }
    pub async fn poll_login(&self, flow_id: &str) -> Result<Option<DimOAuthAccount>> {
        let flow = self
            .memory
            .lock()
            .unwrap()
            .flows
            .get(flow_id)
            .cloned()
            .ok_or("Dim login is no longer pending")?;
        let _polling = flow.polling.lock().await;
        if tokio::time::Instant::now() >= flow.deadline {
            self.cancel_login(flow_id).await?;
            return Err("Dim login timed out".into());
        }
        let outcome = flow.callback.result.lock().await.take();
        let Some(outcome) = outcome else {
            return Ok(None);
        };
        flow.stop().await;
        let outcome = self.complete_login(&flow, outcome).await;
        let _mutation = self.mutation.lock().await;
        let mut memory = self.memory.lock().unwrap();
        if !memory
            .flows
            .get(flow_id)
            .is_some_and(|current| Arc::ptr_eq(current, &flow))
        {
            return Err("Dim login was cancelled".into());
        }
        memory.flows.remove(flow_id);
        let saved = outcome?;
        self.persist(&mut memory)?;
        let account = saved.account.clone();
        memory
            .store
            .default_account_id
            .get_or_insert_with(|| account.id.clone());
        memory.store.accounts.insert(account.id.clone(), saved);
        memory.dirty = true;
        self.persist(&mut memory)?;
        Ok(Some(account))
    }
    async fn complete_login(
        &self,
        flow: &BrowserFlow,
        code: Result<String>,
    ) -> Result<SavedAccount> {
        let code = code?;
        let tokens = self
            .exchange(
                &flow.discovery,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("client_id", &flow.registration.client_id),
                    ("redirect_uri", &flow.registration.callback),
                    ("code_verifier", &flow.verifier),
                ],
                None,
            )
            .await?;
        let response = self
            .client
            .get(&flow.discovery.userinfo_endpoint)
            .bearer_auth(&tokens.access_token)
            .send()
            .await
            .map_err(|_| "Cannot contact Dim userinfo")?;
        let user = self.json(response, "userinfo").await?;
        let subject = nonempty(&user["sub"], "user subject")?;
        let id = format!(
            "{}:{}",
            match flow.site {
                DimSite::Cn => "cn",
                DimSite::Global => "global",
            },
            subject
        );
        let login = user["email"]
            .as_str()
            .filter(|email| !email.trim().is_empty())
            .unwrap_or(&subject)
            .to_owned();
        Ok(SavedAccount {
            account: DimOAuthAccount {
                id,
                site: flow.site,
                login,
                authenticated_at: now() as i64,
                requires_reauth: false,
            },
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
        })
    }
    pub async fn get_request_auth(&self, account_id: &str) -> Result<DimRequestAuth> {
        let _mutation = self.mutation.lock().await;
        let saved = {
            let mut memory = self.memory.lock().unwrap();
            self.persist(&mut memory)?;
            memory
                .store
                .accounts
                .get(account_id)
                .cloned()
                .ok_or("Dim account was not found")?
        };
        if saved.account.requires_reauth {
            return Err("Dim authorization expired; sign in again".into());
        }
        let registration = self.registration(saved.account.site);
        if saved.expires_at > now() + 30 {
            return Ok(DimRequestAuth {
                access_token: saved.access_token,
                issuer: registration.issuer,
            });
        }
        let discovery = self.discovery(&registration).await?;
        let tokens = self
            .exchange(
                &discovery,
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &saved.refresh_token),
                    ("client_id", &registration.client_id),
                ],
                Some(&saved.refresh_token),
            )
            .await;
        let mut memory = self.memory.lock().unwrap();
        let tokens = match tokens {
            Ok(tokens) => tokens,
            Err(error) => {
                if error == "Dim authorization expired; sign in again" {
                    memory
                        .store
                        .accounts
                        .get_mut(account_id)
                        .expect("mutation lock protects account")
                        .account
                        .requires_reauth = true;
                    memory.dirty = true;
                    self.persist(&mut memory)?;
                }
                return Err(error);
            }
        };
        let saved = memory
            .store
            .accounts
            .get_mut(account_id)
            .expect("mutation lock protects account");
        saved.access_token = tokens.access_token.clone();
        saved.refresh_token = tokens.refresh_token;
        saved.expires_at = tokens.expires_at;
        memory.dirty = true;
        self.persist(&mut memory)?;
        Ok(DimRequestAuth {
            access_token: tokens.access_token,
            issuer: registration.issuer,
        })
    }
    async fn business(&self, account_id: &str, path: &str) -> Result<(Value, String)> {
        let auth = self.get_request_auth(account_id).await?;
        let response = self
            .client
            .get(format!("{}{path}", auth.issuer))
            .bearer_auth(auth.access_token)
            .send()
            .await
            .map_err(|_| "Cannot contact Dim account API")?;
        Ok((self.json(response, "account API").await?, auth.issuer))
    }
    pub async fn get_models(&self, account_id: &str) -> Result<Vec<DimModel>> {
        let (payload, _) = self.business(account_id, "/v1/models?type=dim").await?;
        parse_models(&payload)
    }
    pub async fn get_usage(&self, account_id: &str) -> Result<DimUsage> {
        let (payload, issuer) = self.business(account_id, "/api/me/usage").await?;
        parse_usage(&payload, &issuer)
    }
}

fn parse_callback(query: &str, expected_state: &str) -> Option<Result<String>> {
    let mut values = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if values
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return None;
        }
    }
    if values.get("state").map(String::as_str) != Some(expected_state) {
        return None;
    }
    match (values.get("code"), values.get("error")) {
        (Some(code), None) if !code.trim().is_empty() => Some(Ok(code.clone())),
        (None, Some(_)) => Some(Err("Dim authorization was refused".into())),
        _ => None,
    }
}
async fn callback_handler(
    State(state): State<CallbackState>,
    RawQuery(query): RawQuery,
) -> impl IntoResponse {
    let headers = [
        (header::CACHE_CONTROL, "no-store"),
        (header::REFERRER_POLICY, "no-referrer"),
    ];
    let Some(outcome) = parse_callback(query.as_deref().unwrap_or_default(), &state.state) else {
        return (
            StatusCode::BAD_REQUEST,
            headers,
            "Invalid login callback. Return to CC Switch.",
        );
    };
    let mut result = state.result.lock().await;
    if result.is_some() {
        return (
            StatusCode::CONFLICT,
            headers,
            "This callback was already received. Return to CC Switch.",
        );
    }
    *result = Some(outcome);
    (
        StatusCode::OK,
        headers,
        "Authorization received. Return to CC Switch.",
    )
}

// Model limits are optional external directory data, never guessed from model names.
fn optional_limit(value: Option<&Value>) -> Result<Option<u64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(0) => Ok(None),
            Some(value) => Ok(Some(value)),
            None => Err("Invalid Dim model limit".into()),
        },
    }
}

/// Apply an explicitly selected directory effort after protocol conversion.
pub(crate) fn apply_dim_effort(
    body: &mut serde_json::Map<String, Value>,
    format: &str,
    effort: Option<&str>,
) {
    let Some(effort) = effort else {
        return;
    };
    match format {
        "openai_chat" => {
            body.insert("reasoning_effort".into(), Value::from(effort));
        }
        "openai_responses" | "anthropic" => {
            let field = if format == "anthropic" {
                "output_config"
            } else {
                "reasoning"
            };
            let config = body.entry(field).or_insert_with(|| serde_json::json!({}));
            if !config.is_object() {
                *config = serde_json::json!({});
            }
            config
                .as_object_mut()
                .expect("effort configuration is an object")
                .insert("effort".into(), Value::from(effort));
        }
        _ => {}
    }
}

fn parse_models(payload: &Value) -> Result<Vec<DimModel>> {
    let rows = payload["data"].as_array().ok_or("Invalid Dim model list")?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for raw in rows {
        let id = nonempty(&raw["id"], "model ID")?;
        if !seen.insert(id.clone()) {
            return Err("Duplicate Dim model ID".into());
        }
        let config = match raw.get("dim") {
            Some(v) if !v.is_null() => {
                if !v.is_object() {
                    return Err("Invalid Dim model config".into());
                }
                v
            }
            _ => raw,
        };
        if config
            .get("id")
            .is_some_and(|value| value.as_str() != Some(&id))
        {
            return Err("Dim model ID mismatch".into());
        }
        if config["status"] == "unavailable" {
            continue;
        }
        let explicit = config
            .get("endpoint_type")
            .or_else(|| raw.get("endpointType"))
            .or_else(|| raw.get("endpoint_type"));
        let declared = config
            .get("selectableEndpointTypes")
            .or_else(|| config.get("selectable_endpoint_types"))
            .or_else(|| config.get("supportedEndpointTypes"))
            .or_else(|| config.get("supported_endpoint_types"));
        let endpoints = match (explicit, declared) {
            (Some(value), _) => vec![nonempty(value, "endpoint type")?],
            (_, Some(value)) => strings(value)?,
            _ => vec!["openai".into()],
        };
        let mut api_formats = Vec::new();
        for endpoint in endpoints {
            let format = match endpoint.as_str() {
                "openai" => "openai_chat",
                "openai-response" => "openai_responses",
                "anthropic" => "anthropic",
                "gemini" => "gemini_native",
                _ => continue,
            };
            if !api_formats.iter().any(|v| v == format) {
                api_formats.push(format.into());
            }
        }
        if api_formats.is_empty() {
            continue;
        }
        let name = config
            .get("displayName")
            .or_else(|| config.get("display_name"))
            .or_else(|| config.get("name"))
            .or_else(|| raw.get("display_name"))
            .or_else(|| raw.get("name"))
            .map(|v| nonempty(v, "model name"))
            .transpose()?
            .unwrap_or_else(|| id.clone());
        if config
            .get("limit")
            .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err("Invalid Dim model limits".into());
        }
        let reasoning = config
            .get("extra_capabilities")
            .and_then(|v| v.get("reasoning"))
            .or_else(|| config.get("reasoning"));
        let mut effort_values =
            reasoning.and_then(|v| v.get("effortOptions").or_else(|| v.get("effort_options")));
        if effort_values.is_none() {
            if let Some(options) = config.get("reasoning_options") {
                let options = options.as_array().ok_or("Invalid Dim reasoning options")?;
                effort_values = options
                    .iter()
                    .find(|v| v["type"] == "effort")
                    .and_then(|v| v.get("values"));
            }
        }
        let disabled = reasoning
            .is_some_and(|v| v == false || v.get("supported") == Some(&Value::Bool(false)));
        let efforts = if disabled {
            Vec::new()
        } else {
            effort_values.map(strings).transpose()?.unwrap_or_default()
        };
        models.push(DimModel {
            id,
            name,
            api_formats,
            efforts,
            zdr: raw["zdr"] == true,
            context_window: optional_limit(
                config
                    .get("context_window")
                    .or_else(|| config.get("contextWindow"))
                    .or_else(|| config.get("limit").and_then(|v| v.get("context"))),
            )?,
            max_output_tokens: optional_limit(
                config
                    .get("max_output_tokens")
                    .or_else(|| config.get("maxOutputTokens"))
                    .or_else(|| config.get("limit").and_then(|v| v.get("output"))),
            )?,
        });
    }
    Ok(models)
}
fn parse_usage(payload: &Value, issuer: &str) -> Result<DimUsage> {
    let data = payload["data"]
        .as_object()
        .ok_or("Invalid Dim usage data")?;
    let credits = data
        .get("credits")
        .and_then(Value::as_object)
        .ok_or("Invalid Dim credits")?;
    let mut windows = Vec::new();
    if let Some(bucket) = credits.get("subscription_bucket").filter(|v| !v.is_null()) {
        if !bucket.is_object() {
            return Err("Invalid Dim subscription bucket".into());
        }
        if bucket["unlimited"] != true {
            if let (Some(used), Some(total)) = (bucket.get("used_units"), bucket.get("total_units"))
            {
                let used = number(used, "subscription used")?;
                let total = number(total, "subscription total")?;
                if total > 0.0 {
                    windows.push(DimUsageWindow {
                        name: "Subscription".into(),
                        used,
                        total,
                        unit: "units".into(),
                        resets_at: optional_date(bucket.get("expires_at"))?,
                    });
                }
            }
        }
        let legacy;
        let states = match bucket.get("window_states") {
            Some(value) => value.as_array().ok_or("Invalid Dim usage windows")?,
            None => {
                let keys = [
                    "window_duration_hours",
                    "window_token_cap",
                    "window_token_used",
                    "window_started_at",
                    "window_expires_at",
                ];
                if keys
                    .iter()
                    .all(|key| bucket.get(key).is_none_or(|v| v.is_null() || v == 0))
                {
                    legacy = Vec::new();
                } else {
                    if keys.iter().any(|key| bucket.get(key).is_none()) {
                        return Err("Incomplete Dim legacy window".into());
                    }
                    legacy = vec![bucket.clone()];
                }
                &legacy
            }
        };
        for state in states {
            let total = number(&state["window_token_cap"], "window total")?;
            let used = number(&state["window_token_used"], "window used")?;
            let hours = number(&state["window_duration_hours"], "window duration")?;
            if total > 0.0 {
                windows.push(DimUsageWindow {
                    name: format!("{hours}h window"),
                    used,
                    total,
                    unit: "tokens".into(),
                    resets_at: optional_date(state.get("window_expires_at"))?,
                });
            }
        }
    }
    if let Some(meters) = data.get("feature_meters") {
        for meter in meters.as_array().ok_or("Invalid Dim feature meters")? {
            if meter["unlimited"] == true {
                continue;
            }
            if let (Some(used), Some(total)) =
                (meter.get("total_used"), meter.get("total_allowance"))
            {
                let total = number(total, "feature total")?;
                if total > 0.0 {
                    windows.push(DimUsageWindow {
                        name: nonempty(&meter["feature_key"], "feature name")?,
                        used: number(used, "feature used")?,
                        total,
                        unit: meter
                            .get("unit")
                            .map(|v| nonempty(v, "feature unit"))
                            .transpose()?
                            .unwrap_or_else(|| "units".into()),
                        resets_at: optional_date(meter.get("period_end"))?,
                    });
                }
            }
        }
    }
    let plan_name = data
        .get("subscription")
        .and_then(|v| v.get("product"))
        .and_then(|v| v.get("name"))
        .map(|v| nonempty(v, "plan name"))
        .transpose()?;
    let remaining_units = if credits
        .get("subscription_bucket")
        .is_some_and(|bucket| bucket["unlimited"] == true)
    {
        None
    } else {
        credits
            .get("remaining_units")
            .map(|v| number(v, "remaining units"))
            .transpose()?
    };
    Ok(DimUsage {
        plan_name,
        remaining_units,
        windows,
        subscription_url: format!("{issuer}/console/subscription"),
        pricing_url: format!("{issuer}/console/pricing"),
    })
}
fn optional_date(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let text = nonempty(value, "reset date")?;
            chrono::DateTime::parse_from_rfc3339(&text).map_err(|_| "Invalid Dim reset date")?;
            Ok(Some(text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Form, routing::post, Json};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone)]
    struct Fixture {
        issuer: String,
        challenge: Arc<Mutex<String>>,
        refreshes: Arc<AtomicUsize>,
        reject_refresh: Arc<AtomicBool>,
        hold_exchange: Arc<AtomicBool>,
        exchange_started: Arc<tokio::sync::Notify>,
        release_exchange: Arc<tokio::sync::Notify>,
    }
    struct Server {
        fixture: Fixture,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn fixture() -> Server {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixture = Fixture {
            issuer: format!("http://{}", listener.local_addr().unwrap()),
            challenge: Arc::new(Mutex::new(String::new())),
            refreshes: Arc::new(AtomicUsize::new(0)),
            reject_refresh: Arc::new(AtomicBool::new(false)),
            hold_exchange: Arc::new(AtomicBool::new(false)),
            exchange_started: Arc::new(tokio::sync::Notify::new()),
            release_exchange: Arc::new(tokio::sync::Notify::new()),
        };
        async fn discovery(State(s): State<Fixture>) -> Json<Value> {
            Json(
                json!({"issuer":s.issuer,"authorization_endpoint":format!("{}/authorize",s.issuer),"token_endpoint":format!("{}/token",s.issuer),"userinfo_endpoint":format!("{}/userinfo",s.issuer)}),
            )
        }
        async fn token(
            State(s): State<Fixture>,
            Form(fields): Form<HashMap<String, String>>,
        ) -> (StatusCode, Json<Value>) {
            if fields["grant_type"] == "authorization_code" {
                let actual =
                    URL_SAFE_NO_PAD.encode(Sha256::digest(fields["code_verifier"].as_bytes()));
                assert_eq!(actual, *s.challenge.lock().unwrap());
                assert_eq!(fields["code"], "fixture-code");
                if s.hold_exchange.load(Ordering::SeqCst) {
                    s.exchange_started.notify_one();
                    s.release_exchange.notified().await;
                }
            } else {
                s.refreshes.fetch_add(1, Ordering::SeqCst);
                assert_eq!(fields["refresh_token"], "fixture-refresh-1");
                if s.reject_refresh.load(Ordering::SeqCst) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(
                            json!({"error":"invalid_grant","error_description":"Never expose this upstream content"}),
                        ),
                    );
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let refresh = if fields["grant_type"] == "authorization_code" {
                "fixture-refresh-1"
            } else {
                "fixture-refresh-2"
            };
            (
                StatusCode::OK,
                Json(
                    json!({"token_type":"Bearer","access_token":"fixture-access","refresh_token":refresh,"expires_in":3600}),
                ),
            )
        }
        let app = Router::new().route("/.well-known/openid-configuration", get(discovery)).route("/token", post(token)).route("/userinfo", get(|| async { Json(json!({"sub":"subject","uid":"different-uid","email":"test@example.invalid"})) })).route("/v1/models", get(|| async { Json(json!({"data":[{"id":"vendor/full-id","dim":null,"zdr_supported":true},{"id":"reasoner","zdr":true,"dim":{"endpoint_type":"openai-response","reasoning_options":[{"type":"effort","values":["high","max"]}]}},{"id":"off","dim":{"endpoint_type":"unsupported"}}]})) })).route("/api/me/usage", get(|| async { Json(json!({"data":{"credits":{"remaining_units":80,"subscription_bucket":{"total_units":100,"used_units":30,"window_states":[{"window_duration_hours":5,"window_token_cap":200,"window_token_used":10,"window_expires_at":"2030-01-01T00:00:00Z"}]}},"subscription":{"product":{"name":"Fixture plan"}},"feature_meters":[{"feature_key":"search","total_used":1,"total_allowance":10,"unit":"calls"}]}})) })).with_state(fixture.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Server { fixture, task }
    }
    async fn manager(dir: &std::path::Path, fixture: &Fixture) -> DimOAuthManager {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let callback = format!("http://{}/oauth/callback", listener.local_addr().unwrap());
        drop(listener);
        let mut manager = DimOAuthManager::new(dir.to_path_buf()).unwrap();
        manager.registration = Some((fixture.issuer.clone(), callback));
        manager
    }
    async fn login(manager: &DimOAuthManager, fixture: &Fixture, site: DimSite) -> DimOAuthAccount {
        let start = manager.start_login(site).await.unwrap();
        let url = Url::parse(&start.auth_url).unwrap();
        let query: HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(query["scope"], "openid profile email");
        assert_eq!(query["code_challenge_method"], "S256");
        *fixture.challenge.lock().unwrap() = query["code_challenge"].clone();
        let response = reqwest::Client::new()
            .get(&query["redirect_uri"])
            .query(&[("state", &query["state"]), ("code", &"fixture-code".into())])
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        manager.poll_login(&start.flow_id).await.unwrap().unwrap()
    }
    #[tokio::test]
    async fn native_account_login_catalog_usage_restart_and_single_flight_rotation() {
        let server = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path(), &server.fixture).await;
        let account = login(&manager, &server.fixture, DimSite::Cn).await;
        assert_eq!(account.id, "cn:subject");
        let models = manager.get_models(&account.id).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "vendor/full-id");
        assert_eq!(models[0].api_formats, ["openai_chat"]);
        assert!(!models[0].zdr);
        assert!(models[1].zdr);
        assert_eq!(models[1].efforts, ["high", "max"]);
        let usage = manager.get_usage(&account.id).await.unwrap();
        assert_eq!(usage.remaining_units, Some(80.0));
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].used, 30.0);
        assert_eq!(usage.windows[0].total, 100.0);
        {
            let mut memory = manager.memory.lock().unwrap();
            memory
                .store
                .accounts
                .get_mut(&account.id)
                .unwrap()
                .expires_at = 0;
            memory.dirty = true;
            manager.persist(&mut memory).unwrap();
        }
        let (first, second) = tokio::join!(
            manager.get_request_auth(&account.id),
            manager.get_request_auth(&account.id)
        );
        assert!(first.is_ok() && second.is_ok());
        assert_eq!(server.fixture.refreshes.load(Ordering::SeqCst), 1);
        let restored = DimOAuthManager::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            restored.status().await.default_account_id.as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(
            restored.memory.lock().unwrap().store.accounts[&account.id].refresh_token,
            "fixture-refresh-2"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&manager.storage_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        manager.remove_account(&account.id).await.unwrap();
        assert!(manager.get_request_auth(&account.id).await.is_err());
    }
    #[tokio::test]
    async fn callback_rejects_invalid_state_duplicates_and_denial_and_releases_port() {
        let server = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path(), &server.fixture).await;
        let start = manager.start_login(DimSite::Cn).await.unwrap();
        let url = Url::parse(&start.auth_url).unwrap();
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let client = reqwest::Client::new();
        for parameters in [
            vec![("state", "wrong"), ("code", "secret")],
            vec![
                ("state", query["state"].as_str()),
                ("code", "a"),
                ("code", "b"),
            ],
            vec![
                ("state", query["state"].as_str()),
                ("code", "a"),
                ("error", "denied"),
            ],
        ] {
            let response = client
                .get(&query["redirect_uri"])
                .query(&parameters)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        assert!(manager.poll_login(&start.flow_id).await.unwrap().is_none());
        client
            .get(&query["redirect_uri"])
            .query(&[("state", query["state"].as_str()), ("error", "denied")])
            .send()
            .await
            .unwrap();
        assert!(manager
            .poll_login(&start.flow_id)
            .await
            .unwrap_err()
            .contains("refused"));
        let port = Url::parse(&query["redirect_uri"]).unwrap().port().unwrap();
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        assert!(manager.start_login(DimSite::Cn).await.is_err());
        drop(listener);
        let start = manager.start_login(DimSite::Global).await.unwrap();
        manager.cancel_login(&start.flow_id).await.unwrap();
        assert!(manager.poll_login(&start.flow_id).await.is_err());
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        drop(listener);
        let start = manager
            .start_login_with_lifetime(DimSite::Cn, Duration::from_millis(50))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(manager.poll_login(&start.flow_id).await.is_err());
        let _listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn rotated_refresh_survives_storage_failure_without_reusing_old_token() {
        let server = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path(), &server.fixture).await;
        let account = login(&manager, &server.fixture, DimSite::Cn).await;
        {
            manager
                .memory
                .lock()
                .unwrap()
                .store
                .accounts
                .get_mut(&account.id)
                .unwrap()
                .expires_at = 0;
        }
        fs::remove_file(&manager.storage_path).unwrap();
        fs::create_dir(&manager.storage_path).unwrap();
        assert!(manager.get_request_auth(&account.id).await.is_err());
        assert_eq!(server.fixture.refreshes.load(Ordering::SeqCst), 1);
        fs::remove_dir(&manager.storage_path).unwrap();
        assert!(manager.get_request_auth(&account.id).await.is_ok());
        assert_eq!(server.fixture.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            DimOAuthManager::new(dir.path().to_path_buf())
                .unwrap()
                .memory
                .lock()
                .unwrap()
                .store
                .accounts[&account.id]
                .refresh_token,
            "fixture-refresh-2"
        );
    }
    #[tokio::test]
    async fn expired_refresh_is_explicit_and_sites_have_distinct_accounts() {
        let server = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path(), &server.fixture).await;
        let cn = login(&manager, &server.fixture, DimSite::Cn).await;
        let global = login(&manager, &server.fixture, DimSite::Global).await;
        assert_ne!(cn.id, global.id);
        manager
            .memory
            .lock()
            .unwrap()
            .store
            .accounts
            .get_mut(&cn.id)
            .unwrap()
            .expires_at = 0;
        server.fixture.reject_refresh.store(true, Ordering::SeqCst);
        let error = manager.get_request_auth(&cn.id).await.err().unwrap();
        assert_eq!(error, "Dim authorization expired; sign in again");
        assert!(
            manager
                .status()
                .await
                .accounts
                .iter()
                .find(|a| a.id == cn.id)
                .unwrap()
                .requires_reauth
        );
        assert!(manager.get_request_auth(&global.id).await.is_ok());
        assert!(endpoint("https://evil.invalid/token", DimSite::Cn.issuer()).is_err());
        let usage = parse_usage(
            &json!({"data":{"credits":{"remaining_units":10,"subscription_bucket":null}}}),
            DimSite::Cn.issuer(),
        )
        .unwrap();
        assert_eq!(usage.remaining_units, Some(10.0));
        assert!(usage.windows.is_empty());
    }
    #[tokio::test]
    async fn account_removal_prevents_late_login_and_explicit_missing_account_never_falls_back() {
        let server = fixture().await;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(manager(dir.path(), &server.fixture).await);
        let existing = login(&manager, &server.fixture, DimSite::Global).await;
        manager.set_default_account(&existing.id).await.unwrap();
        assert_eq!(
            manager
                .resolve_account_id(Some("deleted-account"))
                .await
                .unwrap(),
            "deleted-account"
        );
        assert!(manager.get_request_auth("deleted-account").await.is_err());
        let start = manager.start_login(DimSite::Cn).await.unwrap();
        let query: HashMap<String, String> = Url::parse(&start.auth_url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        *server.fixture.challenge.lock().unwrap() = query["code_challenge"].clone();
        reqwest::Client::new()
            .get(&query["redirect_uri"])
            .query(&[("state", query["state"].as_str()), ("code", "fixture-code")])
            .send()
            .await
            .unwrap();
        server.fixture.hold_exchange.store(true, Ordering::SeqCst);
        let task_manager = manager.clone();
        let pending = tokio::spawn(async move { task_manager.poll_login(&start.flow_id).await });
        server.fixture.exchange_started.notified().await;
        manager.remove_account(&existing.id).await.unwrap();
        server.fixture.release_exchange.notify_one();
        assert!(pending.await.unwrap().is_err());
        assert!(manager.status().await.accounts.is_empty());
        let usage = parse_usage(&json!({"data":{"credits":{"remaining_units":0,"subscription_bucket":{"unlimited":true,"window_states":[]}}}}),DimSite::Cn.issuer()).unwrap();
        assert!(usage.remaining_units.is_none());
    }
}
