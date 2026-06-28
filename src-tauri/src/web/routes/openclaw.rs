use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::openclaw_config;
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route(
            "/default-model",
            get(get_default_model).put(set_default_model),
        )
        .route(
            "/model-catalog",
            get(get_model_catalog).put(set_model_catalog),
        )
        .route(
            "/agents-defaults",
            get(get_agents_defaults).put(set_agents_defaults),
        )
        .route("/env", get(get_env).put(set_env))
        .route("/tools", get(get_tools).put(set_tools))
        .route("/health", get(scan_health))
        .route("/live-provider/:id", get(get_live_provider))
}

async fn get_default_model(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Option<openclaw_config::OpenClawDefaultModel>>> {
    match openclaw_config::get_default_model() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn set_default_model(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(model): Json<openclaw_config::OpenClawDefaultModel>,
) -> Json<ApiResponse<openclaw_config::OpenClawWriteOutcome>> {
    match openclaw_config::set_default_model(&model) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_model_catalog(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Option<HashMap<String, openclaw_config::OpenClawModelCatalogEntry>>>> {
    match openclaw_config::get_model_catalog() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn set_model_catalog(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(catalog): Json<HashMap<String, openclaw_config::OpenClawModelCatalogEntry>>,
) -> Json<ApiResponse<openclaw_config::OpenClawWriteOutcome>> {
    match openclaw_config::set_model_catalog(&catalog) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_agents_defaults(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Option<openclaw_config::OpenClawAgentsDefaults>>> {
    match openclaw_config::get_agents_defaults() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn set_agents_defaults(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(defaults): Json<openclaw_config::OpenClawAgentsDefaults>,
) -> Json<ApiResponse<openclaw_config::OpenClawWriteOutcome>> {
    match openclaw_config::set_agents_defaults(&defaults) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_env(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<openclaw_config::OpenClawEnvConfig>> {
    match openclaw_config::get_env_config() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn set_env(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(env): Json<openclaw_config::OpenClawEnvConfig>,
) -> Json<ApiResponse<openclaw_config::OpenClawWriteOutcome>> {
    match openclaw_config::set_env_config(&env) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_tools(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<openclaw_config::OpenClawToolsConfig>> {
    match openclaw_config::get_tools_config() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn set_tools(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(tools): Json<openclaw_config::OpenClawToolsConfig>,
) -> Json<ApiResponse<openclaw_config::OpenClawWriteOutcome>> {
    match openclaw_config::set_tools_config(&tools) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn scan_health(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<openclaw_config::OpenClawHealthWarning>>> {
    match openclaw_config::scan_openclaw_config_health() {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_live_provider(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Option<serde_json::Value>>> {
    match openclaw_config::get_provider(&id) {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
