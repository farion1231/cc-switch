//! 镜像 commands/settings.rs —— 全量 AppSettings blob + app_config_dir 覆盖。
//!
//! GET 返回 `get_settings_for_frontend()`（脱敏 webdav/s3 密码）；
//! PUT 走 `merge_settings_for_save` + `update_settings`，与命令侧同路径，
//! 保留 webdav/s3/本地迁移/rest_api 等字段的合并保护。

use axum::response::IntoResponse;
use axum::routing::{get, put};
use axum::Json;

use crate::commands::settings::merge_settings_for_save;
use crate::rest::error::RestError;
use crate::settings::{get_settings, get_settings_for_frontend, update_settings, AppSettings};

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/settings", get(get_settings_handler))
        .route("/control/v1/settings", put(save_settings_handler))
        .route(
            "/control/v1/app-config-dir-override",
            get(get_app_config_dir_override_handler),
        )
}

/// `GET /control/v1/settings` → 全量 AppSettings（脱敏）。
async fn get_settings_handler() -> Result<impl IntoResponse, RestError> {
    let settings = tokio::task::spawn_blocking(get_settings_for_frontend)
        .await
        .map_err(|e| RestError::App(format!("读取设置失败: {e}")))?;
    Ok(Json(settings))
}

/// `PUT /control/v1/settings` body=`{settings: AppSettings}` → 合并后持久化。
/// body 包裹一层 `settings`，对齐 Tauri 命令参数形状（`save_settings(settings)`）。
async fn save_settings_handler(
    Json(body): Json<SettingsBody>,
) -> Result<impl IntoResponse, RestError> {
    let incoming = body.settings;
    tokio::task::spawn_blocking(move || {
        let existing = get_settings();
        let merged = merge_settings_for_save(incoming, &existing);
        update_settings(merged)
    })
    .await
    .map_err(|e| RestError::App(format!("保存任务失败: {e}")))?
    .map_err(|e| RestError::App(e.to_string()))?;
    // AppSettings 已持久化
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
struct SettingsBody {
    settings: AppSettings,
}

/// `GET /control/v1/app-config-dir-override` → app_config_dir 覆盖路径（无则 null）。
async fn get_app_config_dir_override_handler() -> Result<impl IntoResponse, RestError> {
    let path = tokio::task::spawn_blocking(crate::app_store::get_app_config_dir_override)
        .await
        .map_err(|e| RestError::App(format!("读取覆盖失败: {e}")))?;
    Ok(Json(path.map(|p| p.to_string_lossy().to_string())))
}
