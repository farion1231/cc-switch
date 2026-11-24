//! HTTP handlers for the LAN Mobile Web UI control plane.
//!
//! Security invariants:
//! - Public routes: `/`, `/mobile.html`, `/assets/*`, `/manifest.json`, `/icons/*`,
//!   `/api/auth/status`, `/api/auth/session`, `/api/auth/logout`.
//! - All other `/api/*` routes require a valid `HttpOnly; SameSite=Strict` session
//!   cookie derived from the active access token (constant-time verified).
//! - No endpoint can read or modify the LAN Web config or token (desktop-only).
//! - Provider endpoints strip `settings_config` / API keys / tokens completely.
//! - Mutating endpoints (`/switch`, `/proxy/start`, `/proxy/stop`) require
//!   `allow_control == true` and emit desktop Tauri events + refresh the tray menu.

use super::config::{
    derive_session_cookie, verify_access_token, LanWebConfig, SESSION_COOKIE_NAME,
    SESSION_MAX_AGE_SECS,
};
use super::security::has_valid_session_cookie;
use crate::app_config::AppType;
use crate::proxy::types::{ActiveTarget, ProviderHealth, ProxyStatus, ProxyTakeoverStatus};
use crate::services::usage_stats::{
    DailyStats, LogFilters, ModelStats, ProviderStats, RequestLogDetail, UsageSummary,
    SPEED_ESTIMATE_MIN_DURATION_MS, SPEED_ESTIMATE_MIN_OUTPUT_TOKENS, SPEED_MIN_GENERATION_MS,
    SPEED_MIN_OUTPUT_TOKENS,
};
use crate::services::ProviderService;
use crate::store::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

const APP_ICON_PNG: &[u8] = include_bytes!("../../icons/128x128@2x.png");

#[derive(Clone)]
pub struct LanWebRouterState {
    pub app_state: AppState,
    pub config: Arc<RwLock<LanWebConfig>>,
    pub app_handle: Option<tauri::AppHandle>,
    pub dist_dir_override: Option<PathBuf>,
}

impl LanWebRouterState {
    async fn is_authenticated(&self, headers: &HeaderMap) -> bool {
        let token = self.config.read().await.token.clone();
        has_valid_session_cookie(headers, &token)
    }

    async fn allow_control(&self) -> bool {
        self.config.read().await.allow_control
    }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Authentication required" })),
    )
        .into_response()
}

fn forbidden_readonly_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Remote control is disabled. Enable 'Allow remote control' in desktop Settings → Network."
        })),
    )
        .into_response()
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

// ============================================================
// Auth Endpoints (Public)
// ============================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub allow_control: bool,
    pub version: String,
}

