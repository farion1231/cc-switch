use crate::app_config::AppType;
use crate::commands;
use crate::error::AppError;
use crate::provider::{Provider, UniversalProvider};
use crate::services::{ProviderService, ProviderSortUpdate};
use crate::store::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashSet, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};
use tauri::{path::BaseDirectory, AppHandle, Manager};
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tower_http::cors::{AllowCredentials, AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

const DEFAULT_WEBUI_HOST: &str = "127.0.0.1";
const DEFAULT_WEBUI_PORT: u16 = 15722;
const TOKEN_ENV: &str = "CC_SWITCH_WEBUI_TOKEN";
const SESSION_COOKIE: &str = "cc_switch_webui_session";
const HOST_ENV: &str = "CC_SWITCH_WEBUI_HOST";
const PORT_ENV: &str = "CC_SWITCH_WEBUI_PORT";

#[derive(Clone)]
struct WebUiState {
    app_state: Arc<AppState>,
    app_handle: AppHandle,
    password: Option<String>,
    sessions: Arc<RwLock<HashSet<String>>>,
}

pub struct WebUiServer {
    state: Arc<AppState>,
    app_handle: AppHandle,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    actual_addr: Arc<RwLock<Option<SocketAddr>>>,
    sessions: Arc<RwLock<HashSet<String>>>,
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
struct UrlRequest {
    url: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebUiLoginRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UniversalProviderRequest {
    provider: UniversalProvider,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdRequest {
    id: String,
}

fn parse_bool_env(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn configured_password() -> Option<String> {
    std::env::var(TOKEN_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn sanitize_password(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn session_from_cookie(headers: &header::HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn is_auth_exempt_path(path: &str) -> bool {
    matches!(
        path,
        "/health" | "/api/health" | "/api/webui/login" | "/api/webui/auth-status"
    )
}

fn public_webui_addr(addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        SocketAddr::from(([127, 0, 0, 1], addr.port()))
    } else {
        addr
    }
}

fn webui_address_from_settings() -> String {
    let settings = crate::settings::get_settings();
    let host = if settings.webui_host == "0.0.0.0" || settings.webui_host == "::" {
        "127.0.0.1".to_string()
    } else {
        settings.webui_host
    };
    format!("http://{}:{}", host, settings.webui_port)
}

fn is_allowed_cors_origin(origin: &HeaderValue, actual_addr: SocketAddr) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };

    if url.port_or_known_default() == Some(actual_addr.port()) {
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            return true;
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            return ip == public_webui_addr(actual_addr).ip();
        }
    }

    (host == "localhost" || host == "127.0.0.1")
        && matches!(url.port_or_known_default(), Some(3000 | 5173 | 1420))
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
    if is_auth_exempt_path(request.uri().path()) {
        return next.run(request).await;
    }

    let Some(_password) = state.password.as_deref() else {
        return next.run(request).await;
    };

    let session = session_from_cookie(request.headers());
    if let Some(session) = session {
        if state.sessions.read().await.contains(&session) {
            return next.run(request).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "WebUI requires password login.",
            "requiresLogin": true,
        })),
    )
        .into_response()
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "cc-switch-webui",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn webui_status(State(state): State<WebUiState>) -> Json<Value> {
    let settings = crate::settings::get_settings();
    Json(json!({
        "running": true,
        "enabled": settings.webui_enabled,
        "port": settings.webui_port,
        "host": settings.webui_host,
        "address": webui_address_from_settings(),
        "tokenSet": settings.webui_token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
        "authRequired": state.password.is_some(),
    }))
}

async fn webui_auth_status(
    State(state): State<WebUiState>,
    headers: header::HeaderMap,
) -> Json<Value> {
    let authenticated = if state.password.is_none() {
        true
    } else if let Some(session) = session_from_cookie(&headers) {
        state.sessions.read().await.contains(&session)
    } else {
        false
    };

    Json(json!({
        "authRequired": state.password.is_some(),
        "authenticated": authenticated,
    }))
}

async fn webui_login(
    State(state): State<WebUiState>,
    Json(payload): Json<WebUiLoginRequest>,
) -> Response {
    let Some(expected) = state.password.as_deref() else {
        return Json(json!({ "ok": true })).into_response();
    };

    if payload.password != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid WebUI password",
                "requiresLogin": true,
            })),
        )
            .into_response();
    }

    let session = uuid::Uuid::new_v4().to_string();
    state.sessions.write().await.insert(session.clone());
    let cookie =
        format!("{SESSION_COOKIE}={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

/// Browser client hitting this route means the server is already running.
async fn webui_start() -> Json<Value> {
    Json(json!(webui_address_from_settings()))
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

async fn get_providers(State(state): State<WebUiState>, Query(query): Query<AppQuery>) -> Response {
    let app_type = match app_type(&query.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::list(&state.app_state, app_type) {
        Ok(providers) => {
            let providers_vec: Vec<_> = providers.into_values().collect();
            Json(providers_vec).into_response()
        }
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

async fn remove_provider_from_live_config(
    State(state): State<WebUiState>,
    Json(payload): Json<DeleteProviderRequest>,
) -> Response {
    let app_type = match app_type(&payload.app) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match ProviderService::remove_from_live_config(&state.app_state, app_type, &payload.id) {
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
    match ProviderService::switch(&state.app_state, app_type.clone(), &payload.id) {
        Ok(result) => {
            // TODO: emit provider-switched event
            crate::tray::schedule_tray_refresh(&state.app_handle);
            Json(result).into_response()
        }
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
    match commands::import_default_config_test_hook(&state.app_state, app_type) {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn import_claude_desktop_providers_from_claude(State(state): State<WebUiState>) -> Response {
    match commands::import_claude_desktop_providers_from_claude_for_state(&state.app_state) {
        Ok(count) => Json(count).into_response(),
        Err(e) => command_error(e),
    }
}

async fn import_opencode_providers_from_live(State(state): State<WebUiState>) -> Response {
    match crate::services::provider::import_opencode_providers_from_live(&state.app_state) {
        Ok(count) => Json(count).into_response(),
        Err(e) => command_error(e),
    }
}

async fn import_openclaw_providers_from_live(State(state): State<WebUiState>) -> Response {
    match crate::services::provider::import_openclaw_providers_from_live(&state.app_state) {
        Ok(count) => Json(count).into_response(),
        Err(e) => command_error(e),
    }
}

async fn import_hermes_providers_from_live(State(state): State<WebUiState>) -> Response {
    match crate::services::provider::import_hermes_providers_from_live(&state.app_state) {
        Ok(count) => Json(count).into_response(),
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

async fn update_tray_menu(State(state): State<WebUiState>) -> Response {
    match crate::tray::create_tray_menu(&state.app_handle, &state.app_state) {
        Ok(new_menu) => {
            if let Some(tray) = state.app_handle.tray_by_id(crate::tray::TRAY_ID) {
                match tray.set_menu(Some(new_menu)) {
                    Ok(()) => Json(true).into_response(),
                    Err(e) => command_error(format!("更新托盘菜单失败: {e}")),
                }
            } else {
                Json(false).into_response()
            }
        }
        Err(e) => command_error(e),
    }
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

    if takeover.claude
        || takeover.codex
        || takeover.gemini
        || takeover.opencode
        || takeover.openclaw
    {
        return command_error(
            "仍有应用处于代理接管状态，请先在设置中关闭对应应用接管后再停止本地路由。",
        );
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
    Json(
        state
            .app_state
            .proxy_service
            .is_takeover_active()
            .await
            .unwrap_or(false),
    )
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
    match state
        .app_state
        .db
        .get_proxy_config_for_app(&payload.app_type)
        .await
    {
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

async fn get_global_proxy_url(State(state): State<WebUiState>) -> Response {
    match state.app_state.db.get_global_proxy_url() {
        Ok(value) => Json(value).into_response(),
        Err(e) => command_error(e),
    }
}

async fn set_global_proxy_url(
    State(state): State<WebUiState>,
    Json(payload): Json<UrlRequest>,
) -> Response {
    let url_opt = if payload.url.trim().is_empty() {
        None
    } else {
        Some(payload.url.as_str())
    };

    if let Err(e) = crate::proxy::http_client::validate_proxy(url_opt) {
        return command_error(e);
    }
    if let Err(e) = state.app_state.db.set_global_proxy_url(url_opt) {
        return command_error(e);
    }
    match crate::proxy::http_client::apply_proxy(url_opt) {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn test_proxy_url(Json(payload): Json<UrlRequest>) -> Response {
    match commands::test_proxy_url(payload.url).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_upstream_proxy_status() -> Json<commands::UpstreamProxyStatus> {
    Json(commands::get_upstream_proxy_status())
}

async fn get_default_cost_multiplier(
    State(state): State<WebUiState>,
    Json(payload): Json<AppTypeValueRequest>,
) -> Response {
    match state
        .app_state
        .db
        .get_default_cost_multiplier(&payload.app_type)
        .await
    {
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
    match state
        .app_state
        .db
        .get_pricing_model_source(&payload.app_type)
        .await
    {
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
        .set_pricing_model_source(
            &payload.app_type,
            payload.value.as_deref().unwrap_or("response"),
        )
        .await
    {
        Ok(()) => Json(Value::Null).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_universal_providers(State(state): State<WebUiState>) -> Response {
    match ProviderService::list_universal(&state.app_state) {
        Ok(providers) => Json(providers).into_response(),
        Err(e) => command_error(e),
    }
}

async fn get_universal_provider(
    State(state): State<WebUiState>,
    Query(query): Query<IdRequest>,
) -> Response {
    match ProviderService::get_universal(&state.app_state, &query.id) {
        Ok(provider) => Json(provider).into_response(),
        Err(e) => command_error(e),
    }
}

async fn upsert_universal_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<UniversalProviderRequest>,
) -> Response {
    match ProviderService::upsert_universal(&state.app_state, payload.provider) {
        Ok(result) => {
            // TODO: emit universal-provider-synced event
            Json(result).into_response()
        }
        Err(e) => command_error(e),
    }
}

async fn delete_universal_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<IdRequest>,
) -> Response {
    match ProviderService::delete_universal(&state.app_state, &payload.id) {
        Ok(result) => {
            // TODO: emit universal-provider-synced event
            Json(result).into_response()
        }
        Err(e) => command_error(e),
    }
}

async fn sync_universal_provider(
    State(state): State<WebUiState>,
    Json(payload): Json<IdRequest>,
) -> Response {
    match ProviderService::sync_universal_to_apps(&state.app_state, &payload.id) {
        Ok(result) => {
            // TODO: emit universal-provider-synced event
            Json(result).into_response()
        }
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
                payload
                    .get("pageSize")
                    .and_then(Value::as_u64)
                    .unwrap_or(50) as u32,
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string())
        }
        "get_request_detail" => db
            .get_request_detail(
                payload
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "queryProviderUsage" => {
            let app_type = match payload.get("app").and_then(Value::as_str) {
                Some(app) => match AppType::from_str(app) {
                    Ok(app_type) => app_type,
                    Err(e) => return command_error(e),
                },
                None => return command_error("missing app"),
            };
            let provider_id = payload
                .get("providerId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            ProviderService::query_usage(&state.app_state, app_type, provider_id)
                .await
                .map(|v| json!(v))
                .map_err(|e| e.to_string())
        }
        "testUsageScript" => {
            let app_type = match payload.get("app").and_then(Value::as_str) {
                Some(app) => match AppType::from_str(app) {
                    Ok(app_type) => app_type,
                    Err(e) => return command_error(e),
                },
                None => return command_error("missing app"),
            };
            ProviderService::test_usage_script(
                state.app_state.as_ref(),
                app_type,
                payload
                    .get("providerId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                payload
                    .get("scriptCode")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                payload.get("timeout").and_then(Value::as_u64).unwrap_or(10),
                payload.get("apiKey").and_then(Value::as_str),
                payload.get("baseUrl").and_then(Value::as_str),
                payload.get("accessToken").and_then(Value::as_str),
                payload.get("userId").and_then(Value::as_str),
                payload.get("templateType").and_then(Value::as_str),
            )
            .await
            .map(|v| json!(v))
            .map_err(|e| e.to_string())
        }
        "get_model_pricing" => {
            if let Err(e) = state.app_state.db.ensure_model_pricing_seeded() {
                Err(e.to_string())
            } else {
                let db = state.app_state.db.clone();
                let conn = match db.conn.lock() {
                    Ok(conn) => conn,
                    Err(e) => {
                        return command_error(AppError::Database(format!("Mutex lock failed: {e}")))
                    }
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
        "update_model_pricing" => {
            let model_id = payload
                .get("modelId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let display_name = payload
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if model_id.is_empty() {
                return command_error("模型 ID 不能为空");
            }
            if display_name.is_empty() {
                return command_error("显示名称不能为空");
            }
            let input_cost = payload
                .get("inputCost")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let output_cost = payload
                .get("outputCost")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let cache_read_cost = payload
                .get("cacheReadCost")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let cache_creation_cost = payload
                .get("cacheCreationCost")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            for (label, value) in [
                ("input_cost", input_cost),
                ("output_cost", output_cost),
                ("cache_read_cost", cache_read_cost),
                ("cache_creation_cost", cache_creation_cost),
            ] {
                match rust_decimal::Decimal::from_str(value) {
                    Ok(parsed) if parsed >= rust_decimal::Decimal::ZERO => {}
                    Ok(_) => return command_error(format!("{label} 价格必须为非负数: {value}")),
                    Err(e) => return command_error(format!("{label} 价格无效: {value} - {e}")),
                }
            }
            let result = {
                let conn = match state.app_state.db.conn.lock() {
                    Ok(guard) => guard,
                    Err(e) => return command_error(format!("Database lock failed: {e}")),
                };
                conn.execute(
                    "INSERT OR REPLACE INTO model_pricing (
                        model_id, display_name, input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        model_id,
                        display_name,
                        input_cost,
                        output_cost,
                        cache_read_cost,
                        cache_creation_cost
                    ],
                )
                .map(|_| Value::Null)
                .map_err(|e| format!("更新模型定价失败: {e}"))
            };
            result
        }
        "delete_model_pricing" => {
            let model_id = payload
                .get("modelId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = {
                let conn = match state.app_state.db.conn.lock() {
                    Ok(guard) => guard,
                    Err(e) => return command_error(format!("Database lock failed: {e}")),
                };
                conn.execute(
                    "DELETE FROM model_pricing WHERE model_id = ?1",
                    rusqlite::params![model_id],
                )
                .map(|_| Value::Null)
                .map_err(|e| format!("删除模型定价失败: {e}"))
            };
            result
        }
        "check_provider_limits" => state
            .app_state
            .db
            .check_provider_limits(
                payload
                    .get("providerId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                payload
                    .get("appType")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .map(|v| json!(v))
            .map_err(|e| e.to_string()),
        "sync_session_usage" => {
            let mut result =
                match crate::services::session_usage::sync_claude_session_logs(&state.app_state.db)
                {
                    Ok(result) => result,
                    Err(e) => return command_error(e),
                };
            if let Ok(codex_result) =
                crate::services::session_usage_codex::sync_codex_usage(&state.app_state.db)
            {
                result.imported += codex_result.imported;
                result.skipped += codex_result.skipped;
                result.files_scanned += codex_result.files_scanned;
                result.errors.extend(codex_result.errors);
            }
            if let Ok(opencode_result) =
                crate::services::session_usage_opencode::sync_opencode_usage(&state.app_state.db)
            {
                result.imported += opencode_result.imported;
                result.skipped += opencode_result.skipped;
                result.files_scanned += opencode_result.files_scanned;
                result.errors.extend(opencode_result.errors);
            }
            Ok(json!(result))
        }
        "get_usage_data_sources" => Ok(json!([])),
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "unsupported WebUI command" })),
            )
                .into_response()
        }
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
        .route("/api/webui/auth-status", get(webui_auth_status))
        .route("/api/webui/login", post(webui_login))
        .route("/api/webui/start", post(webui_start))
        .route("/api/webui/stop", post(webui_stop))
        .route("/api/webui/restart", post(webui_restart))
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/providers", get(get_providers))
        .route("/api/providers/current", get(get_current_provider))
        .route("/api/providers/add", post(add_provider))
        .route("/api/providers/update", post(update_provider))
        .route("/api/providers/delete", post(delete_provider))
        .route(
            "/api/providers/remove-live",
            post(remove_provider_from_live_config),
        )
        .route("/api/providers/switch", post(switch_provider))
        .route("/api/providers/import-default", post(import_default_config))
        .route("/api/providers/sort", post(update_sort_order))
        .route("/api/claude-desktop/status", get(get_claude_desktop_status))
        .route(
            "/api/claude-desktop/default-routes",
            get(get_claude_desktop_default_routes),
        )
        .route(
            "/api/claude-desktop/import-from-claude",
            post(import_claude_desktop_providers_from_claude),
        )
        .route(
            "/api/opencode/import-live",
            post(import_opencode_providers_from_live),
        )
        .route(
            "/api/opencode/live-provider-ids",
            get(get_opencode_live_provider_ids),
        )
        .route(
            "/api/openclaw/import-live",
            post(import_openclaw_providers_from_live),
        )
        .route(
            "/api/openclaw/live-provider-ids",
            get(get_openclaw_live_provider_ids),
        )
        .route(
            "/api/hermes/import-live",
            post(import_hermes_providers_from_live),
        )
        .route(
            "/api/hermes/live-provider-ids",
            get(get_hermes_live_provider_ids),
        )
        .route("/api/tray/update", post(update_tray_menu))
        .route("/api/proxy/start", post(start_proxy))
        .route("/api/proxy/stop", post(stop_proxy_server))
        .route(
            "/api/proxy/stop-with-restore",
            post(stop_proxy_with_restore),
        )
        .route("/api/proxy/status", get(get_proxy_status))
        .route("/api/proxy/running", get(is_proxy_running))
        .route(
            "/api/proxy/live-takeover-active",
            get(is_live_takeover_active),
        )
        .route("/api/proxy/takeover-status", get(get_proxy_takeover_status))
        .route("/api/proxy/takeover", post(set_proxy_takeover))
        .route("/api/proxy/switch-provider", post(switch_proxy_provider))
        .route(
            "/api/proxy/config",
            get(get_proxy_config).post(update_proxy_config),
        )
        .route(
            "/api/proxy/global-config",
            get(get_global_proxy_config).post(update_global_proxy_config),
        )
        .route(
            "/api/proxy/global-url",
            get(get_global_proxy_url).post(set_global_proxy_url),
        )
        .route("/api/proxy/test-url", post(test_proxy_url))
        .route("/api/proxy/upstream-status", get(get_upstream_proxy_status))
        .route("/api/proxy/app-config", post(get_proxy_config_for_app))
        .route(
            "/api/proxy/app-config/update",
            post(update_proxy_config_for_app),
        )
        .route(
            "/api/proxy/default-cost-multiplier",
            post(get_default_cost_multiplier),
        )
        .route(
            "/api/proxy/default-cost-multiplier/update",
            post(set_default_cost_multiplier),
        )
        .route(
            "/api/proxy/pricing-model-source",
            post(get_pricing_model_source),
        )
        .route(
            "/api/proxy/pricing-model-source/update",
            post(set_pricing_model_source),
        )
        .route("/api/models/fetch", post(fetch_models))
        .route("/api/universal-providers", get(get_universal_providers))
        .route("/api/universal-providers/get", get(get_universal_provider))
        .route(
            "/api/universal-providers/upsert",
            post(upsert_universal_provider),
        )
        .route(
            "/api/universal-providers/delete",
            post(delete_universal_provider),
        )
        .route(
            "/api/universal-providers/sync",
            post(sync_universal_provider),
        )
        .route("/api/usage/:command", post(usage_command))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

impl WebUiServer {
    pub fn new(state: Arc<AppState>, app_handle: AppHandle) -> Self {
        Self {
            state,
            app_handle,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
            actual_addr: Arc::new(RwLock::new(None)),
            sessions: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn should_start_from_env() -> bool {
        parse_bool_env("CC_SWITCH_WEBUI")
    }

    pub async fn start_from_env(&self) -> Result<SocketAddr, String> {
        self.start(configured_addr(), configured_password()).await
    }

    /// Start WebUI using persisted AppSettings (env vars still override if set)
    pub async fn start_from_settings(&self) -> Result<SocketAddr, String> {
        let settings = crate::settings::get_settings();

        // Env var override takes precedence
        let host = std::env::var(HOST_ENV).unwrap_or_else(|_| settings.webui_host.clone());
        let port = std::env::var(PORT_ENV)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(settings.webui_port);

        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], DEFAULT_WEBUI_PORT)));

        let password = configured_password().or_else(|| sanitize_password(settings.webui_token));

        self.start(addr, password).await
    }

    pub async fn is_running(&self) -> bool {
        self.shutdown_tx.read().await.is_some()
    }

    pub async fn get_status(&self) -> serde_json::Value {
        let running = self.is_running().await;
        let settings = crate::settings::get_settings();
        let address = self.public_address().await;
        serde_json::json!({
            "running": running,
            "host": settings.webui_host,
            "port": settings.webui_port,
            "address": address,
            "hasToken": settings.webui_token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
            "tokenSet": settings.webui_token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
            "authRequired": sanitize_password(settings.webui_token).is_some(),
            "enabled": settings.webui_enabled,
        })
    }

    fn find_dist_path(&self) -> Option<PathBuf> {
        self.app_handle
            .path()
            .resolve("_up_/dist", BaseDirectory::Resource)
            .ok()
            .filter(|p| p.exists())
            .or_else(|| {
                self.app_handle
                    .path()
                    .resolve("dist", BaseDirectory::Resource)
                    .ok()
                    .filter(|p| p.exists())
            })
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.join("dist")))
                    .filter(|p| p.exists())
            })
            .or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                cwd.join("dist").exists().then_some(cwd.join("dist"))
            })
            .or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                cwd.parent()?
                    .join("dist")
                    .exists()
                    .then_some(cwd.parent()?.join("dist"))
            })
    }

    pub async fn public_address(&self) -> Option<String> {
        self.actual_addr
            .read()
            .await
            .map(|addr| format!("http://{}", public_webui_addr(addr)))
    }

    pub async fn start(
        &self,
        addr: SocketAddr,
        password: Option<String>,
    ) -> Result<SocketAddr, String> {
        if self.shutdown_tx.read().await.is_some() {
            return Err("WebUI server is already running".to_string());
        }

        let password = sanitize_password(password);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("WebUI bind failed: {e}"))?;
        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("WebUI local address failed: {e}"))?;

        let state = WebUiState {
            app_state: self.state.clone(),
            app_handle: self.app_handle.clone(),
            password,
            sessions: self.sessions.clone(),
        };

        let dist_path = self.find_dist_path();

        let app = if let Some(dist) = dist_path {
            log::info!("WebUI serving static files from: {}", dist.display());
            // Serve API routes + static files with SPA fallback
            command_router(state.clone())
                .fallback_service(ServeDir::new(dist).append_index_html_on_directories(true))
        } else {
            log::warn!("WebUI dist/ not found, serving API only");
            command_router(state.clone())
        };

        let cors_addr = actual_addr;
        let app = app.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin, _parts| {
                    is_allowed_cors_origin(origin, cors_addr)
                }))
                .allow_credentials(AllowCredentials::yes())
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE]),
        );

        *self.shutdown_tx.write().await = Some(shutdown_tx);
        *self.actual_addr.write().await = Some(actual_addr);
        let actual_addr_slot = self.actual_addr.clone();
        let shutdown_slot = self.shutdown_tx.clone();
        let handle = tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(e) = server.await {
                log::error!("WebUI server error: {e}");
            }
            *actual_addr_slot.write().await = None;
            *shutdown_slot.write().await = None;
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
        *self.actual_addr.write().await = None;
        self.sessions.write().await.clear();
        Ok(())
    }
}
