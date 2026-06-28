use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use crate::hermes_config::{self, MemoryKind};
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

/// Error string returned when the local Hermes Web UI can't be reached. Kept in
/// sync with `HERMES_WEB_OFFLINE_ERROR` in src/hooks/useHermes.ts and the Tauri
/// command in src/commands/hermes.rs.
const HERMES_WEB_OFFLINE_ERROR: &str = "hermes_web_offline";

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/model-config", get(get_model_config))
        .route("/memory-limits", get(get_memory_limits))
        .route("/memory/:kind", get(get_memory))
        .route("/memory/:kind", put(set_memory))
        .route("/memory/:kind/enabled", post(set_memory_enabled))
        .route("/open-web-ui", post(open_web_ui))
        .route("/launch-dashboard", post(launch_dashboard))
}

fn parse_kind(raw: &str) -> Result<MemoryKind, String> {
    match raw.to_ascii_lowercase().as_str() {
        "memory" => Ok(MemoryKind::Memory),
        "user" => Ok(MemoryKind::User),
        other => Err(format!("Invalid memory kind: {other}")),
    }
}

async fn get_model_config(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Option<hermes_config::HermesModelConfig>>> {
    match hermes_config::get_model_config() {
        Ok(cfg) => Json(ApiResponse::success(cfg)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_memory_limits(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<hermes_config::HermesMemoryLimits>> {
    match hermes_config::read_memory_limits() {
        Ok(limits) => Json(ApiResponse::success(limits)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_memory(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(kind): Path<String>,
) -> Json<ApiResponse<String>> {
    let kind = match parse_kind(&kind) {
        Ok(k) => k,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match hermes_config::read_memory(kind) {
        Ok(content) => Json(ApiResponse::success(content)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct SetMemoryRequest {
    content: String,
}

async fn set_memory(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(kind): Path<String>,
    Json(body): Json<SetMemoryRequest>,
) -> Json<ApiResponse<bool>> {
    let kind = match parse_kind(&kind) {
        Ok(k) => k,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match hermes_config::write_memory(kind, &body.content) {
        Ok(()) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct SetMemoryEnabledRequest {
    enabled: bool,
}

async fn set_memory_enabled(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(kind): Path<String>,
    Json(body): Json<SetMemoryEnabledRequest>,
) -> Json<ApiResponse<hermes_config::HermesWriteOutcome>> {
    let kind = match parse_kind(&kind) {
        Ok(k) => k,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match hermes_config::set_memory_enabled(kind, body.enabled) {
        Ok(outcome) => Json(ApiResponse::success(outcome)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct OpenWebUiRequest {
    #[serde(default)]
    path: Option<String>,
}

/// Probe the local Hermes Web UI server-side and return the resolved URL the
/// browser should open. Mirrors the desktop `open_hermes_web_ui` probe so the
/// frontend can branch on `hermes_web_offline`.
async fn open_web_ui(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<OpenWebUiRequest>,
) -> Json<ApiResponse<String>> {
    let port = std::env::var("HERMES_WEB_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .unwrap_or(9119);

    let base = format!("http://127.0.0.1:{port}");
    let probe_url = format!("{base}/api/status");

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(1200))
        .no_proxy()
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Json(ApiResponse::error(format!(
                "failed to build probe client: {e}"
            )))
        }
    };

    if client.get(&probe_url).send().await.is_err() {
        return Json(ApiResponse::error(HERMES_WEB_OFFLINE_ERROR.to_string()));
    }

    let target = match body.path.as_deref() {
        Some(p) if p.starts_with('/') => format!("{base}{p}"),
        Some(p) if !p.is_empty() => format!("{base}/{p}"),
        _ => format!("{base}/"),
    };

    Json(ApiResponse::success(target))
}

async fn launch_dashboard(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let result = tokio::task::spawn_blocking(|| {
        crate::commands::launch_terminal_running("hermes dashboard", "hermes_dashboard")
    })
    .await;

    match result {
        Ok(Ok(())) => Json(ApiResponse::success(true)),
        Ok(Err(e)) => Json(ApiResponse::error(e)),
        Err(e) => Json(ApiResponse::error(format!("launch task join error: {e}"))),
    }
}
