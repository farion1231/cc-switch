//! 镜像 rest/commands.rs —— REST 控制面配置（只读）。
//!
//! `get_rest_config` 命令读运行期 SharedRestConfig；这里从 settings.json 的
//! `rest_api` 段读取（同源数据），仅供 web 设置面板展示。

use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;

use crate::rest::error::RestError;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route("/control/v1/rest-config", get(get_rest_config_handler))
}

/// `GET /control/v1/rest-config` → RestConfig（从 settings.json，只读展示）。
async fn get_rest_config_handler() -> Result<impl IntoResponse, RestError> {
    let cfg = tokio::task::spawn_blocking(|| crate::settings::get_settings().rest_api)
        .await
        .map_err(|e| RestError::App(format!("读取 REST 配置失败: {e}")))?;
    Ok(Json(cfg))
}
