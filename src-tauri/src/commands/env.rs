use crate::bridges::env as env_bridge;
use crate::services::env_checker::EnvConflict;
use crate::services::env_manager::BackupInfo;

/// Check environment variable conflicts for a specific app
#[tauri::command]
pub fn check_env_conflicts(app: String) -> Result<Vec<EnvConflict>, String> {
    env_bridge::check_env_conflicts(&app).map_err(|e| e.to_string())
}

/// Delete environment variables with backup
#[tauri::command]
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    env_bridge::delete_env_vars(conflicts).map_err(|e| e.to_string())
}

/// Restore environment variables from backup file
#[tauri::command]
pub fn restore_env_backup(backup_path: String) -> Result<(), String> {
    env_bridge::restore_env_backup(backup_path).map_err(|e| e.to_string())
}
