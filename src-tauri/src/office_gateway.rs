use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures::StreamExt;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tower_http::cors::{Any, CorsLayer};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8790;
const LOG_RING_LIMIT: usize = 600;
const MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOOL_DESC_LEN: usize = 8_000;
const SHUTDOWN_GRACE_TIMEOUT_SECS: u64 = 2;

static SECRET_HEADER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)
        (
            \b(?:authorization|x-api-key|api[_-]?key)\b
            ["']?\s*[:=]\s*["']?
            (?:bearer\s+)?
        )
        ([^"',\s}\]]+)
        "#,
    )
    .expect("valid secret header regex")
});
static BEARER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\bbearer\s+([^"',\s}\]]+)"#).expect("valid bearer regex"));
static KEY_PREFIX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\b(?:sk|dk|tp)-[A-Za-z0-9._-]+\b|\bgho_[A-Za-z0-9_]+\b"#)
        .expect("valid key prefix regex")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeGatewayProviderKind {
    Auto,
    DeepSeek,
    Kimi,
    Mimo,
    MiniMax,
}

impl OfficeGatewayProviderKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::DeepSeek => "deepseek",
            Self::Kimi => "kimi",
            Self::Mimo => "mimo",
            Self::MiniMax => "minimax",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeGatewayConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub active_provider: OfficeGatewayProviderKind,
    pub default_max_tokens: u32,
    pub min_compat_max_tokens: u32,
    pub passthrough_metadata: bool,
    pub enable_web_search_tool: bool,
    pub model_primary: String,
    pub model_mid: String,
    pub model_fast: String,
    pub deepseek_api_key: String,
    pub deepseek_base_url: String,
    pub deepseek_model_primary: String,
    pub deepseek_model_mid: String,
    pub deepseek_model_fast: String,
    pub kimi_api_key: String,
    pub kimi_payg_base_url: String,
    pub kimi_coding_base_url: String,
    pub kimi_coding_model: String,
    pub kimi_model_primary: String,
    pub kimi_model_mid: String,
    pub kimi_model_fast: String,
    pub mimo_api_key: String,
    pub mimo_payg_base_url: String,
    pub mimo_tp_region: String,
    pub mimo_tp_base_url_cn: String,
    pub mimo_tp_base_url_sgp: String,
    pub mimo_tp_base_url_ams: String,
    pub mimo_model_primary: String,
    pub mimo_model_mid: String,
    pub mimo_model_fast: String,
    pub minimax_api_key: String,
    pub minimax_region: String,
    pub minimax_base_url_cn: String,
    pub minimax_base_url_global: String,
    pub minimax_model_primary: String,
    pub minimax_model_mid: String,
    pub minimax_model_fast: String,
}

impl Default for OfficeGatewayConfig {
    fn default() -> Self {
        Self {
            listen_host: DEFAULT_HOST.to_string(),
            listen_port: DEFAULT_PORT,
            active_provider: OfficeGatewayProviderKind::Auto,
            default_max_tokens: 4096,
            min_compat_max_tokens: 16,
            passthrough_metadata: false,
            enable_web_search_tool: false,
            model_primary: "deepseek-v4-pro".to_string(),
            model_mid: "deepseek-v4-flash".to_string(),
            model_fast: "deepseek-v4-flash".to_string(),
            deepseek_api_key: String::new(),
            deepseek_base_url: "https://api.deepseek.com/anthropic".to_string(),
            deepseek_model_primary: "deepseek-v4-pro".to_string(),
            deepseek_model_mid: "deepseek-v4-flash".to_string(),
            deepseek_model_fast: "deepseek-v4-flash".to_string(),
            kimi_api_key: String::new(),
            kimi_payg_base_url: "https://api.moonshot.cn/anthropic".to_string(),
            kimi_coding_base_url: "https://api.kimi.com/coding".to_string(),
            kimi_coding_model: "kimi-for-coding".to_string(),
            kimi_model_primary: "kimi-k2.6".to_string(),
            kimi_model_mid: "kimi-k2.5".to_string(),
            kimi_model_fast: "kimi-k2.5".to_string(),
            mimo_api_key: String::new(),
            mimo_payg_base_url: "https://api.xiaomimimo.com/anthropic".to_string(),
            mimo_tp_region: "cn".to_string(),
            mimo_tp_base_url_cn: "https://token-plan-cn.xiaomimimo.com/anthropic".to_string(),
            mimo_tp_base_url_sgp: "https://token-plan-sgp.xiaomimimo.com/anthropic".to_string(),
            mimo_tp_base_url_ams: "https://token-plan-ams.xiaomimimo.com/anthropic".to_string(),
            mimo_model_primary: "mimo-v2.5-pro".to_string(),
            mimo_model_mid: "mimo-v2.5".to_string(),
            mimo_model_fast: "mimo-v2.5".to_string(),
            minimax_api_key: String::new(),
            minimax_region: "cn".to_string(),
            minimax_base_url_cn: "https://api.minimaxi.com/anthropic".to_string(),
            minimax_base_url_global: "https://api.minimax.io/anthropic".to_string(),
            minimax_model_primary: "MiniMax-M2.7".to_string(),
            minimax_model_mid: "MiniMax-M2.5".to_string(),
            minimax_model_fast: "MiniMax-M2.5-highspeed".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeGatewayStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub active_provider: OfficeGatewayProviderKind,
    pub log_file: String,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeGatewayLogEntry {
    pub ts: String,
    pub level: String,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeGatewayLogSnapshot {
    pub entries: Vec<OfficeGatewayLogEntry>,
    pub log_file: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeGatewayUpstreamTestResult {
    pub ok: bool,
    pub provider: OfficeGatewayProviderKind,
    pub route_kind: String,
    pub upstream_url: String,
    pub model: String,
    pub status: u16,
    pub message: String,
    pub body_preview: String,
}

#[derive(Clone)]
struct GatewayRuntimeState {
    config: Arc<RwLock<OfficeGatewayConfig>>,
    logger: OfficeGatewayLogger,
    client: Client,
}

pub struct OfficeGatewayService {
    config: Arc<RwLock<OfficeGatewayConfig>>,
    config_path: PathBuf,
    logger: OfficeGatewayLogger,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    server_task: Mutex<Option<JoinHandle<()>>>,
    bound_addr: RwLock<Option<SocketAddr>>,
    started_at: RwLock<Option<String>>,
}

impl OfficeGatewayService {
    pub fn new(app_config_dir: PathBuf) -> Self {
        let office_dir = app_config_dir.join("office-gateway");
        let config_path = office_dir.join("config.json");
        let config = fs::read_to_string(&config_path)
            .ok()
            .and_then(|text| serde_json::from_str::<OfficeGatewayConfig>(&text).ok())
            .map(normalize_config)
            .unwrap_or_default();
        let logger = OfficeGatewayLogger::new(office_dir.join("office-gateway.log"));
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            logger,
            shutdown: Mutex::new(None),
            server_task: Mutex::new(None),
            bound_addr: RwLock::new(None),
            started_at: RwLock::new(None),
        }
    }

    pub async fn get_config(&self) -> OfficeGatewayConfig {
        self.config.read().await.clone()
    }

    pub async fn save_config(&self, config: OfficeGatewayConfig) -> Result<(), String> {
        let config = normalize_config(config);
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create Office Gateway config dir: {e}"))?;
        }
        let text = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize Office Gateway config: {e}"))?;
        fs::write(&self.config_path, text)
            .map_err(|e| format!("Failed to save Office Gateway config: {e}"))?;
        *self.config.write().await = config;
        self.logger
            .info("config", "Office Gateway config updated")
            .await;
        Ok(())
    }

    pub async fn status(&self) -> OfficeGatewayStatus {
        let config = self.config.read().await.clone();
        let running = self.shutdown.lock().await.is_some();
        let bound_addr = *self.bound_addr.read().await;
        let (host, port) = if running {
            bound_addr
                .map(|addr| (addr.ip().to_string(), addr.port()))
                .unwrap_or_else(|| (config.listen_host.clone(), config.listen_port))
        } else {
            (config.listen_host.clone(), config.listen_port)
        };
        OfficeGatewayStatus {
            running,
            base_url: format!("http://{host}:{port}"),
            host,
            port,
            active_provider: config.active_provider,
            log_file: self.logger.path_string(),
            started_at: self.started_at.read().await.clone(),
        }
    }

    pub async fn logs(&self) -> OfficeGatewayLogSnapshot {
        OfficeGatewayLogSnapshot {
            entries: self.logger.entries().await,
            log_file: self.logger.path_string(),
        }
    }

    pub async fn clear_logs(&self) -> Result<(), String> {
        self.logger.clear().await
    }

    pub fn log_file_path(&self) -> String {
        self.logger.path_string()
    }

    pub async fn start(&self) -> Result<OfficeGatewayStatus, String> {
        if self.shutdown.lock().await.is_some() {
            return Ok(self.status().await);
        }

        let config = self.config.read().await.clone();
        let addr: SocketAddr = format!("{}:{}", config.listen_host, config.listen_port)
            .parse()
            .map_err(|e| format!("Invalid Office Gateway listen address: {e}"))?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind Office Gateway on {addr}: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read Office Gateway address: {e}"))?;
        let (tx, rx) = oneshot::channel();
        let state = GatewayRuntimeState {
            config: self.config.clone(),
            logger: self.logger.clone(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| format!("Failed to build Office Gateway HTTP client: {e}"))?,
        };
        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/v1/models", get(models))
            .route("/models", get(models))
            .route("/v1/messages", post(messages))
            .fallback(fallback)
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                    .allow_headers(Any),
            )
            .with_state(state.clone());

        let logger = self.logger.clone();
        let server_task = tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
            if let Err(err) = result {
                logger
                    .error(
                        "server",
                        format!("Office Gateway server stopped with error: {err}"),
                    )
                    .await;
            }
        });

        *self.shutdown.lock().await = Some(tx);
        *self.server_task.lock().await = Some(server_task);
        *self.bound_addr.write().await = Some(local_addr);
        *self.started_at.write().await = Some(Utc::now().to_rfc3339());
        self.logger
            .info(
                "server",
                format!("Office Gateway started at http://{local_addr}"),
            )
            .await;
        Ok(self.status().await)
    }

    pub async fn stop(&self) -> Result<(), String> {
        let tx = self.shutdown.lock().await.take();
        let server_task = self.server_task.lock().await.take();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
        if let Some(server_task) = server_task {
            let mut server_task = server_task;
            tokio::select! {
                _ = &mut server_task => {
                    self.logger.info("server", "Office Gateway stopped").await;
                }
                _ = sleep(Duration::from_secs(SHUTDOWN_GRACE_TIMEOUT_SECS)) => {
                    server_task.abort();
                    let _ = server_task.await;
                    self.logger
                        .info("server", "Office Gateway forced stop after graceful shutdown timeout")
                        .await;
                }
            }
        }
        *self.bound_addr.write().await = None;
        *self.started_at.write().await = None;
        Ok(())
    }

    pub async fn restart(&self) -> Result<OfficeGatewayStatus, String> {
        self.stop().await?;
        self.start().await
    }

    pub async fn test_upstream(&self) -> Result<OfficeGatewayUpstreamTestResult, String> {
        let config = self.config.read().await.clone();
        let headers = test_headers_for_config(&config)?;
        let route = resolve_route(&config, &headers).map_err(|(_, msg)| msg.to_string())?;
        let model = route_model(&config, route.provider.clone(), "sonnet", &route.kind);
        let body = json!({
            "model": model,
            "max_tokens": config.min_compat_max_tokens.max(16),
            "stream": false,
            "messages": [{"role": "user", "content": "ping"}],
        });
        self.logger
            .info(
                "test",
                format!(
                    "testing upstream provider={} kind={} upstream={} model={}",
                    route.provider.as_str(),
                    route.kind,
                    route.upstream_url,
                    body["model"].as_str().unwrap_or_default(),
                ),
            )
            .await;
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to build Office Gateway test client: {e}"))?;
        let resp = client
            .post(&route.upstream_url)
            .bearer_auth(&route.api_key)
            .header("x-api-key", &route.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Upstream test request failed: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let preview = truncate_for_log(&redact_secrets(&text), 1200);
        let result = OfficeGatewayUpstreamTestResult {
            ok: status.is_success(),
            provider: route.provider,
            route_kind: route.kind,
            upstream_url: route.upstream_url,
            model: body["model"].as_str().unwrap_or_default().to_string(),
            status: status.as_u16(),
            message: if status.is_success() {
                "上游测试通过".to_string()
            } else {
                format!("上游返回 HTTP {}", status.as_u16())
            },
            body_preview: preview,
        };
        let level = if result.ok { "info" } else { "error" };
        self.logger
            .push(
                level,
                "test",
                format!(
                    "upstream test status={} ok={} body={}",
                    result.status, result.ok, result.body_preview
                ),
            )
            .await;
        Ok(result)
    }
}

#[derive(Clone)]
pub struct OfficeGatewayLogger {
    path: Arc<PathBuf>,
    entries: Arc<Mutex<VecDeque<OfficeGatewayLogEntry>>>,
}

impl OfficeGatewayLogger {
    fn new(path: PathBuf) -> Self {
        Self {
            path: Arc::new(path),
            entries: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn path_string(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    async fn entries(&self) -> Vec<OfficeGatewayLogEntry> {
        self.entries.lock().await.iter().cloned().collect()
    }

    async fn clear(&self) -> Result<(), String> {
        self.entries.lock().await.clear();
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create Office Gateway log dir: {e}"))?;
        }
        fs::write(&*self.path, "").map_err(|e| format!("Failed to clear log file: {e}"))?;
        Ok(())
    }

    async fn info(&self, category: &str, message: impl Into<String>) {
        self.push("info", category, message.into()).await;
    }

    async fn error(&self, category: &str, message: impl Into<String>) {
        self.push("error", category, message.into()).await;
    }

    async fn push(&self, level: &str, category: &str, message: String) {
        let entry = OfficeGatewayLogEntry {
            ts: Utc::now().to_rfc3339(),
            level: level.to_string(),
            category: category.to_string(),
            message: redact_secrets(&message),
        };
        {
            let mut entries = self.entries.lock().await;
            entries.push_back(entry.clone());
            while entries.len() > LOG_RING_LIMIT {
                entries.pop_front();
            }
        }
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*self.path)
        {
            let _ = writeln!(
                file,
                "{} [{}] {} {}",
                entry.ts, entry.level, entry.category, entry.message
            );
        }
    }
}

async fn healthz(State(state): State<GatewayRuntimeState>) -> Json<Value> {
    let config = state.config.read().await;
    Json(json!({
        "status": "ok",
        "provider": config.active_provider.as_str(),
    }))
}

async fn models(State(state): State<GatewayRuntimeState>) -> Json<Value> {
    let config = state.config.read().await.clone();
    Json(build_models_response(&config))
}

async fn fallback() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"type": "not_found", "message": "Office Gateway route not found"}})),
    )
}

