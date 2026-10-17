//! 镜像 commands/misc.rs —— 迁移一次性状态（只读，take 后清空）。

use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;

use crate::rest::error::RestError;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/migration-result", get(get_migration_result))
        .route(
            "/control/v1/skills-migration-result",
            get(get_skills_migration_result),
        )
}

/// `GET /control/v1/migration-result` → bool（一次性：取后清空）。
async fn get_migration_result() -> Result<impl IntoResponse, RestError> {
    Ok(Json(crate::init_status::take_migration_success()))
}

/// `GET /control/v1/skills-migration-result` → Option<SkillsMigrationPayload>（一次性）。
async fn get_skills_migration_result() -> Result<impl IntoResponse, RestError> {
    Ok(Json(crate::init_status::take_skills_migration_result()))
}
