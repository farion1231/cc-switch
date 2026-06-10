use crate::app_config::AppType;
use crate::commands;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::{ProviderService, ProviderSortUpdate};
use crate::store::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

const DEFAULT_WEBUI_HOST: &str = "127.0.0.1";
const DEFAULT_WEBUI_PORT: u16 = 15722;
const TOKEN_ENV: &str = "CC_SWITCH_WEBUI_TOKEN";
const HOST_ENV: &str = "CC_SWITCH_WEBUI_HOST";
const PORT_ENV: &str = "CC_SWITCH_WEBUI_PORT";

#[derive(Clone)]
struct WebUiState {
    app_state: Arc<AppState>,
    token: Option<String>,
}

pub struct WebUiServer {
    state: Arc<AppState>,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppQuery {
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchProviderRequest {
    id: String,
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertProviderRequest {
    provider: Provider,
    app: String,
    #[serde(default)]
    add_to_live: Option<bool>,
    #[serde(default)]
    original_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteProviderRequest {
    id: String,
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SortProvidersRequest {
    updates: Vec<ProviderSortUpdate>,
    app: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppTypeRequest {
    app_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetTakeoverRequest {
    app_type: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchProxyProviderRequest {
    app_type: String,
    provider_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppTypeValueRequest {
    app_type: String,
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchModelsRequest {
    base_url: String,
    api_key: String,
    #[serde(default)]
    is_full_url: bool,
    #[serde(default)]
    models_url: Option<String>,
}

fn parse_bool_env(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn is_private_ip(addr: &SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()  // RFC 1918: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || ip.is_link_local()  // 169.254.0.0/16
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback() || ip.is_unicast_link_local()
        }
    }
}

fn auth_token() -> Option<String> {
    std::env::var(TOKEN_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn configured_addr() -> SocketAddr {
    let host = std::env::var(HOST_ENV).unwrap_or_else(|_| DEFAULT_WEBUI_HOST.to_string());
    let port = std::env::var(PORT_ENV)
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_WEBUI_PORT);
    format!("{host}:{port}")
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], DEFAULT_WEBUI_PORT)))
}

fn command_error(error: impl ToString) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": error.to_string(),
        })),
    )
        .into_response()
}

fn app_type(app: &str) -> Result<AppType, Response> {
    AppType::from_str(app).map_err(command_error)
}

async fn require_auth(
    State(state): State<WebUiState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Some(expected) = state.token.as_deref() else {
        return next.run(request).await;
    };

    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|token| token == expected)
        .unwrap_or(false);

    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": format!("WebUI requires Authorization: Bearer <token>. Set {TOKEN_ENV} to enable LAN-safe access."),
            })),
        )
            .into_response()
    }
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "cc-switch-webui",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn webui_status() -> Json<Value> {
    let settings = crate::settings::get_settings();
    Json(json!({
        "running": true,
        "enabled": settings.webui_enabled,
        "port": settings.webui_port,
        "host": settings.webui_host,
        "address": format!("http://{}:{}", settings.webui_host, settings.webui_port),
        "tokenSet": settings.webui_token.is_some(),
    }))
}

/// Browser client hitting this route means the server is already running.
async fn webui_start() -> Json<Value> {
    let settings = crate::settings::get_settings();
    Json(json!(format!("http://{}:{}", settings.webui_host, settings.webui_port)))
}

/// Server cannot stop itself from a browser client request.
/// The setting is saved separately; the server will not auto-start next launch.
async fn webui_stop() -> Response {
    (StatusCode::CONFLICT, Json(json!({
        "error": "Cannot stop WebUI server from browser. Disable in settings and restart the app, or use the desktop UI."
    }))).into_response()
}

/// Server cannot restart itself from its own HTTP handler.
async fn webui_restart() -> Response {
    (StatusCode::CONFLICT, Json(json!({
        "error": "Cannot restart WebUI server from browser. Change settings and restart the app, or use the desktop UI."
    }))).into_response()
}

async fn get_settings() -> Json<crate::settings::AppSettings> {
    Json(crate::settings::get_settings_for_frontend())
}