async fn messages(
    State(state): State<GatewayRuntimeState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "Request body too large",
        );
    }

    let raw_bytes = body.as_ref();
    let raw_bytes = raw_bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw_bytes);
    let raw_body: Value = match serde_json::from_slice(raw_bytes) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Request body must be a JSON object",
            )
        }
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_json", "Invalid JSON body"),
    };

    let config = state.config.read().await.clone();
    let route = match resolve_route(&config, &headers) {
        Ok(route) => route,
        Err((status, msg)) => return json_error(status, "authentication_error", msg),
    };

    state
        .logger
        .info(
            "route",
            format!(
                "provider={} kind={} upstream={}",
                route.provider.as_str(),
                route.kind,
                route.upstream_url
            ),
        )
        .await;

    let sanitize = sanitize_request(
        raw_body,
        &config,
        route.provider.clone(),
        route.image_support,
    );
    if !sanitize.removed_fields.is_empty() || !sanitize.dropped.is_empty() {
        state
            .logger
            .info(
                "sanitize",
                format!(
                    "dropped={} removed={}",
                    serde_json::to_string(&sanitize.dropped).unwrap_or_default(),
                    sanitize.removed_fields.join(",")
                ),
            )
            .await;
    }
    if sanitize
        .body
        .get("messages")
        .and_then(Value::as_array)
        .is_none_or(|m| m.is_empty())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {"type": "invalid_request_error", "message": "No valid messages remain after gateway sanitization"},
                "dropped": sanitize.dropped,
                "removed_fields": sanitize.removed_fields,
            })),
        )
            .into_response();
    }

    let mut upstream_body = sanitize.body;
    let model_before = upstream_body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let model_after = route_model(&config, route.provider.clone(), &model_before, &route.kind);
    upstream_body["model"] = Value::String(model_after.clone());
    if model_before != model_after {
        state
            .logger
            .info(
                "model",
                format!("mapped {model_before:?} -> {model_after:?}"),
            )
            .await;
    }
    let stream = upstream_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages_count = upstream_body
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    state
        .logger
        .info(
            "request",
            format!(
                "model={} stream={} messages={} max_tokens={}",
                model_after,
                stream,
                messages_count,
                upstream_body
                    .get("max_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
            ),
        )
        .await;

    let request_builder = state
        .client
        .post(&route.upstream_url)
        .bearer_auth(&route.api_key)
        .header("x-api-key", &route.api_key)
        .header(
            "anthropic-version",
            headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(ANTHROPIC_VERSION),
        )
        .json(&upstream_body);

    if stream {
        match request_builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    state
                        .logger
                        .error(
                            "upstream",
                            format!(
                                "stream status={} body={}",
                                status,
                                truncate_for_log(&redact_secrets(&text), 1200)
                            ),
                        )
                        .await;
                    let mut response = Response::new(Body::from(text));
                    *response.status_mut() = status;
                    response.headers_mut().insert(
                        axum::http::header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    );
                    return response;
                }
                state
                    .logger
                    .info("upstream", format!("stream status={status}"))
                    .await;
                let mut response = Response::new(Body::from_stream(sse_shim_stream(
                    resp.bytes_stream(),
                    state.logger.clone(),
                )));
                *response.status_mut() = status;
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                response.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                );
                response
            }
            Err(err) => {
                state
                    .logger
                    .error("stream", format!("upstream stream error: {err}"))
                    .await;
                json_error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_http_error",
                    "Upstream service unavailable",
                )
            }
        }
    } else {
        match request_builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                state
                    .logger
                    .info(
                        "upstream",
                        format!(
                            "status={} bytes={}{}",
                            status,
                            text.len(),
                            if status.is_success() {
                                String::new()
                            } else {
                                format!(" body={}", truncate_for_log(&redact_secrets(&text), 1200))
                            }
                        ),
                    )
                    .await;
                let mut response = Response::new(Body::from(text));
                *response.status_mut() = status;
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                response
            }
            Err(err) => {
                state
                    .logger
                    .error("upstream", format!("upstream http error: {err}"))
                    .await;
                json_error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_http_error",
                    "Upstream service unavailable",
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Route {
    provider: OfficeGatewayProviderKind,
    api_key: String,
    upstream_url: String,
    kind: String,
    image_support: bool,
}

fn resolve_route(
    config: &OfficeGatewayConfig,
    headers: &HeaderMap,
) -> Result<Route, (StatusCode, &'static str)> {
    let provider = if config.active_provider == OfficeGatewayProviderKind::Auto {
        let key = incoming_token(headers).unwrap_or_default();
        classify_auto_provider(&key)
    } else {
        config.active_provider.clone()
    };

    match provider {
        OfficeGatewayProviderKind::DeepSeek => resolve_deepseek(config, headers),
        OfficeGatewayProviderKind::Kimi => resolve_kimi(config, headers),
        OfficeGatewayProviderKind::Mimo => resolve_mimo(config, headers),
        OfficeGatewayProviderKind::MiniMax => resolve_minimax(config, headers),
        OfficeGatewayProviderKind::Auto => unreachable!("auto should be resolved before routing"),
    }
}

fn test_headers_for_config(config: &OfficeGatewayConfig) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let key = match config.active_provider {
        OfficeGatewayProviderKind::DeepSeek => config.deepseek_api_key.trim(),
        OfficeGatewayProviderKind::Kimi => config.kimi_api_key.trim(),
        OfficeGatewayProviderKind::Mimo => config.mimo_api_key.trim(),
        OfficeGatewayProviderKind::MiniMax => config.minimax_api_key.trim(),
        OfficeGatewayProviderKind::Auto => [
            config.deepseek_api_key.trim(),
            config.kimi_api_key.trim(),
            config.mimo_api_key.trim(),
            config.minimax_api_key.trim(),
        ]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default(),
    };
    if key.is_empty() {
        return Err("请先在 Office Gateway 配置里填写当前 Provider 的 API Key".to_string());
    }
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(key).map_err(|_| "API Key 包含非法 header 字符".to_string())?,
    );
    Ok(headers)
}

