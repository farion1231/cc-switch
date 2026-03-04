use crate::services::{
    legacy_startup_migration, GuardianStatus, LegacyStartupMigrationResult,
    LegacyStartupRollbackResult,
};
use crate::store::AppState;

#[tauri::command]
pub async fn get_guardian_status(
    state: tauri::State<'_, AppState>,
) -> Result<GuardianStatus, String> {
    Ok(state.guardian_service.get_status().await)
}

#[tauri::command]
pub async fn set_guardian_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<GuardianStatus, String> {
    state.guardian_service.set_enabled(enabled).await
}

#[tauri::command]
pub async fn run_guardian_once(
    state: tauri::State<'_, AppState>,
) -> Result<GuardianStatus, String> {
    state.guardian_service.run_once("manual").await
}

#[tauri::command]
pub async fn run_guardian_diagnostic(
    state: tauri::State<'_, AppState>,
) -> Result<GuardianStatus, String> {
    state.guardian_service.run_once("diagnostic").await
}

#[tauri::command]
pub async fn get_guardian_migration_status(
) -> Result<legacy_startup_migration::GuardianMigrationStatus, String> {
    legacy_startup_migration::get_guardian_migration_status().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn migrate_legacy_startup_items() -> Result<LegacyStartupMigrationResult, String> {
    legacy_startup_migration::migrate_legacy_startup_items(false).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rollback_legacy_migration() -> Result<LegacyStartupRollbackResult, String> {
    legacy_startup_migration::rollback_legacy_migration().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rollback_legacy_migration_with_backup_id(
    backup_id: Option<String>,
) -> Result<LegacyStartupRollbackResult, String> {
    legacy_startup_migration::rollback_legacy_migration_with_backup_id(backup_id)
        .map_err(|e| e.to_string())
}
