//! Web 控制台专用：SQL 导出/导入端点（绕过原生文件对话框）。
//!
//! - `GET  /control/v1/db/export-sql` → `application/sql` attachment（浏览器自动下载）
//! - `POST /control/v1/db/import-sql`  → 接收 raw SQL body，执行导入 + post-sync

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::commands::import_export::run_with_database_restore_lock;
use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::rest::error::RestError;
use crate::store::AppState;

pub fn routes() -> Router<crate::rest::service::RestState> {
    Router::new()
        .route("/control/v1/db/export-sql", get(export_sql))
        .route("/control/v1/db/import-sql", post(import_sql))
}

/// GET /control/v1/db/export-sql
///
/// 返回完整 SQL 备份作为可下载 attachment。
async fn export_sql(State(state): State<AppState>) -> Result<Response, RestError> {
    let sql = state
        .db
        .export_sql_string()
        .map_err(|e| RestError::App(e.to_string()))?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/sql".parse().unwrap());
    let now = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("cc-switch-export-{now}.sql");
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{filename}\"")
            .parse()
            .unwrap(),
    );

    Ok((headers, sql).into_response())
}

/// POST /control/v1/db/import-sql
///
/// Body: raw SQL 文本（`text/plain` 或 `application/sql`）。
/// 返回与 `import_config_from_file` 一致的 JSON payload。
async fn import_sql(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<impl IntoResponse, RestError> {
    let sql = String::from_utf8(body.to_vec())
        .map_err(|e| RestError::Bad(format!("invalid UTF-8 body: {e}")))?;

    let app_state = state.clone();
    let db = app_state.db.clone();

    let backup_id = run_with_database_restore_lock(move || {
        tokio::task::spawn_blocking(move || {
            let _guard = crate::services::skill::skill_state_write_guard();
            db.import_sql_string(&sql)
        })
    })
    .await
    .map_err(|e| RestError::App(format!("导入任务失败: {e}")))??;

    let warning = post_sync_warning_from_result(Ok(run_post_import_sync(&app_state)));
    Ok(Json(success_payload_with_warning(backup_id, warning)))
}
