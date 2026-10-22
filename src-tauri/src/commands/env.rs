use crate::services::env_checker::{check_env_conflicts as check_conflicts, EnvConflict};
use crate::services::env_manager::{
    delete_env_vars as delete_vars, restore_from_backup, BackupInfo,
};
use cc_command_api::command_api;

/// Check environment variable conflicts for a specific app
#[command_api]
pub fn check_env_conflicts(app: String) -> Result<Vec<EnvConflict>, String> {
    check_conflicts(&app)
}

/// Delete environment variables with backup
#[command_api]
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, String> {
    delete_vars(conflicts)
}

/// Restore environment variables from backup file
#[command_api]
pub fn restore_env_backup(backup_path: String) -> Result<(), String> {
    restore_from_backup(backup_path)
}
