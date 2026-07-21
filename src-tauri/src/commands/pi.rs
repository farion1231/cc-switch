use tauri::State;

use crate::pi_config;
use crate::store::AppState;

// ============================================================================
// Pi Config Commands
// ============================================================================

/// Get Pi config directory.
#[tauri::command]
pub fn get_pi_dir() -> Result<String, String> {
    Ok(pi_config::get_pi_dir().display().to_string())
}

/// Get Pi config file path.
#[tauri::command]
pub fn get_pi_config_path() -> Result<String, String> {
    Ok(pi_config::get_pi_config_path().display().to_string())
}

/// Get all providers from Pi config.
#[tauri::command]
pub fn get_pi_providers(
) -> Result<indexmap::IndexMap<String, pi_config::PiProviderConfig>, String> {
    pi_config::get_providers().map_err(|e| e.to_string())
}

/// Get a single Pi provider by name.
#[tauri::command]
pub fn get_pi_provider(
    #[allow(non_snake_case)] providerName: String,
) -> Result<Option<pi_config::PiProviderConfig>, String> {
    pi_config::get_provider(&providerName).map_err(|e| e.to_string())
}

/// Upsert a Pi provider.
#[tauri::command]
pub fn set_pi_provider(
    #[allow(non_snake_case)] providerName: String,
    #[allow(non_snake_case)] providerConfig: pi_config::PiProviderConfig,
) -> Result<pi_config::PiWriteOutcome, String> {
    pi_config::set_provider(&providerName, providerConfig).map_err(|e| e.to_string())
}

/// Remove a Pi provider.
#[tauri::command]
pub fn remove_pi_provider(
    #[allow(non_snake_case)] providerName: String,
) -> Result<pi_config::PiWriteOutcome, String> {
    pi_config::remove_provider(&providerName).map_err(|e| e.to_string())
}

/// Get the active Pi provider name.
#[tauri::command]
pub fn get_pi_active_provider() -> Result<Option<String>, String> {
    pi_config::get_active_provider().map_err(|e| e.to_string())
}

/// Set the active Pi provider name.
#[tauri::command]
pub fn set_pi_active_provider(
    #[allow(non_snake_case)] providerName: String,
) -> Result<pi_config::PiWriteOutcome, String> {
    pi_config::set_active_provider(&providerName).map_err(|e| e.to_string())
}

/// Scan Pi config for known configuration hazards.
#[tauri::command]
pub fn scan_pi_config_health() -> Result<Vec<pi_config::PiHealthWarning>, String> {
    pi_config::scan_pi_config_health().map_err(|e| e.to_string())
}

/// Import providers from Pi live config to database.
#[tauri::command]
pub fn import_pi_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_pi_providers_from_live(state.inner()).map_err(|e| e.to_string())
}

/// Get provider IDs in the Pi live config.
#[tauri::command]
pub fn get_pi_live_provider_ids() -> Result<Vec<String>, String> {
    pi_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single Pi provider fragment from live config.
#[tauri::command]
pub fn get_pi_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    match pi_config::get_provider(&providerId).map_err(|e| e.to_string())? {
        Some(provider) => serde_json::to_value(provider)
            .map(Some)
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}