fn classify_auto_provider(key: &str) -> OfficeGatewayProviderKind {
    let lower = key.trim().to_lowercase();
    if lower.starts_with("dk-") {
        OfficeGatewayProviderKind::DeepSeek
    } else if lower.starts_with("sk-kimi-") {
        OfficeGatewayProviderKind::Kimi
    } else if lower.starts_with("sk-api-") || lower.starts_with("sk-cp-") {
        OfficeGatewayProviderKind::MiniMax
    } else {
        OfficeGatewayProviderKind::Mimo
    }
}

fn resolve_deepseek(
    config: &OfficeGatewayConfig,
    headers: &HeaderMap,
) -> Result<Route, (StatusCode, &'static str)> {
    let key = configured_or_incoming(&config.deepseek_api_key, headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No API key available"))?;
    if config.deepseek_api_key.trim().is_empty()
        && !(key.starts_with("sk-") || key.starts_with("dk-"))
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid API key format for DeepSeek",
        ));
    }
    Ok(Route {
        provider: OfficeGatewayProviderKind::DeepSeek,
        api_key: key,
        upstream_url: append_messages_path(&config.deepseek_base_url),
        kind: "deepseek".to_string(),
        image_support: false,
    })
}

fn resolve_kimi(
    config: &OfficeGatewayConfig,
    headers: &HeaderMap,
) -> Result<Route, (StatusCode, &'static str)> {
    let key = configured_or_incoming(&config.kimi_api_key, headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No API key available"))?;
    if !key.starts_with("sk-") {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API key format for Kimi"));
    }
    let (base, kind) = if key.to_lowercase().starts_with("sk-kimi-") {
        (&config.kimi_coding_base_url, "kimi:codingplan")
    } else {
        (&config.kimi_payg_base_url, "kimi:payg")
    };
    Ok(Route {
        provider: OfficeGatewayProviderKind::Kimi,
        api_key: key,
        upstream_url: append_messages_path(base),
        kind: kind.to_string(),
        image_support: true,
    })
}

fn resolve_mimo(
    config: &OfficeGatewayConfig,
    headers: &HeaderMap,
) -> Result<Route, (StatusCode, &'static str)> {
    let key = configured_or_incoming(&config.mimo_api_key, headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No API key available"))?;
    if key.starts_with("sk-") {
        return Ok(Route {
            provider: OfficeGatewayProviderKind::Mimo,
            api_key: key,
            upstream_url: append_messages_path(&config.mimo_payg_base_url),
            kind: "mimo:payg".to_string(),
            image_support: false,
        });
    }
    if key.starts_with("tp-") {
        let region = header_str(headers, "x-mimo-tp-region")
            .map(|v| v.to_lowercase())
            .unwrap_or_else(|| config.mimo_tp_region.to_lowercase());
        let base = match region.as_str() {
            "cn" => &config.mimo_tp_base_url_cn,
            "sgp" => &config.mimo_tp_base_url_sgp,
            "ams" => &config.mimo_tp_base_url_ams,
            _ => return Err((StatusCode::BAD_REQUEST, "Invalid x-mimo-tp-region")),
        };
        return Ok(Route {
            provider: OfficeGatewayProviderKind::Mimo,
            api_key: key,
            upstream_url: append_messages_path(base),
            kind: format!("mimo:token-plan:{region}"),
            image_support: false,
        });
    }
    Err((StatusCode::UNAUTHORIZED, "Invalid API key prefix for MiMo"))
}

fn resolve_minimax(
    config: &OfficeGatewayConfig,
    headers: &HeaderMap,
) -> Result<Route, (StatusCode, &'static str)> {
    let key = configured_or_incoming(&config.minimax_api_key, headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No API key available"))?;
    if !(key.starts_with("sk-api-") || key.starts_with("sk-cp-")) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid API key prefix for MiniMax",
        ));
    }
    let region = header_str(headers, "x-minimax-region")
        .map(|v| v.to_lowercase())
        .unwrap_or_else(|| config.minimax_region.to_lowercase());
    let base = match region.as_str() {
        "cn" => &config.minimax_base_url_cn,
        "global" => &config.minimax_base_url_global,
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid x-minimax-region")),
    };
    let billing = if key.starts_with("sk-cp-") {
        "codingplan"
    } else {
        "payg"
    };
    Ok(Route {
        provider: OfficeGatewayProviderKind::MiniMax,
        api_key: key,
        upstream_url: append_messages_path(base),
        kind: format!("minimax:{billing}:{region}"),
        image_support: false,
    })
}