pub async fn get_auth_status(
    State(state): State<LanWebRouterState>,
    headers: HeaderMap,
) -> Json<AuthStatusResponse> {
    let cfg = state.config.read().await;
    let authenticated = has_valid_session_cookie(&headers, &cfg.token);
    Json(AuthStatusResponse {
        authenticated,
        allow_control: if authenticated {
            cfg.allow_control
        } else {
            false
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub token: String,
}

pub async fn create_session(
    State(state): State<LanWebRouterState>,
    Json(payload): Json<CreateSessionRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    if !verify_access_token(&payload.token, &cfg.token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid access token" })),
        )
            .into_response();
    }

    let cookie_val = derive_session_cookie(&cfg.token);
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={cookie_val}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_MAX_AGE_SECS}"
    );

    let mut response = (
        StatusCode::OK,
        Json(AuthStatusResponse {
            authenticated: true,
            allow_control: cfg.allow_control,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
    )
        .into_response();

    if let Ok(val) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, val);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub async fn logout_session() -> Response {
    let clear_cookie =
        format!("{SESSION_COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    let mut response = (StatusCode::OK, Json(json!({ "authenticated": false }))).into_response();
    if let Ok(val) = HeaderValue::from_str(&clear_cookie) {
        response.headers_mut().append(header::SET_COOKIE, val);
    }
    response
}

// ============================================================
// Overview Endpoint (Protected)
// ============================================================

#[derive(Deserialize)]
pub struct OverviewQuery {
    pub days: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAppSnapshot {
    pub app: String,
    pub current_provider_id: Option<String>,
    pub current_provider_name: Option<String>,
    pub provider_count: usize,
}

/// CamelCase DTO for `ActiveTarget` (upstream `proxy::types::ActiveTarget` uses snake_case).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeActiveTarget {
    pub app_type: String,
    pub provider_name: String,
    pub provider_id: String,
}

impl From<ActiveTarget> for SafeActiveTarget {
    fn from(t: ActiveTarget) -> Self {
        Self {
            app_type: t.app_type,
            provider_name: t.provider_name,
            provider_id: t.provider_id,
        }
    }
}

/// CamelCase DTO for `ProxyStatus` (upstream `proxy::types::ProxyStatus` uses snake_case).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeProxyStatus {
    pub running: bool,
    pub address: String,
    pub port: u16,
    pub active_connections: usize,
    pub total_requests: u64,
    pub success_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f32,
    pub uptime_seconds: u64,
    pub current_provider: Option<String>,
    pub current_provider_id: Option<String>,
    pub last_request_at: Option<String>,
    pub last_error: Option<String>,
    pub failover_count: u64,
    pub active_targets: Vec<SafeActiveTarget>,
}

impl From<ProxyStatus> for SafeProxyStatus {
    fn from(s: ProxyStatus) -> Self {
        Self {
            running: s.running,
            address: s.address,
            port: s.port,
            active_connections: s.active_connections,
            total_requests: s.total_requests,
            success_requests: s.success_requests,
            failed_requests: s.failed_requests,
            success_rate: s.success_rate,
            uptime_seconds: s.uptime_seconds,
            current_provider: s.current_provider,
            current_provider_id: s.current_provider_id,
            last_request_at: s.last_request_at,
            last_error: s.last_error,
            failover_count: s.failover_count,
            active_targets: s
                .active_targets
                .into_iter()
                .map(SafeActiveTarget::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewResponse {
    pub days: u32,
    pub summary: UsageSummary,
    pub daily_trends: Vec<DailyStats>,
    pub provider_stats: Vec<ProviderStats>,
    pub model_stats: Vec<ModelStats>,
    pub aggregate_tps: Option<f64>,
    pub aggregate_tps_estimated: bool,
    pub active_apps: Vec<ActiveAppSnapshot>,
    pub proxy_status: SafeProxyStatus,
    pub allow_control: bool,
}

pub async fn get_overview(
    State(state): State<LanWebRouterState>,
    headers: HeaderMap,
    Query(query): Query<OverviewQuery>,
) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }

    let days = match query.days.unwrap_or(1) {
        1 => 1,
        7 => 7,
        30 => 30,
        other => other.clamp(1, 90),
    };
    let end_ts = Utc::now().timestamp();
    let start_ts = end_ts - i64::from(days) * 86_400;

    let db = state.app_state.db.clone();
    let db_result = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let summary = db
            .get_usage_summary(Some(start_ts), Some(end_ts), None, None, None)
            .map_err(|e| e.to_string())?;
        let daily_trends = db
            .get_daily_trends(Some(start_ts), Some(end_ts), None, None, None)
            .map_err(|e| e.to_string())?;
        let provider_stats = db
            .get_provider_stats(Some(start_ts), Some(end_ts), None, None, None)
            .map_err(|e| e.to_string())?;
        let model_stats = db
            .get_model_stats(Some(start_ts), Some(end_ts), None, None, None)
            .map_err(|e| e.to_string())?;

        let mut active_apps = Vec::new();
        for app_type in AppType::all() {
            let rows = db.get_provider_rows(&app_type).map_err(|e| e.to_string())?;
            if rows.readable.is_empty() {
                continue;
            }
            let current_id = db
                .get_current_provider(&app_type)
                .map_err(|e| e.to_string())?;
            let current_name = current_id
                .as_ref()
                .and_then(|cid| rows.readable.get(cid))
                .map(|p| p.name.clone());
            active_apps.push(ActiveAppSnapshot {
                app: app_type.as_str().to_string(),
                current_provider_id: current_id,
                current_provider_name: current_name,
                provider_count: rows.readable.len(),
            });
        }

        Ok((
            summary,
            daily_trends,
            provider_stats,
            model_stats,
            active_apps,
        ))
    })
    .await;

    let (summary, daily_trends, provider_stats, model_stats, active_apps) = match db_result {
        Ok(Ok(data)) => data,
        Ok(Err(err)) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err),
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Overview query failed: {err}"),
            )
        }
    };

    let (aggregate_tps, aggregate_tps_estimated) = compute_aggregate_tps(&model_stats);
    let proxy_status = SafeProxyStatus::from(
        state
            .app_state
            .proxy_service
            .get_status()
            .await
            .unwrap_or_default(),
    );
    let allow_control = state.allow_control().await;

    Json(OverviewResponse {
        days,
        summary,
        daily_trends,
        provider_stats,
        model_stats,
        aggregate_tps,
        aggregate_tps_estimated,
        active_apps,
        proxy_status,
        allow_control,
    })
    .into_response()
}

