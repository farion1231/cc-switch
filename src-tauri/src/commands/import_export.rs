#![allow(non_snake_case)]

use serde_json::Value;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::bridges::import_export as import_export_bridge;
use crate::database::backup::BackupEntry;
use crate::store::AppState;

// ─── File import/export ──────────────────────────────────────

/// 导出数据库为 SQL 备份
#[tauri::command]
pub async fn export_config_to_file(
    #[allow(non_snake_case)] filePath: String,
    _state: State<'_, AppState>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        import_export_bridge::export_config_to_file(&filePath)
    })
    .await
    .map_err(|e| format!("导出配置失败: {e}"))?
    .map_err(|e| e.to_string())
}

/// 从 SQL 备份导入数据库
#[tauri::command]
pub async fn import_config_from_file(
    #[allow(non_snake_case)] filePath: String,
    _state: State<'_, AppState>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        import_export_bridge::import_config_from_file(&filePath)
    })
    .await
    .map_err(|e| format!("导入配置失败: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sync_current_providers_live(state: State<'_, AppState>) -> Result<Value, String> {
    let _ = state;
    tauri::async_runtime::spawn_blocking(move || {
        import_export_bridge::sync_current_providers_live()
    })
    .await
    .map_err(|e| format!("同步当前供应商失败: {e}"))?
    .map_err(|e| e.to_string())
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
pub async fn create_db_backup(_state: State<'_, AppState>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(import_export_bridge::create_db_backup)
    .await
    .map_err(|e| format!("Backup failed: {e}"))?
    .map_err(|e| e.to_string())
}

/// List all database backup files
#[tauri::command]
pub fn list_db_backups() -> Result<Vec<BackupEntry>, String> {
    import_export_bridge::list_db_backups().map_err(|e| e.to_string())
}

/// Restore database from a backup file
#[tauri::command]
pub async fn restore_db_backup(
    _state: State<'_, AppState>,
    filename: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || import_export_bridge::restore_db_backup(&filename))
        .await
        .map_err(|e| format!("Restore failed: {e}"))?
        .map_err(|e| e.to_string())
}

/// Rename a database backup file
#[tauri::command]
pub fn rename_db_backup(
    #[allow(non_snake_case)] oldFilename: String,
    #[allow(non_snake_case)] newName: String,
) -> Result<String, String> {
    import_export_bridge::rename_db_backup(&oldFilename, &newName).map_err(|e| e.to_string())
}

/// Delete a database backup file
#[tauri::command]
pub fn delete_db_backup(filename: String) -> Result<(), String> {
    import_export_bridge::delete_db_backup(&filename).map_err(|e| e.to_string())
}
