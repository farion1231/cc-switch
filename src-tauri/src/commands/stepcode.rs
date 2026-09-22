use tauri::State;

use crate::stepcode_config;
use crate::store::AppState;

// ============================================================================
// StepCode Provider Commands
// ============================================================================

/// Import providers from StepCode live config (`~/.stepcode/models.json`) to database.
///
/// StepCode uses additive mode — users may already have providers configured
/// in models.json before installing CC Switch.
#[tauri::command]
pub fn import_stepcode_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_stepcode_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider IDs present in the StepCode live config.
#[tauri::command]
pub fn get_stepcode_live_provider_ids() -> Result<Vec<String>, String> {
    stepcode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single StepCode provider fragment from the live config.
#[tauri::command]
pub fn get_stepcode_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    stepcode_config::get_provider(&providerId).map_err(|e| e.to_string())
}
