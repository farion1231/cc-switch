#![allow(non_snake_case)]

use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::services::webdav::{WebDavBackupRequest, WebDavBackupService};
use crate::store::AppState;

/// 导出数据库为 SQL 备份
#[tauri::command]
pub async fn export_config_to_file(
    #[allow(non_snake_case)] filePath: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let target_path = PathBuf::from(&filePath);
        db.export_sql(&target_path)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "SQL exported successfully",
            "filePath": filePath
        }))
    })
    .await
    .map_err(|e| format!("导出配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// 从 SQL 备份导入数据库
#[tauri::command]
pub async fn import_config_from_file(
    #[allow(non_snake_case)] filePath: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_state = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path_buf = PathBuf::from(&filePath);
        let backup_id = db.import_sql(&path_buf)?;

        // 导入后同步当前供应商到各自的 live 配置
        let app_state = AppState::new(db_for_state);
        if let Err(err) = ProviderService::sync_current_to_live(&app_state) {
            log::warn!("导入后同步 live 配置失败: {err}");
        }

        // 重新加载设置到内存缓存，确保导入的设置生效
        if let Err(err) = crate::settings::reload_settings() {
            log::warn!("导入后重载设置失败: {err}");
        }

        Ok::<_, AppError>(json!({
            "success": true,
            "message": "SQL imported successfully",
            "backupId": backup_id
        }))
    })
    .await
    .map_err(|e| format!("导入配置失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

#[tauri::command]
pub async fn sync_current_providers_live(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app_state = AppState::new(db);
        ProviderService::sync_current_to_live(&app_state)?;
        Ok::<_, AppError>(json!({
            "success": true,
            "message": "Live configuration synchronized"
        }))
    })
    .await
    .map_err(|e| format!("同步当前供应商失败: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// 测试 WebDAV 连接
#[tauri::command]
pub async fn webdav_test_connection(config: WebDavBackupRequest) -> Result<Value, String> {
    WebDavBackupService::test_connection(&config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "success": true,
        "message": "WebDAV connection ok"
    }))
}

/// 立即执行 WebDAV 备份
#[tauri::command]
pub async fn webdav_backup_now(
    config: WebDavBackupRequest,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let sql_content = tauri::async_runtime::spawn_blocking(move || db.export_sql_string())
        .await
        .map_err(|e| format!("生成 SQL 备份失败: {e}"))?
        .map_err(|e: AppError| e.to_string())?;

    let result = WebDavBackupService::upload_backup(&config, sql_content)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "success": true,
        "message": "WebDAV backup uploaded",
        "remoteUrl": result.remote_url,
        "fileName": result.file_name,
        "sizeBytes": result.size_bytes
    }))
}

/// 从 WebDAV 恢复最新备份
#[tauri::command]
pub async fn webdav_restore_latest(
    config: WebDavBackupRequest,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    // 1. 下载最新备份
    let result = WebDavBackupService::download_latest(&config)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 导入 SQL 到数据库
    let db = state.db.clone();
    let db_for_state = db.clone();
    let sql_content = result.content;
    let file_name = result.file_name.clone();

    tauri::async_runtime::spawn_blocking(move || {
        db.import_sql_string(&sql_content)?;

        // 导入后同步当前供应商到各自的 live 配置
        let app_state = AppState::new(db_for_state);
        if let Err(err) = ProviderService::sync_current_to_live(&app_state) {
            log::warn!("导入后同步 live 配置失败: {err}");
        }

        // 重新加载设置到内存缓存
        if let Err(err) = crate::settings::reload_settings() {
            log::warn!("导入后重载设置失败: {err}");
        }

        Ok::<_, AppError>(())
    })
    .await
    .map_err(|e| format!("恢复备份失败: {e}"))?
    .map_err(|e: AppError| e.to_string())?;

    Ok(json!({
        "success": true,
        "message": "WebDAV restore completed",
        "fileName": file_name
    }))
}

/// 保存文件对话框
#[tauri::command]
pub async fn save_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    #[allow(non_snake_case)] defaultName: String,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .set_file_name(&defaultName)
        .blocking_save_file();

    Ok(result.map(|p| p.to_string()))
}

/// 打开文件对话框
#[tauri::command]
pub async fn open_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("SQL", &["sql"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}

/// 打开 ZIP 文件选择对话框
#[tauri::command]
pub async fn open_zip_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    let dialog = app.dialog();
    let result = dialog
        .file()
        .add_filter("ZIP", &["zip"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}
