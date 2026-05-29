//! 请求日志捕获相关的 Tauri 命令

use crate::proxy::request_log::ProxyRequestLogEntry;
use crate::store::AppState;

/// 获取所有捕获的请求日志
#[tauri::command]
pub async fn get_captured_request_logs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProxyRequestLogEntry>, String> {
    state.proxy_service.get_captured_request_logs().await
}

/// 获取单条请求日志详情（含完整 request body）
#[tauri::command]
pub async fn get_captured_request_log_detail(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<ProxyRequestLogEntry>, String> {
    state
        .proxy_service
        .get_captured_request_log_detail(&id)
        .await
}

/// 清空所有请求日志
#[tauri::command]
pub async fn clear_captured_request_logs(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.proxy_service.clear_captured_request_logs().await
}

/// 设置请求日志捕获开关
#[tauri::command]
pub async fn set_request_log_capture_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state
        .proxy_service
        .set_request_log_capture_enabled(enabled)
        .await
}

/// 获取请求日志捕获开关状态
#[tauri::command]
pub async fn is_request_log_capture_enabled(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state
        .proxy_service
        .is_request_log_capture_enabled()
        .await)
}

/// 获取日志最大保留条数
#[tauri::command]
pub async fn get_request_log_max_entries(
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    Ok(state.proxy_service.get_request_log_max_entries())
}

/// 设置日志最大保留条数
#[tauri::command]
pub async fn set_request_log_max_entries(
    state: tauri::State<'_, AppState>,
    max: usize,
) -> Result<(), String> {
    state.proxy_service.set_request_log_max_entries(max).await;
    Ok(())
}