fn configured_or_incoming(configured: &str, headers: &HeaderMap) -> Option<String> {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    incoming_token(headers)
}

fn incoming_token(headers: &HeaderMap) -> Option<String> {
    if let Some(auth) = header_str(headers, "authorization") {
        if auth.to_lowercase().starts_with("bearer ") {
            let token = auth[7..].trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    header_str(headers, "x-api-key")
        .or_else(|| header_str(headers, "api-key"))
        .filter(|v| !v.trim().is_empty())
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

fn append_messages_path(base_url: &str) -> String {
    format!("{}/v1/messages", base_url.trim_end_matches('/'))
}

fn route_model(
    config: &OfficeGatewayConfig,
    provider: OfficeGatewayProviderKind,
    model_id: &str,
    route_kind: &str,
) -> String {
    if provider == OfficeGatewayProviderKind::Kimi && route_kind == "kimi:codingplan" {
        return config.kimi_coding_model.clone();
    }
    let (primary, mid, fast) = match provider {
        OfficeGatewayProviderKind::DeepSeek => (
            &config.deepseek_model_primary,
            &config.deepseek_model_mid,
            &config.deepseek_model_fast,
        ),
        OfficeGatewayProviderKind::Kimi => (
            &config.kimi_model_primary,
            &config.kimi_model_mid,
            &config.kimi_model_fast,
        ),
        OfficeGatewayProviderKind::Mimo => (
            &config.mimo_model_primary,
            &config.mimo_model_mid,
            &config.mimo_model_fast,
        ),
        OfficeGatewayProviderKind::MiniMax => (
            &config.minimax_model_primary,
            &config.minimax_model_mid,
            &config.minimax_model_fast,
        ),
        OfficeGatewayProviderKind::Auto => {
            (&config.model_primary, &config.model_mid, &config.model_fast)
        }
    };
    let value = model_id.trim();
    if value.is_empty() {
        return primary.clone();
    }
    let effective_mid = if mid.trim().is_empty() { fast } else { mid };
    let haiku_target = if provider == OfficeGatewayProviderKind::MiniMax {
        fast
    } else {
        effective_mid
    };
    match value {
        "opus" | "claude-opus-4-5" => return primary.clone(),
        "sonnet" | "claude-sonnet-4-5" => return effective_mid.clone(),
        "haiku" | "claude-haiku-4-5" => return haiku_target.clone(),
        _ => {}
    }
    if value == primary || value == mid || value == fast {
        return value.to_string();
    }
    let lower = value.to_lowercase();
    if lower.starts_with("claude-opus") {
        primary.clone()
    } else if lower.starts_with("claude-sonnet") {
        effective_mid.clone()
    } else if lower.starts_with("claude-haiku") {
        haiku_target.clone()
    } else {
        primary.clone()
    }
}

struct SanitizedRequest {
    body: Value,
    dropped: Map<String, Value>,
    removed_fields: Vec<String>,
}

fn sanitize_request(
    raw_body: Value,
    config: &OfficeGatewayConfig,
    provider: OfficeGatewayProviderKind,
    image_support: bool,
) -> SanitizedRequest {
    let raw_obj = raw_body.as_object().cloned().unwrap_or_default();
    let mut body = Map::new();
    let mut dropped = Map::new();
    let mut removed_fields = Vec::new();
    let allow = [
        "model",
        "max_tokens",
        "messages",
        "system",
        "temperature",
        "top_p",
        "top_k",
        "stop_sequences",
        "stream",
        "tools",
        "tool_choice",
        "thinking",
        "metadata",
    ];
    for (key, value) in raw_obj.iter() {
        if provider == OfficeGatewayProviderKind::DeepSeek && key == "thinking" {
            removed_fields.push(key.clone());
            continue;
        }
        if allow.contains(&key.as_str()) && (key != "metadata" || config.passthrough_metadata) {
            body.insert(key.clone(), value.clone());
        } else {
            removed_fields.push(key.clone());
        }
    }
    if let Some(system) = body.get("system").cloned() {
        body.insert("system".to_string(), normalize_system(system));
    }
    let supported = supported_content_types(image_support, config.enable_web_search_tool);
    let messages = normalize_messages(body.get("messages").cloned(), &mut dropped, &supported);
    body.insert("messages".to_string(), Value::Array(messages));
    if let Some(tools) = body.get("tools").cloned() {
        let sanitized_tools = sanitize_tools(tools, &mut dropped, config.enable_web_search_tool);
        if sanitized_tools
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            body.insert("tools".to_string(), sanitized_tools);
        } else {
            body.remove("tools");
        }
    }
    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    if let Some(tool_choice) = body.get("tool_choice").cloned() {
        if let Some(sanitized_tool_choice) =
            sanitize_tool_choice(tool_choice, has_tools, &mut dropped)
        {
            body.insert("tool_choice".to_string(), sanitized_tool_choice);
        } else {
            body.remove("tool_choice");
        }
    }
    if !matches!(body.get("stream"), Some(Value::Bool(_))) && body.contains_key("stream") {
        body.remove("stream");
        increment_drop(&mut dropped, "stream_invalid");
    }
    let is_probe = looks_like_connection_probe(&Value::Object(raw_obj));
    let raw_max = body.get("max_tokens").and_then(Value::as_i64).unwrap_or(0);
    if raw_max <= 0 {
        body.insert("max_tokens".to_string(), json!(config.default_max_tokens));
        increment_drop(&mut dropped, "max_tokens_defaulted");
    } else if is_probe && raw_max < config.min_compat_max_tokens as i64 {
        body.insert(
            "max_tokens".to_string(),
            json!(config.min_compat_max_tokens),
        );
        increment_drop(&mut dropped, "max_tokens_raised_for_compat");
    }
    SanitizedRequest {
        body: Value::Object(body),
        dropped,
        removed_fields,
    }
}

fn supported_content_types(image_support: bool, enable_web_search_tool: bool) -> Vec<&'static str> {
    let mut types = vec!["text", "thinking", "tool_use", "tool_result"];
    if image_support {
        types.push("image");
    }
    if enable_web_search_tool {
        types.push("server_tool_use");
        types.push("web_search_tool_result");
    }
    types
}

fn normalize_system(system: Value) -> Value {
    match system {
        Value::String(_) => system,
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(text) if !text.trim().is_empty() => {
                        out.push(json!({"type": "text", "text": text}))
                    }
                    Value::Object(obj)
                        if obj.get("type").and_then(Value::as_str) == Some("text")
                            && obj.get("text").and_then(Value::as_str).is_some() =>
                    {
                        out.push(Value::Object(obj));
                    }
                    _ => {}
                }
            }
            Value::Array(out)
        }
        Value::Object(obj)
            if obj.get("type").and_then(Value::as_str) == Some("text")
                && obj.get("text").and_then(Value::as_str).is_some() =>
        {
            Value::Array(vec![Value::Object(obj)])
        }
        other => Value::String(value_to_compact_string(&other)),
    }
}

