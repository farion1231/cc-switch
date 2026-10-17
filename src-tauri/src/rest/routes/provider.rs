//! Phase 2a — provider CRUD + 测试簇。
//!
//! 镜像 `commands/provider.rs` 的 service 调用。handler 通过 `ProviderService` 调用，
//! 绝不直访 DB（R-003）。

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::Json;
use serde::Deserialize;

use crate::provider::Provider;
use crate::rest::error::RestError;
use crate::services::provider::{EditorSave, ProviderService, ProviderSortUpdate};
use crate::store::AppState;

use super::common::{parse_app, AppQuery};

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/providers", post(add_provider))
        .route("/control/v1/providers/sort", patch(update_sort_order))
        .route("/control/v1/providers/editor-view", post(editor_view))
        .route(
            "/control/v1/providers/:id",
            axum::routing::put(update_provider),
        )
        .route("/control/v1/providers/:id", delete(delete_provider))
        .route(
            "/control/v1/providers/:id/custom-endpoints",
            get(get_custom_endpoints),
        )
        .route(
            "/control/v1/providers/:id/custom-endpoints",
            post(add_custom_endpoint),
        )
        .route(
            "/control/v1/providers/:id/custom-endpoints",
            delete(remove_custom_endpoint),
        )
        .route("/control/v1/providers/test-endpoints", post(test_endpoints))
}

#[derive(Deserialize)]
struct AddBody {
    app: String,
    provider: Provider,
    #[serde(rename = "addToLive", default)]
    add_to_live: Option<bool>,
    #[serde(rename = "editorSave")]
    editor_save: Option<EditorSave>,
}

async fn add_provider(
    State(state): State<AppState>,
    Json(body): Json<AddBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let add_to_live = body.add_to_live.unwrap_or(true);
    let provider = body.provider;
    let editor_save = body.editor_save;
    let ok = tokio::task::spawn_blocking(move || {
        ProviderService::add_from_editor(&state, app_type, provider, add_to_live, editor_save)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": ok })))
}

#[derive(Deserialize)]
struct UpdateBody {
    app: String,
    provider: Provider,
    #[serde(rename = "originalId")]
    original_id: Option<String>,
    #[serde(rename = "editorSave")]
    editor_save: Option<EditorSave>,
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let original_id = body.original_id.or(Some(id));
    let provider = body.provider;
    let editor_save = body.editor_save;
    let ok = tokio::task::spawn_blocking(move || {
        ProviderService::update_from_editor(
            &state,
            app_type,
            original_id.as_deref(),
            provider,
            editor_save,
        )
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": ok })))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<AppQuery>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&q.app)?;
    let ok = tokio::task::spawn_blocking(move || {
        ProviderService::delete(&state, app_type, &id).map(|_| true)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": ok })))
}

#[derive(Deserialize)]
struct EditorViewBody {
    app: String,
    #[serde(rename = "settingsConfig")]
    settings_config: serde_json::Value,
    category: Option<String>,
    #[serde(rename = "providerId")]
    provider_id: Option<String>,
}

async fn editor_view(
    State(state): State<AppState>,
    Json(body): Json<EditorViewBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let view = tokio::task::spawn_blocking(move || {
        let category = ProviderService::editor_category(
            &state,
            &app_type,
            body.provider_id.as_deref(),
            body.category,
        )?;
        ProviderService::editor_view(&state, app_type, &body.settings_config, category.as_deref())
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(view))
}

async fn get_custom_endpoints(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<AppQuery>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&q.app)?;
    let eps = tokio::task::spawn_blocking(move || {
        ProviderService::get_custom_endpoints(&state, app_type, &id)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(eps))
}

#[derive(Deserialize)]
struct CustomEndpointBody {
    app: String,
    url: String,
}

async fn add_custom_endpoint(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CustomEndpointBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let url = body.url;
    tokio::task::spawn_blocking(move || {
        ProviderService::add_custom_endpoint(&state, app_type, &id, url)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RemoveEndpointBody {
    app: String,
    url: String,
}

async fn remove_custom_endpoint(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RemoveEndpointBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let url = body.url;
    tokio::task::spawn_blocking(move || {
        ProviderService::remove_custom_endpoint(&state, app_type, &id, url)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TestEndpointsBody {
    urls: Vec<String>,
    #[serde(rename = "timeoutSecs")]
    timeout_secs: Option<u64>,
}

async fn test_endpoints(
    State(_state): State<AppState>,
    Json(body): Json<TestEndpointsBody>,
) -> Result<impl IntoResponse, RestError> {
    let result = crate::services::SpeedtestService::test_endpoints(body.urls, body.timeout_secs)
        .await
        .map_err(|e| RestError::App(e.to_string()))?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct SortBody {
    app: String,
    updates: Vec<ProviderSortUpdate>,
}

async fn update_sort_order(
    State(state): State<AppState>,
    Json(body): Json<SortBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let updates = body.updates;
    let ok = tokio::task::spawn_blocking(move || {
        ProviderService::update_sort_order(&state, app_type, updates).map(|_| true)
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": ok })))
}
