//! 流式健康检查命令

use crate::app_config::AppType;
use crate::bridges::stream_check as stream_check_bridge;
use crate::error::AppError;
use crate::services::stream_check::{StreamCheckConfig, StreamCheckResult};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn stream_check_provider(
    _state: State<'_, AppState>,
    app_type: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    stream_check_bridge::check_provider(app_type, &provider_id).await
}

#[tauri::command]
pub async fn stream_check_all_providers(
    _state: State<'_, AppState>,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    stream_check_bridge::check_all_providers(app_type, proxy_targets_only).await
}

#[tauri::command]
pub fn get_stream_check_config(
    _state: State<'_, AppState>,
) -> Result<StreamCheckConfig, AppError> {
    stream_check_bridge::get_config()
}

#[tauri::command]
pub fn save_stream_check_config(
    _state: State<'_, AppState>,
    config: StreamCheckConfig,
) -> Result<(), AppError> {
    stream_check_bridge::save_config(config)
}