fn normalize_messages(
    messages: Option<Value>,
    dropped: &mut Map<String, Value>,
    supported_types: &[&str],
) -> Vec<Value> {
    let Some(Value::Array(items)) = messages else {
        increment_drop(dropped, "messages_not_array");
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items.into_iter().take(1000) {
        let Value::Object(mut msg) = item else {
            increment_drop(dropped, "message_not_object");
            continue;
        };
        let role = msg
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if role != "user" && role != "assistant" {
            increment_drop(dropped, &format!("invalid_role:{role}"));
            continue;
        }
        let Some(content) = msg.remove("content") else {
            increment_drop(dropped, "message_missing_content");
            continue;
        };
        let normalized_content = normalize_content(content, dropped, supported_types);
        if normalized_content.as_array().is_none_or(|a| a.is_empty()) {
            increment_drop(dropped, "message_empty_after_sanitize");
            continue;
        }
        out.push(json!({"role": role, "content": normalized_content}));
    }
    out
}

fn normalize_content(
    content: Value,
    dropped: &mut Map<String, Value>,
    supported_types: &[&str],
) -> Value {
    match content {
        Value::String(text) => Value::Array(vec![json!({"type": "text", "text": text})]),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter_map(|block| sanitize_content_block(block, dropped, supported_types))
                .collect(),
        ),
        Value::Object(_) => sanitize_content_block(content, dropped, supported_types)
            .map(|v| Value::Array(vec![v]))
            .unwrap_or_else(|| Value::Array(Vec::new())),
        other => Value::Array(vec![
            json!({"type": "text", "text": value_to_compact_string(&other)}),
        ]),
    }
}

fn sanitize_content_block(
    block: Value,
    dropped: &mut Map<String, Value>,
    supported_types: &[&str],
) -> Option<Value> {
    let Value::Object(mut obj) = block else {
        increment_drop(dropped, "content_block_not_object");
        return None;
    };
    let block_type = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .to_string();
    if !supported_types.contains(&block_type.as_str()) {
        increment_drop(dropped, &format!("unsupported_content_block:{block_type}"));
        return None;
    }
    match block_type.as_str() {
        "text" if obj.get("text").and_then(Value::as_str).is_none() => {
            increment_drop(dropped, "text_missing_text");
            return None;
        }
        "thinking" if obj.get("thinking").and_then(Value::as_str).is_none() => {
            increment_drop(dropped, "thinking_missing_thinking");
            return None;
        }
        "tool_use" => {
            if obj.get("id").and_then(Value::as_str).is_none()
                || obj.get("name").and_then(Value::as_str).is_none()
            {
                increment_drop(dropped, "tool_use_invalid");
                return None;
            }
            if !obj.contains_key("input") {
                obj.insert("input".to_string(), json!({}));
            }
        }
        "tool_result" if obj.get("tool_use_id").and_then(Value::as_str).is_none() => {
            increment_drop(dropped, "tool_result_invalid");
            return None;
        }
        "image" if obj.get("source").is_none() => {
            increment_drop(dropped, "image_missing_source");
            return None;
        }
        _ => {}
    }
    Some(Value::Object(obj))
}

