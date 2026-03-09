#![allow(non_snake_case)]

use crate::bridges::settings as settings_bridge;
use tauri::AppHandle;

#[tauri::command]
pub async fn get_settings() -> Result<crate::settings::AppSettings, String> {
    settings_bridge::get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_settings(settings: crate::settings::AppSettings) -> Result<bool, String> {
    let result = settings_bridge::save_settings(settings).map_err(|e| e.to_string())?;
    for warning in result.warnings {
        log::warn!("{warning}");
    }
    Ok(true)
}

#[tauri::command]
pub async fn restart_app(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        app.restart();
    });
    Ok(true)
}

#[tauri::command]
pub async fn get_app_config_dir_override(app: AppHandle) -> Result<Option<String>, String> {
    Ok(crate::app_store::refresh_app_config_dir_override(&app)
        .map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn set_app_config_dir_override(
    app: AppHandle,
    path: Option<String>,
) -> Result<bool, String> {
    crate::app_store::set_app_config_dir_to_store(&app, path.as_deref())?;
    Ok(true)
}

#[tauri::command]
pub async fn set_auto_launch(enabled: bool) -> Result<bool, String> {
    settings_bridge::set_auto_launch(enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_auto_launch_status() -> Result<bool, String> {
    settings_bridge::get_auto_launch_status().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_rectifier_config(
    _state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::RectifierConfig, String> {
    settings_bridge::get_rectifier_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_rectifier_config(
    _state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::RectifierConfig,
) -> Result<bool, String> {
    settings_bridge::set_rectifier_config(config)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_log_config(
    _state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::LogConfig, String> {
    settings_bridge::get_log_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_log_config(
    _state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::LogConfig,
) -> Result<bool, String> {
    settings_bridge::set_log_config(config)
        .map(|_| true)
        .map_err(|e| e.to_string())
}
