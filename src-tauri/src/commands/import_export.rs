#![allow(non_snake_case)]

use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::database::backup::BackupEntry;
use crate::database::Database;
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::services::skill::skill_state_write_guard;
use crate::services::sync_protocol::sync_mutex;
use crate::store::AppState;

async fn run_with_database_restore_lock<T, Start, Fut>(start_operation: Start) -> T
where
    Start: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _sync_guard = sync_mutex().lock().await;
    start_operation().await
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
    let app_state_for_sync = state.inner().clone();
    let db = app_state_for_sync.db.clone();
    run_with_database_restore_lock(move || {
        tauri::async_runtime::spawn_blocking(move || {
            let path_buf = PathBuf::from(&filePath);
            let backup_id = {
                // SQL restore replaces the `skills` table. Exclude local Skill
                // mutations while the database image is being swapped.
                let _skill_state_guard = skill_state_write_guard();
                db.import_sql(&path_buf)?
            };
            let warning =
                post_sync_warning_from_result(Ok(run_post_import_sync(&app_state_for_sync)));
            if let Some(msg) = warning.as_ref() {
                log::warn!("[Import] post-import sync warning: {msg}");
            }
            Ok::<_, AppError>(success_payload_with_warning(backup_id, warning))
        })
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
        .add_filter("ZIP / Skill", &["zip", "skill"])
        .blocking_pick_file();

    Ok(result.map(|p| p.to_string()))
}

// ─── Database backup management ─────────────────────────────

/// Manually create a database backup
#[tauri::command]
pub async fn create_db_backup(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || match db.backup_database_file()? {
        Some(path) => Ok(path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()),
        None => Err(AppError::Config(
            "Database file not found, backup skipped".to_string(),
        )),
    })
    .await
    .map_err(|e| format!("Backup failed: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// List all database backup files
#[tauri::command]
pub fn list_db_backups() -> Result<Vec<BackupEntry>, String> {
    Database::list_backups().map_err(|e| e.to_string())
}

/// Restore database from a backup file
#[tauri::command]
pub async fn restore_db_backup(
    state: State<'_, AppState>,
    filename: String,
) -> Result<String, String> {
    let app_state_for_sync = state.inner().clone();
    let db = app_state_for_sync.db.clone();
    run_with_database_restore_lock(move || {
        tauri::async_runtime::spawn_blocking(move || {
            let restored = {
                let _skill_state_guard = skill_state_write_guard();
                db.restore_from_backup(&filename)?
            };
            let warning =
                post_sync_warning_from_result(Ok(run_post_import_sync(&app_state_for_sync)));
            if let Some(message) = warning {
                // This legacy command returns only the restored filename, so keep
                // restore success and surface incomplete projection in the log.
                log::warn!("[Restore] post-import sync warning: {message}");
            }
            Ok::<_, AppError>(restored)
        })
    })
    .await
    .map_err(|e| format!("Restore failed: {e}"))?
    .map_err(|e: AppError| e.to_string())
}

/// Rename a database backup file
#[tauri::command]
pub fn rename_db_backup(
    #[allow(non_snake_case)] oldFilename: String,
    #[allow(non_snake_case)] newName: String,
) -> Result<String, String> {
    Database::rename_backup(&oldFilename, &newName).map_err(|e| e.to_string())
}

/// Delete a database backup file
#[tauri::command]
pub fn delete_db_backup(filename: String) -> Result<(), String> {
    Database::delete_backup(&filename).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::run_with_database_restore_lock;
    use crate::services::sync_protocol::sync_mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn manual_restore_starts_blocking_work_after_global_lock_acquisition() {
        let guard = sync_mutex().lock().await;
        let entered = Arc::new(AtomicBool::new(false));
        let entered_in_task = Arc::clone(&entered);
        let restore = run_with_database_restore_lock(move || {
            tokio::task::spawn_blocking(move || {
                entered_in_task.store(true, Ordering::SeqCst);
            })
        });
        tokio::pin!(restore);

        assert!(
            tokio::time::timeout(Duration::from_millis(40), restore.as_mut())
                .await
                .is_err(),
            "restore must wait while another sync operation holds the global lock"
        );
        assert!(!entered.load(Ordering::SeqCst));

        drop(guard);
        tokio::time::timeout(Duration::from_secs(1), restore.as_mut())
            .await
            .expect("restore should start after lock release")
            .expect("blocking restore task should complete");
        assert!(entered.load(Ordering::SeqCst));
    }
}