fn sanitize_tools(
    tools: Value,
    dropped: &mut Map<String, Value>,
    enable_web_search_tool: bool,
) -> Value {
    let Value::Array(items) = tools else {
        increment_drop(dropped, "tools_not_array");
        return Value::Array(Vec::new());
    };
    let mut out = Vec::new();
    for tool in items {
        let Value::Object(obj) = tool else {
            increment_drop(dropped, "tool_not_object");
            continue;
        };
        let tool_type = obj.get("type").and_then(Value::as_str).unwrap_or_default();
        if tool_type.starts_with("web_search_") {
            if enable_web_search_tool {
                out.push(Value::Object(obj));
            } else {
                increment_drop(dropped, "web_search_tool_disabled");
            }
            continue;
        }

        let source = obj.get("custom").and_then(Value::as_object).unwrap_or(&obj);
        let name = source
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| obj.get("name").and_then(Value::as_str))
            .unwrap_or_default();
        if name.is_empty() {
            increment_drop(dropped, "invalid_tool_name");
            continue;
        }
        let description = source
            .get("description")
            .and_then(Value::as_str)
            .or_else(|| obj.get("description").and_then(Value::as_str));
        let input_schema = source
            .get("input_schema")
            .filter(|value| value.is_object())
            .or_else(|| obj.get("input_schema").filter(|value| value.is_object()))
            .cloned()
            .unwrap_or_else(|| {
                increment_drop(dropped, "tool_schema_defaulted");
                json!({"type": "object", "properties": {}})
            });

        let mut normalized = Map::new();
        normalized.insert("name".to_string(), Value::String(name.to_string()));
        normalized.insert("input_schema".to_string(), input_schema);
        if let Some(description) = description.filter(|value| !value.is_empty()) {
            normalized.insert(
                "description".to_string(),
                Value::String(description.chars().take(MAX_TOOL_DESC_LEN).collect()),
            );
        }
        out.push(Value::Object(normalized));
    }
    Value::Array(out)
}

fn sanitize_tool_choice(
    tool_choice: Value,
    has_tools: bool,
    dropped: &mut Map<String, Value>,
) -> Option<Value> {
    if !has_tools {
        increment_drop(dropped, "tool_choice_without_tools");
        return None;
    }
    let Value::Object(mut obj) = tool_choice else {
        increment_drop(dropped, "tool_choice_not_object");
        return None;
    };
    let choice_type = obj.get("type").and_then(Value::as_str).unwrap_or_default();
    match choice_type {
        "auto" | "any" | "none" => Some(Value::Object(obj)),
        "tool" => {
            if obj.get("name").and_then(Value::as_str).is_some() {
                Some(Value::Object(obj))
            } else {
                increment_drop(dropped, "tool_choice_missing_name");
                None
            }
        }
        "custom" => {
            if obj.get("name").and_then(Value::as_str).is_some() {
                obj.insert("type".to_string(), Value::String("tool".to_string()));
                Some(Value::Object(obj))
            } else {
                increment_drop(dropped, "tool_choice_missing_name");
                None
            }
        }
        _ => {
            increment_drop(dropped, "tool_choice_invalid");
            None
        }
    }
}

fn looks_like_connection_probe(raw_body: &Value) -> bool {
    let Some(obj) = raw_body.as_object() else {
        return false;
    };
    if obj.get("stream").and_then(Value::as_bool).unwrap_or(false) || obj.contains_key("system") {
        return false;
    }
    let max_tokens = obj.get("max_tokens").and_then(Value::as_i64).unwrap_or(0);
    if max_tokens > 4 {
        return false;
    }
    let Some(messages) = obj.get("messages").and_then(Value::as_array) else {
        return false;
    };
    messages.len() == 1
}

fn build_models_response(_config: &OfficeGatewayConfig) -> Value {
    let ids = ["claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5"];
    json!({
        "object": "list",
        "data": ids.into_iter().map(|id| json!({
            "id": id,
            "type": "model",
            "object": "model",
            "created_at": 0,
            "display_name": id,
        })).collect::<Vec<_>>()
    })
}

fn increment_drop(dropped: &mut Map<String, Value>, key: &str) {
    let next = dropped.get(key).and_then(Value::as_u64).unwrap_or(0) + 1;
    dropped.insert(key.to_string(), json!(next));
}

fn value_to_compact_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn normalize_config(mut config: OfficeGatewayConfig) -> OfficeGatewayConfig {
    if config.listen_host.trim().is_empty() {
        config.listen_host = DEFAULT_HOST.to_string();
    }
    if config.listen_port == 0 {
        config.listen_port = DEFAULT_PORT;
    }
    for value in [
        &mut config.deepseek_base_url,
        &mut config.kimi_payg_base_url,
        &mut config.kimi_coding_base_url,
        &mut config.mimo_payg_base_url,
        &mut config.mimo_tp_base_url_cn,
        &mut config.mimo_tp_base_url_sgp,
        &mut config.mimo_tp_base_url_ams,
        &mut config.minimax_base_url_cn,
        &mut config.minimax_base_url_global,
    ] {
        *value = value.trim().trim_end_matches('/').to_string();
    }
    config
}

fn json_error(status: StatusCode, error_type: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error": {"type": error_type, "message": message}})),
    )
        .into_response()
}

#[derive(Default)]
struct SseShimState {
    seen_block_starts: HashSet<i64>,
    block_kind_by_index: HashMap<i64, String>,
    tool_partial_json_by_index: HashMap<i64, String>,
    pending_event_name: Option<String>,
}

