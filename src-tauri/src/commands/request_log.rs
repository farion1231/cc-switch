//! Tauri commands related to request log capture

use crate::proxy::request_log::ProxyRequestLogEntry;
use crate::store::AppState;

/// Get all captured request logs
#[tauri::command]
pub async fn get_captured_request_logs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProxyRequestLogEntry>, String> {
    state.proxy_service.get_captured_request_logs().await
}

/// Get details of a single request log (including full request body)
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

/// Clear all request logs
#[tauri::command]
pub async fn clear_captured_request_logs(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.proxy_service.clear_captured_request_logs().await
}

/// Set the request log capture switch
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

/// Get the request log capture switch status
#[tauri::command]
pub async fn is_request_log_capture_enabled(
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state
        .proxy_service
        .is_request_log_capture_enabled()
        .await)
}

/// Get the maximum number of log entries to retain
#[tauri::command]
pub async fn get_request_log_max_entries(
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    Ok(state.proxy_service.get_request_log_max_entries())
}

/// Set the maximum number of log entries to retain
#[tauri::command]
pub async fn set_request_log_max_entries(
    state: tauri::State<'_, AppState>,
    max: usize,
) -> Result<(), String> {
    state.proxy_service.set_request_log_max_entries(max).await;
    Ok(())
}
