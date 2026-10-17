use cc_command_api::command_api;

use crate::error::AppError;
use crate::services::env_checker::{check_env_conflicts as check_conflicts, EnvConflict};
use crate::services::env_manager::{
    delete_env_vars as delete_vars, restore_from_backup, BackupInfo,
};

/// Check environment variable conflicts for a specific app
#[command_api]
pub fn check_env_conflicts(app: String) -> Result<Vec<EnvConflict>, AppError> {
    check_conflicts(&app).map_err(|e| AppError::Config(e))
}

/// Delete environment variables with backup
#[command_api]
pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, AppError> {
    delete_vars(conflicts).map_err(|e| AppError::Config(e))
}

/// Restore environment variables from backup file
#[command_api]
pub fn restore_env_backup(backup_path: String) -> Result<(), AppError> {
    restore_from_backup(backup_path).map_err(|e| AppError::Config(e))
}