fn sse_shim_stream(
    upstream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    logger: OfficeGatewayLogger,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    async_stream::try_stream! {
        let mut upstream = Box::pin(upstream);
        let mut buffer = String::new();
        let mut shim = SseShimState::default();

        while let Some(item) = upstream.next().await {
            let bytes = match item {
                Ok(bytes) => bytes,
                Err(err) => {
                    logger.error("stream", format!("upstream stream error: {err}")).await;
                    Err(std::io::Error::other(err.to_string()))?;
                    unreachable!();
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some((frame_end, delimiter_len)) = find_sse_frame_end(&buffer) {
                let frame_text = buffer[..frame_end].trim_end_matches('\r').to_string();
                buffer = buffer[frame_end + delimiter_len..].to_string();
                let lines = frame_text.lines().map(|line| line.trim_end_matches('\r').to_string()).collect();
                for chunk in process_sse_frame_lines(lines, &mut shim) {
                    yield chunk;
                }
            }
        }

        if !buffer.trim().is_empty() {
            let lines = buffer.lines().map(|line| line.trim_end_matches('\r').to_string()).collect();
            for chunk in process_sse_frame_lines(lines, &mut shim) {
                yield chunk;
            }
        }
    }
}

fn find_sse_frame_end(buffer: &str) -> Option<(usize, usize)> {
    let lf = buffer.find("\n\n").map(|pos| (pos, 2));
    let crlf = buffer.find("\r\n\r\n").map(|pos| (pos, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn process_sse_frame_lines(
    frame_lines: Vec<String>,
    state: &mut SseShimState,
) -> Vec<bytes::Bytes> {
    if frame_lines.is_empty() {
        return Vec::new();
    }

    let mut event_name: Option<String> = None;
    let mut data_lines = Vec::new();
    for line in &frame_lines {
        if let Some(rest) = line.strip_prefix("event:") {
            if event_name.is_none() {
                event_name = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }

    if event_name.is_some() && data_lines.is_empty() {
        state.pending_event_name = event_name;
        return Vec::new();
    }
    if event_name.is_none() && state.pending_event_name.is_some() && !data_lines.is_empty() {
        event_name = state.pending_event_name.take();
    } else if event_name.is_some() && !data_lines.is_empty() {
        state.pending_event_name = None;
    }

    if data_lines.is_empty() {
        return vec![bytes::Bytes::from(format!(
            "{}\n\n",
            frame_lines.join("\n")
        ))];
    }

    let payload = data_lines.join("\n");
    if payload == "[DONE]" {
        return vec![emit_sse_frame(event_name.as_deref(), Some(&payload))];
    }

    let parsed: Value = match serde_json::from_str(&payload) {
        Ok(parsed) => parsed,
        Err(_) => {
            let safe_error = json!({
                "type": "error",
                "error": {
                    "type": "gateway_bad_event",
                    "message": "Dropped malformed upstream data event"
                }
            });
            return vec![emit_sse_frame(Some("error"), Some(&safe_error.to_string()))];
        }
    };

    let event_type = parsed
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let event_index = parsed.get("index").and_then(Value::as_i64);
    let mut output = Vec::new();

    if matches!(event_type, "content_block_delta" | "content_block_stop") {
        if let Some(index) = event_index {
            if !state.seen_block_starts.contains(&index) {
                let synthetic_block = infer_synthetic_block(event_type, &parsed);
                let synthetic_payload = json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": synthetic_block,
                });
                output.push(emit_sse_frame(
                    Some("content_block_start"),
                    Some(&synthetic_payload.to_string()),
                ));
                state.seen_block_starts.insert(index);
                state.block_kind_by_index.insert(
                    index,
                    synthetic_block
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("text")
                        .to_string(),
                );
            }
        }
    } else if event_type == "content_block_start" {
        if let Some(index) = event_index {
            state.seen_block_starts.insert(index);
            if let Some(block_type) = parsed
                .get("content_block")
                .and_then(Value::as_object)
                .and_then(|block| block.get("type"))
                .and_then(Value::as_str)
            {
                state
                    .block_kind_by_index
                    .insert(index, block_type.to_string());
            }
        }
    }

    if event_type == "content_block_delta" {
        if let Some(index) = event_index {
            if state.block_kind_by_index.get(&index).map(String::as_str) == Some("tool_use") {
                if let Some(partial_json) = parsed
                    .get("delta")
                    .and_then(Value::as_object)
                    .filter(|delta| {
                        delta.get("type").and_then(Value::as_str) == Some("input_json_delta")
                    })
                    .and_then(|delta| delta.get("partial_json"))
                    .and_then(Value::as_str)
                {
                    state
                        .tool_partial_json_by_index
                        .entry(index)
                        .or_default()
                        .push_str(partial_json);
                    return output;
                }
            }
        }
    }

    if event_type == "content_block_stop" {
        if let Some(index) = event_index {
            if state.block_kind_by_index.get(&index).map(String::as_str) == Some("tool_use") {
                if let Some(raw) = state.tool_partial_json_by_index.remove(&index) {
                    let normalized = normalize_input_json_for_stream(&raw);
                    let shim_delta_payload = json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": normalized,
                        }
                    });
                    output.push(emit_sse_frame(
                        Some("content_block_delta"),
                        Some(&shim_delta_payload.to_string()),
                    ));
                }
            }
        }
    }

    output.push(emit_sse_frame(event_name.as_deref(), Some(&payload)));
    output
}

fn infer_synthetic_block(event_type: &str, event_data: &Value) -> Value {
    if event_type == "content_block_delta"
        && event_data
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("type"))
            .and_then(Value::as_str)
            == Some("input_json_delta")
    {
        return json!({"type": "tool_use", "id": "", "name": "", "input": {}});
    }
    json!({"type": "text", "text": ""})
}

fn normalize_input_json_for_stream(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return "{}".to_string();
    }
    match serde_json::from_str::<Value>(text) {
        Ok(parsed) => parsed.to_string(),
        Err(_) => {
            let stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
            let mut fragments = Vec::new();
            for item in stream {
                match item {
                    Ok(value) => fragments.push(value),
                    Err(_) => {
                        fragments.clear();
                        break;
                    }
                }
            }
            if fragments.is_empty() {
                return json!({"raw": raw}).to_string();
            }
            let mut merged = fragments.remove(0);
            for value in fragments {
                match (&mut merged, value) {
                    (Value::Object(left), Value::Object(right)) => {
                        left.extend(right);
                    }
                    (Value::Array(left), Value::Array(right)) => {
                        left.extend(right);
                    }
                    (_, next) => merged = next,
                }
            }
            merged.to_string()
        }
    }
}

fn emit_sse_frame(event_name: Option<&str>, data_payload: Option<&str>) -> bytes::Bytes {
    let mut out_lines = Vec::new();
    if let Some(event_name) = event_name {
        out_lines.push(format!("event: {event_name}"));
    }
    if let Some(data_payload) = data_payload {
        for segment in data_payload.split('\n') {
            out_lines.push(format!("data: {segment}"));
        }
    }
    bytes::Bytes::from(format!("{}\n\n", out_lines.join("\n")))
}

fn redact_secrets(input: &str) -> String {
    let out = SECRET_HEADER_RE.replace_all(input, "$1[redacted]");
    let out = BEARER_RE.replace_all(&out, "Bearer [redacted]");
    let mut out = KEY_PREFIX_RE.replace_all(&out, "[redacted]").into_owned();
    if out.len() > 3000 {
        out.truncate(3000);
        out.push('…');
    }
    out
}

