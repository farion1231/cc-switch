//! 镜像 commands/skill.rs —— skills 列表（只读）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;

use crate::rest::error::RestError;
use crate::services::SkillService;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route("/control/v1/installed-skills", get(get_installed_skills))
}

/// `GET /control/v1/installed-skills` → Vec<InstalledSkill>。
async fn get_installed_skills(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, RestError> {
    let skills = tokio::task::spawn_blocking(move || SkillService::get_all_installed(&state.db))
        .await
        .map_err(|e| RestError::App(format!("读取 skills 失败: {e}")))?
        .map_err(|e| RestError::App(e.to_string()))?;
    Ok(Json(skills))
}
