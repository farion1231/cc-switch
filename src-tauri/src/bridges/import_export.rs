use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::commands::sync_support::{
    post_sync_warning_from_result, run_post_import_sync, success_payload_with_warning,
};
use crate::database::backup::BackupEntry;
use crate::database::Database;
use crate::error::AppError;
use crate::store::AppState;

use super::support::{fresh_core_state, map_core_err};

pub fn legacy_export_config_to_file(
    state: &AppState,
    file_path: &str,
) -> Result<Value, AppError> {
    state.db.export_sql(Path::new(file_path))?;
    Ok(json!({
        "success": true,
        "message": "SQL exported successfully",
        "filePath": file_path
    }))
}

pub fn export_config_to_file(file_path: &str) -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    state
        .db
        .export_sql(Path::new(file_path))
        .map_err(map_core_err)?;
    Ok(json!({
        "success": true,
        "message": "SQL exported successfully",
        "filePath": file_path
    }))
}

pub fn legacy_import_config_from_file(
    state: &AppState,
    file_path: &str,
) -> Result<Value, AppError> {
    let backup_id = state.db.import_sql(Path::new(file_path))?;
    let warning = post_sync_warning_from_result(Ok(run_post_import_sync(state.db.clone())));
    if let Some(msg) = warning.as_ref() {
        log::warn!("[Import] post-import sync warning: {msg}");
    }
    Ok(success_payload_with_warning(backup_id, warning))
}

pub fn import_config_from_file(file_path: &str) -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    let backup_id = state
        .db
        .import_sql(Path::new(file_path))
        .map_err(map_core_err)?;
    let warning = post_sync_warning_from_result(Ok(run_post_import_sync(Arc::new(
        Database::init()?,
    ))));
    if let Some(msg) = warning.as_ref() {
        log::warn!("[Import] post-import sync warning: {msg}");
    }
    Ok(success_payload_with_warning(backup_id, warning))
}

pub fn legacy_sync_current_providers_live(_state: &AppState) -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    cc_switch_core::ProviderService::sync_current_to_live(&state).map_err(map_core_err)?;
    Ok(json!({
        "success": true,
        "message": "Live configuration synchronized"
    }))
}

pub fn sync_current_providers_live() -> Result<Value, AppError> {
    let state = fresh_core_state()?;
    cc_switch_core::ProviderService::sync_current_to_live(&state).map_err(map_core_err)?;
    Ok(json!({
        "success": true,
        "message": "Live configuration synchronized"
    }))
}

pub fn create_db_backup() -> Result<String, AppError> {
    let state = fresh_core_state()?;
    match state.db.create_backup().map_err(map_core_err)? {
        Some(path) => Ok(path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()),
        None => Err(AppError::Config(
            "Database file not found, backup skipped".to_string(),
        )),
    }
}

pub fn list_db_backups() -> Result<Vec<BackupEntry>, AppError> {
    Database::list_backups()
}

pub fn restore_db_backup(filename: &str) -> Result<String, AppError> {
    let state = fresh_core_state()?;
    state.db.restore_from_backup(filename).map_err(map_core_err)
}

pub fn rename_db_backup(old_filename: &str, new_name: &str) -> Result<String, AppError> {
    cc_switch_core::Database::rename_backup(old_filename, new_name).map_err(map_core_err)
}

pub fn delete_db_backup(filename: &str) -> Result<(), AppError> {
    cc_switch_core::Database::delete_backup(filename).map_err(map_core_err)
}