async fn save_settings(Json(settings): Json<crate::settings::AppSettings>) -> Response {
    match commands::save_settings(settings).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_providers(
    State(state): State<WebUiState>,
    Query(query): Query<AppQuery>,
) -> Response {
    let app_type = match app_type(&query.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::list(&state.app_state, app_type) {
        Ok(providers) => Json(providers).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_current_provider(
    State(state): State<WebUiState>,
    Query(query): Query<AppQuery>,
) -> Response {
    let app_type = match app_type(&query.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::current(&state.app_state, app_type) {
        Ok(id) => Json(id).into_response(),
        Err(e) => command_error(e),
    }
}

async fn add_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<UpsertProviderRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::add(
        &state.app_state,
        app_type,
        payload.provider,
        payload.add_to_live.unwrap_or(true),
    ) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn update_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<UpsertProviderRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::update(
        &state.app_state,
        app_type,
        payload.original_id.as_deref(),
        payload.provider,
    ) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn delete_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<DeleteProviderRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::delete(&state.app_state, app_type, &payload.id) {
        Ok(()) => Json(true).into_response(),
        Err(e) => command_error(e),
    }
}

async fn switch_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<SwitchProviderRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::switch(&state.app_state, app_type, &payload.id) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn import_default_config(
    State(state): State<WebUiState>,
    Json(payload): Json<AppQuery>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::import_default_config(&state.app_state, app_type) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn update_sort_order(
    State(state): State<WebUiState>,
    Json(payload): Json<SortProvidersRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::update_sort_order(&state.app_state, app_type, payload.updates) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_claude_desktop_status(State(state): State<WebUiState>) -> Response {
    let proxy_running = state.app_state.proxy_service.is_running().await;
    match crate::claude_desktop_config::get_status(state.app_state.db.as_ref(), proxy_running) {
        Ok(status) => Json(status).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_claude_desktop_default_routes() -> Json<Value> {
    Json(json!(crate::claude_desktop_config::default_proxy_routes()))
}

async fn update_tray_menu() -> Json<bool> {
    Json(true)
}

async fn get_opencode_live_provider_ids() -> Response {
    match crate::opencode_config::get_providers() {
        Ok(providers) => Json(providers.keys().cloned().collect::<Vec<_>>()).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_openclaw_live_provider_ids() -> Response {
    match crate::openclaw_config::get_providers() {
        Ok(providers) => Json(providers.keys().cloned().collect::<Vec<_>>()).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_hermes_live_provider_ids() -> Response {
    match crate::hermes_config::get_providers() {
        Ok(providers) => Json(providers.keys().cloned().collect::<Vec<_>>()).into_response(),
        Err(e) => command_error(e),
    }
}

async fn start_proxy(State(state): State<WebUiState>) -> Response {
    match state.app_state.proxy_service.start().await {
        Ok(info) => Json(info).into_response(),
        Err(e) => command_error(e),
    }
}

async fn stop_proxy_server(State(state): State<WebUiState>) -> Response {
    let takeover = match state.app_state.proxy_service.get_takeover_status().await {
        Ok(v) => v,
        Err(e) => return command_error(e),
    };

    if takeover.claude || takeover.codex || takeover.gemini || takeover.opencode || takeover.openclaw {
        return command_error("仍有应用处于代理接管状态，请先在设置中关闭对应应用接管后再停止本地路由。");
    }

    match state.app_state.proxy_service.stop().await {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn stop_proxy_with_restore(State(state): State<WebUiState>) -> Response {
    match state.app_state.proxy_service.stop_with_restore().await {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_proxy_status(State(state): State<WebUiState>) -> Response {
    match state.app_state.proxy_service.get_status().await {
        Ok(status) => Json(status).into_response(),
        Err(e) => command_error(e),
    }
}

async fn is_proxy_running(State(state): State<WebUiState>) -> Json<bool> {
    Json(state.app_state.proxy_service.is_running().await)
}

async fn is_live_takeover_active(State(state): State<WebUiState>) -> Json<bool> {
    Json(state.app_state.proxy_service.is_takeover_active().await.unwrap_or(false))
}

async fn get_proxy_takeover_status(State(state): State<WebUiState>) -> Response {
    match state.app_state.proxy_service.get_takeover_status().await {
        Ok(status) => Json(status).into_response(),
        Err(e) => command_error(e),
    }
}

async fn set_proxy_takeover(
    State(state): State<WebUiState>,
    Json(payload): Json<SetTakeoverRequest>,
) -> Response {
    match state
        .app_state
        .proxy_service
        .set_takeover_for_app(&payload.app_type, payload.enabled)
        .await
    {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn switch_proxy_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<SwitchProxyProviderRequest>,
) -> Response {
    match state
        .app_state
        .proxy_service
        .hot_switch_provider(&payload.app_type, &payload.provider_id)
        .await
    {
        Ok(_result) => Json(json!(true)).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_proxy_config(State(state): State<WebUiState>) -> Response {
    match state.app_state.proxy_service.get_config().await {
        Ok(config) => Json(config).into_response(),
        Err(e) => command_error(e),
    }
}

async fn update_proxy_config(
    State(state): State<WebUiState>,
    Json(payload): Json<Value>,
) -> Response {
    let config = match payload.get("config").cloned() {
        Some(value) => match serde_json::from_value(value) {
            Ok(config) => config,
            Err(e) => return command_error(e),
        },
        None => return command_error("missing config"),
    };
    match state.app_state.proxy_service.update_config(&config).await {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_proxy_config_for_app(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeRequest>,
) -> Response {
    match state.app_state.db.get_proxy_config_for_app(&payload.app_type).await {
        Ok(config) => Json(config).into_response(),
        Err(e) => command_error(e),
    }
}

async fn update_proxy_config_for_app(
    State(state): State<WebUiState>,
    Json(payload): Json<Value>,
) -> Response {
    let config: crate::proxy::types::AppProxyConfig = match payload.get("config").cloned() {
        Some(value) => match serde_json::from_value(value) {
            Ok(config) => config,
            Err(e) => return command_error(e),
        },
        None => return command_error("missing config"),
    };
    match state.app_state.db.update_proxy_config_for_app(config).await {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_global_proxy_config(State(state): State<WebUiState>) -> Response {
    match state.app_state.db.get_global_proxy_config().await {
        Ok(config) => Json(config).into_response(),
        Err(e) => command_error(e),
    }
}

async fn update_global_proxy_config(
    State(state): State<WebUiState>,
    Json(payload): Json<Value>,
) -> Response {
    let config = match payload.get("config").cloned() {
        Some(value) => match serde_json::from_value(value) {
            Ok(config) => config,
            Err(e) => return command_error(e),
        },
        None => return command_error("missing config"),
    };
    match state.app_state.db.update_global_proxy_config(config).await {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_default_cost_multiplier(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeValueRequest>,
) -> Response {
    match state.app_state.db.get_default_cost_multiplier(&payload.app_type).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => command_error(e),
    }
}

async fn set_default_cost_multiplier(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeValueRequest>,
) -> Response {
    match state
        .app_state
        .db
        .set_default_cost_multiplier(&payload.app_type, payload.value.as_deref().unwrap_or("1"))
        .await
    {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_pricing_model_source(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeValueRequest>,
) -> Response {
    match state.app_state.db.get_pricing_model_source(&payload.app_type).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => command_error(e),
    }
}

async fn set_pricing_model_source(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeValueRequest>,
) -> Response {
    match state
        .app_state
        .db
        .set_pricing_model_source(&payload.app_type, payload.value.as_deref().unwrap_or("response"))
        .await
    {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn fetch_models(Json(payload): Json<FetchModelsRequest>) -> Response {
    match crate::services::model_fetch::fetch_models(
        &payload.base_url,
        &payload.api_key,
        payload.is_full_url,
        payload.models_url.as_deref(),
    )
    .await
    {
        Ok(models) => Json(models).into_response(),
        Err(e) => command_error(e),
    }
}

async fn usage_command(
    Path(command): Path<String>,
    State(state): State<WebUiState>,
    Json(payload): Json<Value>,
) -> Response {
    let db = &state.app_state.db;
    let result: Result<Value, String> = match command.as_str() {
        "get_usage_summary" => db
            .get_usage_summary(
                payload.get("startDate").and_then(Value::as_i64),
                payload.get("endDate").and_then(Value::as_i64),
                payload.get("appType").and_then(Value::as_str),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_usage_summary_by_app" => db
            .get_usage_summary_by_app(
                payload.get("startDate").and_then(Value::as_i64),
                payload.get("endDate").and_then(Value::as_i64),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_usage_trends" => db
            .get_daily_trends(
                payload.get("startDate").and_then(Value::as_i64),
                payload.get("endDate").and_then(Value::as_i64),
                payload.get("appType").and_then(Value::as_str),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_provider_stats" => db
            .get_provider_stats(
                payload.get("startDate").and_then(Value::as_i64),
                payload.get("endDate").and_then(Value::as_i64),
                payload.get("appType").and_then(Value::as_str),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_model_stats" => db
            .get_model_stats(
                payload.get("startDate").and_then(Value::as_i64),
                payload.get("endDate").and_then(Value::as_i64),
                payload.get("appType").and_then(Value::as_str),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_request_logs" => {
            let filters = match serde_json::from_value(
                payload.get("filters").cloned().unwrap_or_else(|| json!({})),
            ) {
                Ok(filters) => filters,
                Err(e) => return command_error(e),
            };
            db.get_request_logs(
                &filters,
                payload.get("page").and_then(Value::as_u64).unwrap_or(1) as u32,
                payload.get("pageSize").and_then(Value::as_u64).unwrap_or(50) as u32,
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string())
        }
        "get_request_detail" => db
            .get_request_detail(payload.get("requestId").and_then(Value::as_str).unwrap_or_default())
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "get_model_pricing" => {
            if let Err(e) = state.app_state.db.ensure_model_pricing_seeded() {
                Err(e.to_string())
            } else {
                let db = state.app_state.db.clone();
                let conn = match db.conn.lock() {
                    Ok(conn) => conn,
                    Err(e) => return command_error(AppError::Database(format!("Mutex lock failed: {e}"))),
                };
                let mut stmt = match conn.prepare(
                    "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                            cache_read_cost_per_million, cache_creation_cost_per_million
                     FROM model_pricing
                     ORDER BY display_name",
                ) {
                    Ok(stmt) => stmt,
                    Err(e) => return command_error(e),
                };
                let rows = match stmt.query_map([], |row| {
                    Ok(commands::ModelPricingInfo {
                        model_id: row.get(0)?,
                        display_name: row.get(1)?,
                        input_cost_per_million: row.get(2)?,
                        output_cost_per_million: row.get(3)?,
                        cache_read_cost_per_million: row.get(4)?,
                        cache_creation_cost_per_million: row.get(5)?,
                    })
                }) {
                    Ok(rows) => rows,
                    Err(e) => return command_error(e),
                };
                match rows.collect::<Result<Vec<_>, _>>() {
                    Ok(pricing) => Ok(json!(pricing)),
                    Err(e) => Err(e.to_string()),
                }
            }
        }
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "unsupported WebUI command" }))).into_response(),
    };

    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => command_error(e),
    }
}

fn command_router(state: WebUiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/health", get(health))
        .route("/api/webui/status", get(webui_status))
        .route("/api/webui/start", post(webui_start))
        .route("/api/webui/stop", post(webui_stop))
        .route("/api/webui/restart", post(webui_restart))
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/providers", get(get_providers))
        .route("/api/providers/current", get(get_current_provider))
        .route("/api/providers/add", post(add_provider))
        .route("/api/providers/update", post(update_provider))
        .route("/api/providers/delete", post(delete_provider))
        .route("/api/providers/switch", post(switch_provider))
        .route("/api/providers/import-default", post(import_default_config))
        .route("/api/providers/sort", post(update_sort_order))
        .route("/api/claude-desktop/status", get(get_claude_desktop_status))
        .route("/api/claude-desktop/default-routes", get(get_claude_desktop_default_routes))
        .route("/api/opencode/live-provider-ids", get(get_opencode_live_provider_ids))
        .route("/api/openclaw/live-provider-ids", get(get_openclaw_live_provider_ids))
        .route("/api/hermes/live-provider-ids", get(get_hermes_live_provider_ids))
        .route("/api/tray/update", post(update_tray_menu))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy_server))
        .route("/api/proxy/stop-with-restore", post(stop_proxy_with_restore))
        .route("/api/proxy/status", get(get_proxy_status))
        .route("/api/proxy/running", get(is_proxy_running))
        .route("/api/proxy/live-takeover-active", get(is_live_takeover_active))
        .route("/api/proxy/takeover-status", get(get_proxy_takeover_status))
        .route("/api/proxy/takeover", post(set_proxy_takeover))
        .route("/api/proxy/switch-provider", post(switch_proxy_provider))
        .route("/api/proxy/config", get(get_proxy_config).post(update_proxy_config))
        .route("/api/proxy/global-config", get(get_global_proxy_config).post(update_global_proxy_config))
        .route("/api/proxy/app-config", post(get_proxy_config_for_app))
        .route("/api/proxy/app-config/update", post(update_proxy_config_for_app))
        .route("/api/proxy/default-cost-multiplier", post(get_default_cost_multiplier))
        .route("/api/proxy/default-cost-multiplier/update", post(set_default_cost_multiplier))
        .route("/api/proxy/pricing-model-source", post(get_pricing_model_source))
        .route("/api/proxy/pricing-model-source/update", post(set_pricing_model_source))
        .route("/api/models/fetch", post(fetch_models))
        .route("/api/usage/:command", post(usage_command))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

impl WebUiServer {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub fn should_start_from_env() -> bool {
        parse_bool_env("CC_SWITCH_WEBUI")
    }

    pub async fn start_from_env(&self) -> Result<SocketAddr, String> {
        self.start(configured_addr(), auth_token()).await
    }

    /// Start WebUI using persisted AppSettings (env vars still override if set)
    pub async fn start_from_settings(&self) -> Result<SocketAddr, String> {
        let settings = crate::settings::get_settings();

        // Env var override takes precedence
        let host = std::env::var(HOST_ENV)
            .unwrap_or_else(|_| settings.webui_host.clone());
        let port = std::env::var(PORT_ENV)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(settings.webui_port);

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], DEFAULT_WEBUI_PORT)));

        // Only require token for public IP (non-private) access
        let token = if is_private_ip(&addr) {
            None
        } else {
            auth_token().or_else(|| settings.webui_token.clone())
        };

        self.start(addr, token).await
    }

    pub async fn is_running(&self) -> bool {
        self.shutdown_tx.read().await.is_some()
    }

    pub async fn get_status(&self) -> serde_json::Value {
        let running = self.is_running().await;
        let settings = crate::settings::get_settings();
        serde_json::json!({
            "running": running,
            "host": settings.webui_host,
            "port": settings.webui_port,
            "hasToken": settings.webui_token.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
            "enabled": settings.webui_enabled,
        })
    }

    pub async fn start(&self, addr: SocketAddr, token: Option<String>) -> Result<SocketAddr, String> {
        if self.shutdown_tx.read().await.is_some() {
            return Err("WebUI server is already running".to_string());
        }

        if !is_private_ip(&addr) && token.is_none() {
            return Err(format!(
                "Refusing to expose WebUI on public IP {addr} without {TOKEN_ENV}. Set a strong bearer token first."
            ));
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("WebUI bind failed: {e}"))?;
        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("WebUI local address failed: {e}"))?;

        let state = WebUiState {
            app_state: self.state.clone(),
            token,
        };

        // Try to find dist directory relative to executable
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        let dist_path = exe_dir
            .as_ref()
            .map(|dir| dir.join("dist"))
            .filter(|p| p.exists())
            .or_else(|| {
                // Fallback: check workspace root during development
                let cwd = std::env::current_dir().ok()?;
                let workspace_dist = cwd.join("dist");
                workspace_dist.exists().then_some(workspace_dist)
            });

        let app = if let Some(dist) = dist_path {
            log::info!("WebUI serving static files from: {}", dist.display());
            // Serve API routes + static files with SPA fallback
            command_router(state.clone())
                .fallback_service(ServeDir::new(dist).append_index_html_on_directories(true))
        } else {
            log::warn!("WebUI dist/ not found, serving API only");
            command_router(state.clone())
        };

        let app = app.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        );

        *self.shutdown_tx.write().await = Some(shutdown_tx);
        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(e) = server.await {
                log::error!("WebUI server error: {e}");
            }
        });
        *self.server_handle.write().await = Some(handle);

        log::info!("WebUI server started at http://{actual_addr}");
        Ok(actual_addr)
    }

    pub async fn stop(&self) -> Result<(), String> {
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.write().await.take() {
            handle.await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
