#![allow(non_snake_case)]

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::database::Database;
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::services::webdav_sync;
use crate::settings::{self, WebDavSyncSettings};
use crate::store::AppState;

// ─── Post-import sync helper (fixes review P1 duplication) ───

fn run_post_import_sync(db: Arc<Database>) {
    let app_state = AppState::new(db);
    if let Err(err) = ProviderService::sync_current_to_live(&app_state) {
        log::warn!("导入后同步 live 配置失败: {err}");
    }
    if let Err(err) = settings::reload_settings() {
        log::warn!("导入后重载设置失败: {err}");
    }
}

// ─── File import/export ──────────────────────────────────────

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
    let db_for_sync = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path_buf = PathBuf::from(&filePath);
        let backup_id = db.import_sql(&path_buf)?;
        run_post_import_sync(db_for_sync);
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

// ─── WebDAV sync commands ────────────────────────────────────

/// Shared error persistence for sync operations.
fn persist_sync_error(settings: &mut WebDavSyncSettings, error: &AppError) {
    settings.status.last_error = Some(error.to_string());
    let _ = settings::set_webdav_sync_settings(Some(settings.clone()));
}

fn webdav_not_configured_error() -> String {
    AppError::localized(
        "webdav.sync.not_configured",
        "未配置 WebDAV 同步",
        "WebDAV sync is not configured.",
    )
    .to_string()
}

/// 测试 WebDAV 连接
#[tauri::command]
pub async fn webdav_test_connection(settings: WebDavSyncSettings) -> Result<Value, String> {
    webdav_sync::check_connection(&settings)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "success": true,
        "message": "WebDAV connection ok"
    }))
}

/// 上传同步（替代原 webdav_backup_now）
#[tauri::command]
pub async fn webdav_sync_upload(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let mut settings = settings::get_webdav_sync_settings()
        .ok_or_else(webdav_not_configured_error)?;

    webdav_sync::upload(&db, &mut settings).await.map_err(|e| {
        persist_sync_error(&mut settings, &e);
        e.to_string()
    })
}

/// 下载同步（替代原 webdav_restore_latest）
#[tauri::command]
pub async fn webdav_sync_download(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_sync = db.clone();
    let mut settings = settings::get_webdav_sync_settings()
        .ok_or_else(webdav_not_configured_error)?;

    let result = webdav_sync::download(&db, &mut settings)
        .await
        .map_err(|e| {
            persist_sync_error(&mut settings, &e);
            e.to_string()
        })?;

    // Post-download: sync providers to live config (DB changed)
    tauri::async_runtime::spawn_blocking(move || {
        run_post_import_sync(db_for_sync);
    })
    .await
    .ok();

    Ok(result)
}

/// 显式保存 WebDAV 同步设置（修复 P0: 不再自动保存密码）
///
/// Only persists user-editable fields (credentials, remote root, profile).
/// Server-owned fields (status, deviceId) are preserved from existing config.
#[tauri::command]
pub async fn webdav_sync_save_settings(
    settings: WebDavSyncSettings,
) -> Result<Value, String> {
    let mut s = settings;

    // Preserve server-owned fields that the frontend does not manage
    if let Some(existing) = settings::get_webdav_sync_settings() {
        s.status = existing.status;
        if s.device_id.is_empty() {
            s.device_id = existing.device_id;
        }
        if s.password.is_empty() {
            s.password = existing.password;
        }
    }

    s.normalize();
    s.validate().map_err(|e| e.to_string())?;
    settings::set_webdav_sync_settings(Some(s)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

/// 获取远端同步信息（下载前预览）
#[tauri::command]
pub async fn webdav_sync_fetch_remote_info() -> Result<Value, String> {
    let settings = settings::get_webdav_sync_settings()
        .ok_or_else(webdav_not_configured_error)?;

    let info = webdav_sync::fetch_remote_info(&settings)
        .await
        .map_err(|e| e.to_string())?;

    Ok(info.unwrap_or(json!({ "empty": true })))
}

// ─── File dialogs ────────────────────────────────────────────

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