fn truncate_for_log(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut value = input.to_string();
    value.truncate(max_len);
    value.push('…');
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_routes_expected_prefixes() {
        assert_eq!(
            classify_auto_provider("dk-test"),
            OfficeGatewayProviderKind::DeepSeek
        );
        assert_eq!(
            classify_auto_provider("sk-kimi-test"),
            OfficeGatewayProviderKind::Kimi
        );
        assert_eq!(
            classify_auto_provider("tp-test"),
            OfficeGatewayProviderKind::Mimo
        );
        assert_eq!(
            classify_auto_provider("sk-mimo-test"),
            OfficeGatewayProviderKind::Mimo
        );
        assert_eq!(
            classify_auto_provider("sk-api-test"),
            OfficeGatewayProviderKind::MiniMax
        );
        assert_eq!(
            classify_auto_provider("sk-cp-test"),
            OfficeGatewayProviderKind::MiniMax
        );
        assert_eq!(
            classify_auto_provider("sk-test"),
            OfficeGatewayProviderKind::Mimo
        );
    }

    #[test]
    fn region_headers_select_or_reject_upstream_urls() {
        let config = OfficeGatewayConfig::default();
        let mut mimo_headers = HeaderMap::new();
        mimo_headers.insert("x-api-key", HeaderValue::from_static("tp-test"));
        mimo_headers.insert("x-mimo-tp-region", HeaderValue::from_static("sgp"));
        let mimo_route = resolve_mimo(&config, &mimo_headers).unwrap();
        assert!(mimo_route
            .upstream_url
            .contains("token-plan-sgp.xiaomimimo.com"));

        mimo_headers.insert("x-mimo-tp-region", HeaderValue::from_static("moon"));
        let err = resolve_mimo(&config, &mimo_headers).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);

        let mut minimax_headers = HeaderMap::new();
        minimax_headers.insert("x-api-key", HeaderValue::from_static("sk-api-test"));
        minimax_headers.insert("x-minimax-region", HeaderValue::from_static("global"));
        let minimax_route = resolve_minimax(&config, &minimax_headers).unwrap();
        assert!(minimax_route.upstream_url.contains("api.minimax.io"));

        minimax_headers.insert("x-minimax-region", HeaderValue::from_static("moon"));
        let err = resolve_minimax(&config, &minimax_headers).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn sse_shim_inserts_missing_block_start() {
        let mut state = SseShimState::default();
        let payload = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"}
        });
        let chunks = process_sse_frame_lines(
            vec![
                "event: content_block_delta".to_string(),
                format!("data: {}", payload),
            ],
            &mut state,
        );
        assert_eq!(chunks.len(), 2);
        let first = String::from_utf8(chunks[0].to_vec()).unwrap();
        assert!(first.contains("event: content_block_start"));
        assert!(first.contains("\"content_block\""));
        assert!(first.contains("\"type\":\"text\""));
        assert!(first.contains("\"text\":\"\""));
    }

    #[test]
    fn sse_shim_buffers_tool_input_json_delta() {
        let mut state = SseShimState::default();
        let start = json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "x", "input": {}}
        });
        let delta_a = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"q\":"}
        });
        let delta_b = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "\"hi\"}"}
        });
        let stop = json!({"type": "content_block_stop", "index": 1});

        let _ = process_sse_frame_lines(
            vec![
                "event: content_block_start".to_string(),
                format!("data: {start}"),
            ],
            &mut state,
        );
        assert!(process_sse_frame_lines(
            vec![
                "event: content_block_delta".to_string(),
                format!("data: {delta_a}")
            ],
            &mut state,
        )
        .is_empty());
        assert!(process_sse_frame_lines(
            vec![
                "event: content_block_delta".to_string(),
                format!("data: {delta_b}")
            ],
            &mut state,
        )
        .is_empty());

        let chunks = process_sse_frame_lines(
            vec![
                "event: content_block_stop".to_string(),
                format!("data: {stop}"),
            ],
            &mut state,
        );
        assert_eq!(chunks.len(), 2);
        let first = String::from_utf8(chunks[0].to_vec()).unwrap();
        assert!(first.contains("event: content_block_delta"));
        assert!(first.contains(r#""partial_json":"{\"q\":\"hi\"}""#));
    }

    #[test]
    fn route_model_maps_claude_aliases() {
        let config = OfficeGatewayConfig::default();
        assert_eq!(
            route_model(
                &config,
                OfficeGatewayProviderKind::Mimo,
                "claude-opus-4-9",
                "mimo:payg"
            ),
            "mimo-v2.5-pro"
        );
        assert_eq!(
            route_model(
                &config,
                OfficeGatewayProviderKind::Mimo,
                "haiku",
                "mimo:payg"
            ),
            "mimo-v2.5"
        );
        assert_eq!(
            route_model(
                &config,
                OfficeGatewayProviderKind::MiniMax,
                "haiku",
                "minimax:payg:cn"
            ),
            "MiniMax-M2.5-highspeed"
        );
    }

    #[test]
    fn models_response_only_advertises_canonical_office_models() {
        let config = OfficeGatewayConfig::default();
        let response = build_models_response(&config);
        let ids = response["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec!["claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5"]
        );
    }

    #[test]
    fn probe_max_tokens_is_raised() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({"model": "sonnet", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]});
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::DeepSeek, false);
        assert_eq!(sanitized.body["max_tokens"], json!(16));
        assert_eq!(sanitized.dropped["max_tokens_raised_for_compat"], json!(1));
    }

    #[test]
    fn sanitize_removes_unsupported_content_blocks() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "ok"}, {"type": "image", "source": {}}]}]});
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::DeepSeek, false);
        let blocks = sanitized.body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            sanitized.dropped["unsupported_content_block:image"],
            json!(1)
        );
    }

    #[test]
    fn sanitize_preserves_thinking_blocks_with_thinking_text() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "thinking",
                    "thinking": "reasoning",
                    "signature": "sig"
                }]
            }]
        });
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::Mimo, false);

        assert_eq!(
            sanitized.body["messages"][0]["content"][0]["thinking"],
            json!("reasoning")
        );
        assert_eq!(
            sanitized.body["messages"][0]["content"][0]["signature"],
            json!("sig")
        );
    }

    #[test]
    fn sanitize_normalizes_office_custom_tools() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({
            "messages": [{"role": "user", "content": "ping"}],
            "tools": [{
                "type": "custom",
                "custom": {
                    "name": "do_work",
                    "description": "Run a workbook helper.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        }
                    }
                }
            }]
        });
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::DeepSeek, false);
        let tool = &sanitized.body["tools"][0];

        assert_eq!(tool["name"], json!("do_work"));
        assert_eq!(tool["description"], json!("Run a workbook helper."));
        assert_eq!(tool["input_schema"]["type"], json!("object"));
        assert!(tool.get("type").is_none());
        assert!(tool.get("custom").is_none());
    }

    #[test]
    fn sanitize_removes_empty_tools_and_dangling_tool_choice() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({
            "messages": [{"role": "user", "content": "ping"}],
            "tools": [{"type": "web_search_20260209", "name": "web_search"}],
            "tool_choice": {"type": "any"}
        });
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::DeepSeek, false);

        assert!(sanitized.body.get("tools").is_none());
        assert!(sanitized.body.get("tool_choice").is_none());
        assert_eq!(sanitized.dropped["web_search_tool_disabled"], json!(1));
        assert_eq!(sanitized.dropped["tool_choice_without_tools"], json!(1));
    }

    #[test]
    fn sanitize_normalizes_office_custom_tool_choice() {
        let config = OfficeGatewayConfig::default();
        let raw = json!({
            "messages": [{"role": "user", "content": "ping"}],
            "tools": [{"name": "do_work", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": {"type": "custom", "name": "do_work"}
        });
        let sanitized = sanitize_request(raw, &config, OfficeGatewayProviderKind::DeepSeek, false);

        assert_eq!(
            sanitized.body["tool_choice"],
            json!({"type": "tool", "name": "do_work"})
        );
    }

    #[test]
    fn redacts_log_secrets() {
        let redacted = redact_secrets(
            r#"Authorization Bearer sk-test tp-secret dk-secret {"x-api-key":"sk-json"} Authorization: Bearer sk-header api_key=plain-secret"#,
        );
        assert!(!redacted.contains("sk-test"));
        assert!(!redacted.contains("tp-secret"));
        assert!(!redacted.contains("dk-secret"));
        assert!(!redacted.contains("sk-json"));
        assert!(!redacted.contains("sk-header"));
        assert!(!redacted.contains("plain-secret"));
    }
}
