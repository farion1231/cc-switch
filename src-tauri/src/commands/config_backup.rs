#![allow(non_snake_case)]

use tauri::State;

use crate::error::AppError;
use crate::services::config_backup::{
    self, ConfigBackupEntry, ConfigBackupPreview, ConfigBackupRestoreResult,
};
use crate::store::AppState;

/// List local config backups for a specific app, or for all apps when `app` is None.
#[tauri::command]
pub async fn list_app_config_backups(
    app: Option<String>,
) -> Result<Vec<ConfigBackupEntry>, String> {
    let app_type = match app {
        Some(raw) if !raw.trim().is_empty() && raw.trim() != "all" => {
            Some(config_backup::parse_app_type(&raw).map_err(|e| e.to_string())?)
        }
        _ => None,
    };

    tauri::async_runtime::spawn_blocking(move || config_backup::list_backups(app_type.as_ref()))
        .await
        .map_err(|e| format!("读取应用配置备份列表失败: {e}"))?
        .map_err(|e: AppError| e.to_string())
}

/// Preview the backed up config files inside a snapshot.
#[tauri::command]
pub async fn preview_app_config_backup(
    app: String,
    #[allow(non_snake_case)] backupId: String,
) -> Result<ConfigBackupPreview, String> {
    let app_type = config_backup::parse_app_type(&app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        config_backup::preview_backup(&app_type, &backupId)
    })
    .await
    .map_err(|e| format!("预览应用配置备份失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// Roll back an app's local config files to the specified backup snapshot.
#[tauri::command]
pub async fn restore_app_config_backup(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] backupId: String,
) -> Result<ConfigBackupRestoreResult, String> {
    let app_type = config_backup::parse_app_type(&app).map_err(|e| e.to_string())?;
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        config_backup::restore_backup(Some(&app_state), &app_type, &backupId)
    })
    .await
    .map_err(|e| format!("回退应用配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// Delete a specific app config backup snapshot.
#[tauri::command]
pub async fn delete_app_config_backup(
    app: String,
    #[allow(non_snake_case)] backupId: String,
) -> Result<(), String> {
    let app_type = config_backup::parse_app_type(&app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || config_backup::delete_backup(&app_type, &backupId))
        .await
        .map_err(|e| format!("删除应用配置备份失败: {e}"))?
        .map_err(|e: AppError| e.to_string())
}
