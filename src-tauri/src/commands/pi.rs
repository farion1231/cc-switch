use tauri::State;

use crate::store::AppState;

/// Import providers from Pi Agent live config to database.
#[tauri::command]
pub fn import_pi_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_pi_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider IDs in the Pi Agent live config.
#[tauri::command]
pub fn get_pi_live_provider_ids() -> Result<Vec<String>, String> {
    crate::pi_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single Pi Agent provider fragment from live config.
#[tauri::command]
pub fn get_pi_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    crate::pi_config::get_provider(&providerId).map_err(|e| e.to_string())
}

/// Get the Pi Agent default provider id from models.json.
#[tauri::command]
pub fn get_pi_default_provider() -> Result<Option<String>, String> {
    crate::pi_config::get_default_provider().map_err(|e| e.to_string())
}
