use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;

use crate::app_config::{AppType, InstalledSkill, UnmanagedSkill};
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, MigrationResult, Skill, SkillBackupEntry, SkillRepo,
    SkillService, SkillStorageLocation, SkillUninstallResult, SkillUpdateInfo,
    SkillsShSearchResult,
};
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

// The web skills surface delegates to the exact same `SkillService` + `Database`
// used by the desktop commands (via `state.desktop()`), so behavior is identical
// in both runtimes. Endpoints mirror the Tauri command set consumed by
// src/lib/api/skills.ts / src/hooks/useSkills.ts.
pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_skills))
        .route("/installed", get(get_installed_skills))
        .route("/backups", get(get_backups))
        .route("/backups/:backup_id", axum::routing::delete(delete_backup))
        .route("/backups/:backup_id/restore", post(restore_backup))
        .route("/discover", get(discover_available))
        .route("/unmanaged", get(scan_unmanaged))
        .route("/updates", get(check_updates))
        .route("/search", get(search_skills_sh))
        .route("/migrate-storage", post(migrate_storage))
        .route("/repos", get(get_skill_repos))
        .route("/repos", post(add_skill_repo))
        .route(
            "/repos/:owner/:name",
            axum::routing::delete(remove_skill_repo),
        )
        .route("/import", post(import_from_apps))
        .route("/:id/install", post(install_unified))
        .route("/:id/uninstall", axum::routing::delete(uninstall_unified))
        .route("/:id/toggle", post(toggle_app))
        .route("/:id/update", put(update_skill))
}

fn parse_app(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

async fn get_installed_skills(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<InstalledSkill>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::get_all_installed(&desktop.db) {
        Ok(skills) => Json(ApiResponse::success(skills)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn list_skills(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<Skill>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let repos = match desktop.db.get_skill_repos() {
        Ok(r) => r,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };
    let service = SkillService::new();
    match service.list_skills(repos, &desktop.db).await {
        Ok(skills) => Json(ApiResponse::success(skills)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_backups(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<SkillBackupEntry>>> {
    match SkillService::list_backups() {
        Ok(backups) => Json(ApiResponse::success(backups)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn delete_backup(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(backup_id): Path<String>,
) -> Json<ApiResponse<bool>> {
    match SkillService::delete_backup(&backup_id) {
        Ok(()) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct RestoreBackupRequest {
    #[serde(rename = "currentApp")]
    current_app: String,
}

async fn restore_backup(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(backup_id): Path<String>,
    Json(body): Json<RestoreBackupRequest>,
) -> Json<ApiResponse<InstalledSkill>> {
    let app = match parse_app(&body.current_app) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::restore_from_backup(&desktop.db, &backup_id, &app) {
        Ok(skill) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.restored",
                serde_json::json!({ "id": skill.id }),
            );
            Json(ApiResponse::success(skill))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn discover_available(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<DiscoverableSkill>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let repos = match desktop.db.get_skill_repos() {
        Ok(r) => r,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };
    let service = SkillService::new();
    match service.discover_available(repos).await {
        Ok(skills) => Json(ApiResponse::success(skills)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn scan_unmanaged(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<UnmanagedSkill>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::scan_unmanaged(&desktop.db) {
        Ok(skills) => Json(ApiResponse::success(skills)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn check_updates(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<SkillUpdateInfo>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let service = SkillService::new();
    match service.check_updates(&desktop.db).await {
        Ok(updates) => Json(ApiResponse::success(updates)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_limit() -> usize {
    20
}

async fn search_skills_sh(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<SearchQuery>,
) -> Json<ApiResponse<SkillsShSearchResult>> {
    match SkillService::search_skills_sh(&params.query, params.limit, params.offset).await {
        Ok(result) => Json(ApiResponse::success(result)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct MigrateStorageRequest {
    target: SkillStorageLocation,
}

async fn migrate_storage(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<MigrateStorageRequest>,
) -> Json<ApiResponse<MigrationResult>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::migrate_storage(&desktop.db, body.target) {
        Ok(result) => Json(ApiResponse::success(result)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_skill_repos(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<SkillRepo>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.get_skill_repos() {
        Ok(repos) => Json(ApiResponse::success(repos)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn add_skill_repo(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(repo): Json<SkillRepo>,
) -> Json<ApiResponse<bool>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.save_skill_repo(&repo) {
        Ok(()) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.repo_added",
                serde_json::json!({}),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn remove_skill_repo(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path((owner, name)): Path<(String, String)>,
) -> Json<ApiResponse<bool>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.delete_skill_repo(&owner, &name) {
        Ok(()) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.repo_removed",
                serde_json::json!({ "owner": owner, "name": name }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct ImportRequest {
    imports: Vec<ImportSkillSelection>,
}

async fn import_from_apps(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<ImportRequest>,
) -> Json<ApiResponse<Vec<InstalledSkill>>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::import_from_apps(&desktop.db, body.imports) {
        Ok(skills) => {
            if !skills.is_empty() {
                crate::web::handlers::ws::broadcast_event(
                    &ws_state,
                    "skill.imported",
                    serde_json::json!({ "count": skills.len() }),
                );
            }
            Json(ApiResponse::success(skills))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct InstallRequest {
    skill: DiscoverableSkill,
    #[serde(rename = "currentApp")]
    current_app: String,
}

async fn install_unified(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(_id): Path<String>,
    Json(body): Json<InstallRequest>,
) -> Json<ApiResponse<InstalledSkill>> {
    let app = match parse_app(&body.current_app) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let service = SkillService::new();
    match service.install(&desktop.db, &body.skill, &app).await {
        Ok(skill) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.installed",
                serde_json::json!({ "id": skill.id }),
            );
            Json(ApiResponse::success(skill))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn uninstall_unified(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<SkillUninstallResult>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::uninstall(&desktop.db, &id) {
        Ok(result) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.uninstalled",
                serde_json::json!({ "id": id }),
            );
            Json(ApiResponse::success(result))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct ToggleRequest {
    app: String,
    enabled: bool,
}

async fn toggle_app(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Json(body): Json<ToggleRequest>,
) -> Json<ApiResponse<bool>> {
    let app = match parse_app(&body.app) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match SkillService::toggle_app(&desktop.db, &id, &app, body.enabled) {
        Ok(()) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.toggled",
                serde_json::json!({ "id": id, "app": body.app, "enabled": body.enabled }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn update_skill(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<InstalledSkill>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let service = SkillService::new();
    match service.update_skill(&desktop.db, &id).await {
        Ok(skill) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "skill.updated",
                serde_json::json!({ "id": skill.id }),
            );
            Json(ApiResponse::success(skill))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
