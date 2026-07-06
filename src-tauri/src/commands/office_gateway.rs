use crate::office_gateway::{
    OfficeGatewayConfig, OfficeGatewayLogSnapshot, OfficeGatewayService, OfficeGatewayStatus,
    OfficeGatewayUpstreamTestResult,
};
use std::sync::Arc;
use tauri_plugin_opener::OpenerExt;

pub struct OfficeGatewayState(pub Arc<OfficeGatewayService>);

#[tauri::command]
pub async fn get_office_gateway_config(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayConfig, String> {
    Ok(state.0.get_config().await)
}

#[tauri::command]
pub async fn save_office_gateway_config(
    state: tauri::State<'_, OfficeGatewayState>,
    config: OfficeGatewayConfig,
) -> Result<OfficeGatewayConfig, String> {
    state.0.save_config(config).await?;
    Ok(state.0.get_config().await)
}

#[tauri::command]
pub async fn start_office_gateway(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayStatus, String> {
    state.0.start().await
}

#[tauri::command]
pub async fn stop_office_gateway(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<(), String> {
    state.0.stop().await
}

#[tauri::command]
pub async fn restart_office_gateway(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayStatus, String> {
    state.0.restart().await
}

#[tauri::command]
pub async fn get_office_gateway_status(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayStatus, String> {
    Ok(state.0.status().await)
}

#[tauri::command]
pub async fn get_office_gateway_logs(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayLogSnapshot, String> {
    Ok(state.0.logs().await)
}

#[tauri::command]
pub async fn test_office_gateway_upstream(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<OfficeGatewayUpstreamTestResult, String> {
    state.0.test_upstream().await
}

#[tauri::command]
pub async fn clear_office_gateway_logs(
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<(), String> {
    state.0.clear_logs().await
}

#[tauri::command]
pub async fn open_office_gateway_log_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, OfficeGatewayState>,
) -> Result<(), String> {
    let path = state.0.log_file_path();
    if !std::path::Path::new(&path).exists() {
        state.0.clear_logs().await?;
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Failed to open Office Gateway log file: {e}"))
}
