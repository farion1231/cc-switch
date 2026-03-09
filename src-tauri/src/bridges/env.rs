use crate::bridges::support::convert;
use crate::error::AppError;
use crate::services::env_checker::EnvConflict;
use crate::services::env_manager::BackupInfo;

pub fn legacy_check_env_conflicts(app: &str) -> Result<Vec<EnvConflict>, AppError> {
    crate::services::env_checker::check_env_conflicts(app)
        .map_err(AppError::Message)
}

pub fn check_env_conflicts(app: &str) -> Result<Vec<EnvConflict>, AppError> {
    cc_switch_core::services::env_checker::check_env_conflicts(app)
        .map_err(AppError::Message)
        .and_then(convert)
}

pub fn legacy_delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, AppError> {
    crate::services::env_manager::delete_env_vars(conflicts).map_err(AppError::Message)
}

pub fn delete_env_vars(conflicts: Vec<EnvConflict>) -> Result<BackupInfo, AppError> {
    let core_conflicts = convert(conflicts)?;
    cc_switch_core::services::env_manager::delete_env_vars(core_conflicts)
        .map_err(AppError::Message)
        .and_then(convert)
}

pub fn legacy_restore_env_backup(backup_path: String) -> Result<(), AppError> {
    crate::services::env_manager::restore_from_backup(backup_path).map_err(AppError::Message)
}

pub fn restore_env_backup(backup_path: String) -> Result<(), AppError> {
    cc_switch_core::services::env_manager::restore_from_backup(backup_path)
        .map_err(AppError::Message)
}