fn compute_aggregate_tps(models: &[ModelStats]) -> (Option<f64>, bool) {
    let exact_tokens: u64 = models.iter().map(|m| m.speed_output_tokens).sum();
    let exact_ms: u64 = models.iter().map(|m| m.speed_generation_ms).sum();
    if exact_tokens > 0 && exact_ms > 0 {
        return (
            Some(exact_tokens as f64 / (exact_ms as f64 / 1000.0)),
            false,
        );
    }
    let est_tokens: u64 = models.iter().map(|m| m.est_speed_output_tokens).sum();
    let est_ms: u64 = models.iter().map(|m| m.est_speed_duration_ms).sum();
    if est_tokens > 0 && est_ms > 0 {
        return (Some(est_tokens as f64 / (est_ms as f64 / 1000.0)), true);
    }
    (None, false)
}

// ============================================================
// Providers Endpoints (Protected, Secret-Stripped)
// ============================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeProviderHealth {
    pub is_healthy: bool,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
    pub updated_at: String,
}

impl From<ProviderHealth> for SafeProviderHealth {
    fn from(h: ProviderHealth) -> Self {
        Self {
            is_healthy: h.is_healthy,
            consecutive_failures: h.consecutive_failures,
            last_error: h.last_error,
            updated_at: h.updated_at,
        }
    }
}

/// Safe provider view — strictly excludes `settings_config` (API keys, auth tokens,
/// custom headers, OAuth secrets) and `meta` credentials.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeProvider {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    pub is_current: bool,
    pub in_failover_queue: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<SafeProviderHealth>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeAppProviders {
    pub app: String,
    pub current_provider_id: Option<String>,
    pub providers: Vec<SafeProvider>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersListResponse {
    pub allow_control: bool,
    pub apps: Vec<SafeAppProviders>,
}

pub async fn get_providers(State(state): State<LanWebRouterState>, headers: HeaderMap) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }

    let db = state.app_state.db.clone();
    let apps_raw = tokio::task::spawn_blocking(move || -> Result<Vec<_>, String> {
        let mut out = Vec::new();
        for app_type in AppType::all() {
            let rows = db.get_provider_rows(&app_type).map_err(|e| e.to_string())?;
            if rows.readable.is_empty() {
                continue;
            }
            let current_id = db
                .get_current_provider(&app_type)
                .map_err(|e| e.to_string())?;
            let providers: Vec<_> = rows.readable.into_values().collect();
            out.push((app_type, current_id, providers));
        }
        Ok(out)
    })
    .await;

    let apps_raw = match apps_raw {
        Ok(Ok(data)) => data,
        Ok(Err(err)) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err),
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load providers: {err}"),
            )
        }
    };

    let mut apps = Vec::with_capacity(apps_raw.len());
    for (app_type, current_id, providers) in apps_raw {
        let mut safe_list = Vec::with_capacity(providers.len());
        for p in providers {
            let health = state
                .app_state
                .db
                .get_provider_health(&p.id, &app_type)
                .await
                .ok()
                .map(SafeProviderHealth::from);
            let is_current = current_id.as_deref() == Some(p.id.as_str());
            safe_list.push(SafeProvider {
                id: p.id,
                name: p.name,
                category: p.category,
                icon: p.icon,
                icon_color: p.icon_color,
                website_url: p.website_url,
                is_current,
                in_failover_queue: p.in_failover_queue,
                health,
            });
        }
        apps.push(SafeAppProviders {
            app: app_type.as_str().to_string(),
            current_provider_id: current_id,
            providers: safe_list,
        });
    }

    let allow_control = state.allow_control().await;
    Json(ProvidersListResponse {
        allow_control,
        apps,
    })
    .into_response()
}

