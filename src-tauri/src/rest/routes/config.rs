//! 镜像 commands/config.rs —— 应用配置目录路径（只读）。

use axum::extract::Query;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use serde::Deserialize;

use crate::rest::error::RestError;
use crate::rest::routes::common::parse_app;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route("/control/v1/config-dir", get(get_config_dir_handler))
}

#[derive(Deserialize)]
struct AppQuery {
    app: String,
}

/// `GET /control/v1/config-dir?app=claude` → 该应用的配置目录路径（只读）。
async fn get_config_dir_handler(
    Query(q): Query<AppQuery>,
) -> Result<impl IntoResponse, RestError> {
    parse_app(&q.app)?; // 校验 app 合法性（与命令侧一致）
    let dir = crate::commands::config::get_config_dir(q.app)
        .await
        .map_err(|e| RestError::App(e))?;
    Ok(Json(dir))
}
