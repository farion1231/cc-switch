//! Phase 2e — profile 簇。

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::Json;
use serde::Deserialize;

use crate::commands::{CurrentProfileIds, ProfileDto, ProfilesResponse};
use crate::rest::error::RestError;
use crate::services::profile::{ProfileScope, ProfileService};
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/profiles", get(list_profiles))
        .route("/control/v1/profiles", post(create_profile))
        .route("/control/v1/profiles/:id", put(update_profile))
        .route("/control/v1/profiles/:id", delete(delete_profile))
        .route("/control/v1/profiles/:id/apply", post(apply_profile))
        .route(
            "/control/v1/profiles/current/:scope",
            axum::routing::delete(clear_current),
        )
}

async fn list_profiles(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let (profiles, current_ids) = tokio::task::spawn_blocking(move || {
        let profiles = ProfileService::list(&state)?;
        let current_ids = CurrentProfileIds {
            claude: state
                .db
                .get_current_profile_id(ProfileScope::Claude.as_str())?,
            claude_desktop: state
                .db
                .get_current_profile_id(ProfileScope::ClaudeDesktop.as_str())?,
            codex: state
                .db
                .get_current_profile_id(ProfileScope::Codex.as_str())?,
        };
        Ok::<_, crate::error::AppError>((profiles, current_ids))
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    let dtos: Vec<ProfileDto> = profiles.into_iter().map(ProfileDto::from).collect();
    Ok(Json(ProfilesResponse {
        profiles: dtos,
        current_ids,
    }))
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    scope: String,
}

async fn create_profile(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, RestError> {
    let scope = ProfileScope::parse(&body.scope)?;
    let p = tokio::task::spawn_blocking(move || ProfileService::create(&state, &body.name, scope))
        .await
        .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(ProfileDto::from(p)))
}

#[derive(Deserialize)]
struct UpdateBody {
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<impl IntoResponse, RestError> {
    let scope = body.scope.map(|s| ProfileScope::parse(&s)).transpose()?;
    let p = tokio::task::spawn_blocking(move || {
        ProfileService::update(
            &state,
            &id,
            body.name,
            body.resnapshot.unwrap_or(false),
            scope,
        )
    })
    .await
    .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(ProfileDto::from(p)))
}

async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, RestError> {
    tokio::task::spawn_blocking(move || ProfileService::delete(&state, &id))
        .await
        .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApplyQuery {
    scope: String,
}

async fn apply_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ApplyQuery>,
) -> Result<impl IntoResponse, RestError> {
    let scope = ProfileScope::parse(&q.scope)?;
    // apply 必须同步跑（内部 block_on 取切换锁），用 spawn_blocking
    let warnings = tokio::task::spawn_blocking(move || ProfileService::apply(&state, &id, scope))
        .await
        .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json(warnings))
}

async fn clear_current(
    State(state): State<AppState>,
    Path(scope): Path<String>,
) -> Result<impl IntoResponse, RestError> {
    let scope = ProfileScope::parse(&scope)?;
    state.db.set_current_profile_id(scope.as_str(), None)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