pub async fn switch_provider(
    State(state): State<LanWebRouterState>,
    headers: HeaderMap,
    Path((app, id)): Path<(String, String)>,
) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }
    if !state.allow_control().await {
        return forbidden_readonly_response();
    }

    let app_type = match AppType::from_str(&app) {
        Ok(t) => t,
        Err(err) => return error_response(StatusCode::BAD_REQUEST, err.to_string()),
    };

    let app_state = state.app_state.clone();
    let app_type_clone = app_type.clone();
    let id_clone = id.clone();

    let result = tokio::task::spawn_blocking(move || {
        ProviderService::switch(&app_state, app_type_clone, &id_clone).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(switch_result)) => {
            // Live sync desktop UI + tray menu immediately.
            if let Some(handle) = &state.app_handle {
                emit_desktop_provider_switched(handle, &state.app_state, &app_type, &id);
            }
            Json(json!({
                "ok": true,
                "app": app_type.as_str(),
                "providerId": id,
                "warnings": switch_result.warnings,
            }))
            .into_response()
        }
        Ok(Err(err)) => error_response(StatusCode::BAD_REQUEST, err),
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Provider switch task failed: {err}"),
        ),
    }
}

fn emit_desktop_provider_switched(
    handle: &tauri::AppHandle,
    app_state: &AppState,
    app_type: &AppType,
    provider_id: &str,
) {
    let proxy_enabled = crate::mode::current::is_proxy(app_type);
    let auto_failover = app_state.db.get_proxy_flags_sync(app_type).1;
    let event_data = json!({
        "appType": app_type.as_str(),
        "proxyEnabled": proxy_enabled,
        "autoFailoverEnabled": auto_failover,
        "providerId": provider_id,
    });
    let _ = handle.emit("proxy-flags-changed", event_data.clone());
    let _ = handle.emit("provider-switched", event_data);
    crate::tray::refresh_tray_menu(handle);
}

// ============================================================
// Logs Endpoint (Protected)
// ============================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub app: Option<String>,
    pub status: Option<u16>,
    /// Optional status class filter: `"ok"` (200..300) or `"err"` (>= 400).
    pub status_class: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeRequestLog {
    pub request_id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub app_type: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub total_cost_usd: String,
    pub is_streaming: bool,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub tps: Option<f64>,
    pub tps_estimated: bool,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source: Option<String>,
}

impl From<RequestLogDetail> for SafeRequestLog {
    fn from(log: RequestLogDetail) -> Self {
        let (tps, tps_estimated) = compute_log_tps(&log);
        Self {
            request_id: log.request_id,
            provider_id: log.provider_id,
            provider_name: log.provider_name,
            app_type: log.app_type,
            model: log.model,
            input_tokens: log.input_tokens,
            output_tokens: log.output_tokens,
            cache_read_tokens: log.cache_read_tokens,
            cache_creation_tokens: log.cache_creation_tokens,
            total_cost_usd: log.total_cost_usd,
            is_streaming: log.is_streaming,
            latency_ms: log.latency_ms,
            first_token_ms: log.first_token_ms,
            duration_ms: log.duration_ms,
            tps,
            tps_estimated,
            status_code: log.status_code,
            error_message: log.error_message,
            created_at: log.created_at,
            data_source: log.data_source,
        }
    }
}

fn compute_log_tps(log: &RequestLogDetail) -> (Option<f64>, bool) {
    let is_proxy = log.data_source.as_deref().unwrap_or("proxy") == "proxy";
    if is_proxy {
        if let Some(ft) = log.first_token_ms {
            let gen_ms = log.latency_ms.saturating_sub(ft);
            if i64::from(log.output_tokens) >= SPEED_MIN_OUTPUT_TOKENS
                && gen_ms as i64 >= SPEED_MIN_GENERATION_MS
            {
                return (
                    Some(f64::from(log.output_tokens) / (gen_ms as f64 / 1000.0)),
                    false,
                );
            }
        }
    } else if i64::from(log.output_tokens) >= SPEED_ESTIMATE_MIN_OUTPUT_TOKENS
        && log.latency_ms as i64 >= SPEED_ESTIMATE_MIN_DURATION_MS
    {
        return (
            Some(f64::from(log.output_tokens) / (log.latency_ms as f64 / 1000.0)),
            true,
        );
    }
    (None, false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafePaginatedLogs {
    pub data: Vec<SafeRequestLog>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

pub async fn get_logs(
    State(state): State<LanWebRouterState>,
    headers: HeaderMap,
    Query(query): Query<LogsQuery>,
) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let status_class = query
        .status_class
        .as_deref()
        .map(str::trim)
        .filter(|s| matches!(*s, "ok" | "err"))
        .map(str::to_string);
    let filters = LogFilters {
        app_type: query.app.filter(|s| !s.trim().is_empty()),
        provider_name: None,
        model: query.model.filter(|s| !s.trim().is_empty()),
        status_code: query.status,
        start_date: None,
        end_date: None,
    };

    let db = state.app_state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<SafePaginatedLogs, String> {
        if let Some(class) = status_class {
            // Filter across the most recent 500 matching rows server-side so pagination
            // reflects the selected status class rather than filtering only one page.
            let batch = db
                .get_request_logs(&filters, 1, 500)
                .map_err(|e| e.to_string())?;
            let matched: Vec<SafeRequestLog> = batch
                .data
                .into_iter()
                .filter(|row| match class.as_str() {
                    "ok" => (200..300).contains(&row.status_code),
                    "err" => row.status_code >= 400,
                    _ => true,
                })
                .map(SafeRequestLog::from)
                .collect();
            let total = matched.len() as u32;
            let start = ((page - 1) as usize).saturating_mul(page_size as usize);
            let page_rows = if start < matched.len() {
                let end = (start + page_size as usize).min(matched.len());
                matched[start..end].to_vec()
            } else {
                Vec::new()
            };
            return Ok(SafePaginatedLogs {
                data: page_rows,
                total,
                page,
                page_size,
            });
        }

        let paginated = db
            .get_request_logs(&filters, page, page_size)
            .map_err(|e| e.to_string())?;
        Ok(SafePaginatedLogs {
            data: paginated
                .data
                .into_iter()
                .map(SafeRequestLog::from)
                .collect(),
            total: paginated.total,
            page: paginated.page,
            page_size: paginated.page_size,
        })
    })
    .await;

    match result {
        Ok(Ok(paginated)) => Json(paginated).into_response(),
        Ok(Err(err)) => error_response(StatusCode::INTERNAL_SERVER_ERROR, err),
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Logs query failed: {err}"),
        ),
    }
}

// ============================================================
// Proxy Endpoints (Protected; Start/Stop gated by allowControl)
// ============================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatusApiResponse {
    pub status: SafeProxyStatus,
    pub takeover: ProxyTakeoverStatus,
    pub allow_control: bool,
}

pub async fn get_proxy_status(
    State(state): State<LanWebRouterState>,
    headers: HeaderMap,
) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }

    let status = SafeProxyStatus::from(
        state
            .app_state
            .proxy_service
            .get_status()
            .await
            .unwrap_or_default(),
    );
    let takeover = state
        .app_state
        .proxy_service
        .get_takeover_status()
        .await
        .unwrap_or_default();
    let allow_control = state.allow_control().await;

    Json(ProxyStatusApiResponse {
        status,
        takeover,
        allow_control,
    })
    .into_response()
}

pub async fn start_proxy(State(state): State<LanWebRouterState>, headers: HeaderMap) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }
    if !state.allow_control().await {
        return forbidden_readonly_response();
    }

    match state.app_state.proxy_service.start().await {
        Ok(info) => {
            if let Some(handle) = &state.app_handle {
                crate::tray::refresh_tray_menu(handle);
            }
            let status = SafeProxyStatus::from(
                state
                    .app_state
                    .proxy_service
                    .get_status()
                    .await
                    .unwrap_or_default(),
            );
            let takeover = state
                .app_state
                .proxy_service
                .get_takeover_status()
                .await
                .unwrap_or_default();
            Json(json!({
                "ok": true,
                "info": info,
                "status": status,
                "takeover": takeover,
            }))
            .into_response()
        }
        Err(err) => error_response(StatusCode::BAD_REQUEST, err),
    }
}

pub async fn stop_proxy(State(state): State<LanWebRouterState>, headers: HeaderMap) -> Response {
    if !state.is_authenticated(&headers).await {
        return unauthorized_response();
    }
    if !state.allow_control().await {
        return forbidden_readonly_response();
    }

    // Mirror desktop `commands::proxy::stop_proxy_server` takeover safety guard:
    // refuse to stop while any app's live config is still routed to the local proxy.
    match state.app_state.proxy_service.get_takeover_status().await {
        Ok(takeover)
            if takeover.claude
                || takeover.codex
                || takeover.gemini
                || takeover.grokbuild
                || takeover.claude_desktop =>
        {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Cannot stop local proxy while one or more apps are still in proxy takeover mode. Disable takeover in desktop Settings → Routing first.",
            );
        }
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err),
        Ok(_) => {}
    }

    match state.app_state.proxy_service.stop().await {
        Ok(()) => {
            if let Some(handle) = &state.app_handle {
                crate::tray::refresh_tray_menu(handle);
            }
            let status = SafeProxyStatus::from(
                state
                    .app_state
                    .proxy_service
                    .get_status()
                    .await
                    .unwrap_or_default(),
            );
            let takeover = state
                .app_state
                .proxy_service
                .get_takeover_status()
                .await
                .unwrap_or_default();
            Json(json!({
                "ok": true,
                "status": status,
                "takeover": takeover,
            }))
            .into_response()
        }
        Err(err) => error_response(StatusCode::BAD_REQUEST, err),
    }
}

// ============================================================
// PWA Manifest, Icons & Static Mobile SPA Assets (Public)
// ============================================================

pub async fn serve_manifest() -> Response {
    let manifest = json!({
        "name": "CC Switch",
        "short_name": "CC Switch",
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": "#ffffff",
        "theme_color": "#f97316",
        "icons": [
            { "src": "/icons/apple-touch-icon.png", "sizes": "256x256", "type": "image/png" },
            { "src": "/icons/icon-256.png", "sizes": "256x256", "type": "image/png" }
        ]
    });

    let mut response = (StatusCode::OK, Json(manifest)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/manifest+json; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response
}

pub async fn serve_app_icon() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(APP_ICON_PNG))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub async fn serve_mobile_index(State(state): State<LanWebRouterState>) -> Response {
    serve_dist_asset(&state, "mobile.html").unwrap_or_else(|| {
        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(
                "<!doctype html><meta charset=\"utf-8\"><title>CC Switch Mobile</title>\
                 <body style=\"font-family:system-ui,sans-serif;padding:2rem;max-width:480px;margin:auto\">\
                 <h2>Mobile Web UI bundle not built</h2>\
                 <p>Run <code>pnpm build:renderer</code> to generate <code>dist/mobile.html</code>.</p>\
                 </body>",
            ))
            .unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())
    })
}

pub async fn serve_static_asset(
    State(state): State<LanWebRouterState>,
    Path(asset_path): Path<String>,
) -> Response {
    let rel = format!("assets/{asset_path}");
    serve_dist_asset(&state, &rel)
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "Not found").into_response())
}

fn serve_dist_asset(state: &LanWebRouterState, rel_path: &str) -> Option<Response> {
    let clean = sanitize_asset_path(rel_path)?;

    // 1. Prefer Tauri's embedded `dist/` bundle in packaged/release builds.
    if let Some(handle) = &state.app_handle {
        if let Some(asset) = handle.asset_resolver().get(clean.clone()) {
            return Some(build_asset_response(&clean, asset.bytes));
        }
    }

    // 2. Fallback to `dist/` on disk (dev mode, local preview, or unit tests),
    // verifying canonical containment to prevent symlink escape outside `dist/`.
    for root in candidate_dist_dirs(state.dist_dir_override.as_ref()) {
        let Ok(canon_root) = std::fs::canonicalize(&root) else {
            continue;
        };
        let candidate = root.join(&clean);
        let Ok(canon_file) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if canon_file.starts_with(&canon_root) && canon_file.is_file() {
            if let Ok(bytes) = std::fs::read(&canon_file) {
                return Some(build_asset_response(&clean, bytes));
            }
        }
    }

    None
}

fn sanitize_asset_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
        parts.push(seg);
    }
    Some(parts.join("/"))
}

fn candidate_dist_dirs(override_dir: Option<&PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = override_dir {
        dirs.push(dir.clone());
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("dist"));
        if let Some(parent) = cwd.parent() {
            dirs.push(parent.join("dist"));
        }
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"));
    dirs
}

fn build_asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = guess_mime_type(path);
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn guess_mime_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if lower.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lower.ends_with(".woff2") {
        "font/woff2"
    } else if lower.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}
