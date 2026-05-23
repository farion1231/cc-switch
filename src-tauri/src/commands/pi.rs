use serde_json::{Map, Value};
use tauri::State;

use crate::pi_config;
use crate::store::AppState;

// ============================================================================
// Pi Provider Commands
// ============================================================================

/// Get all CC Switch-managed Pi providers from models.json.
#[tauri::command]
pub fn get_pi_providers() -> Result<Map<String, Value>, String> {
    pi_config::get_pi_providers().map_err(|e| e.to_string())
}

/// Add or update a Pi provider in models.json.
#[tauri::command]
pub fn set_pi_provider(
    #[allow(non_snake_case)] providerId: String,
    config: Value,
) -> Result<(), String> {
    pi_config::set_pi_provider(&providerId, &config).map_err(|e| e.to_string())
}

/// Remove a Pi provider from models.json.
#[tauri::command]
pub fn remove_pi_provider(#[allow(non_snake_case)] providerId: String) -> Result<(), String> {
    pi_config::remove_pi_provider(&providerId).map_err(|e| e.to_string())
}

/// Set the currently active Pi provider (writes to settings.json).
#[tauri::command]
pub fn set_active_pi_provider(
    #[allow(non_snake_case)] providerId: String,
    #[allow(non_snake_case)] modelId: Option<String>,
) -> Result<(), String> {
    pi_config::set_active_pi_provider(&providerId, modelId.as_deref()).map_err(|e| e.to_string())
}

// ============================================================================
// Pi Settings Commands
// ============================================================================

/// Get Pi managed settings from settings.json.
#[tauri::command]
pub fn get_pi_settings() -> Result<pi_config::PiSettings, String> {
    pi_config::get_pi_settings().map_err(|e| e.to_string())
}

/// Update Pi settings in settings.json (partial update, preserves unknown fields).
#[tauri::command]
pub fn update_pi_settings(fields: Map<String, Value>) -> Result<(), String> {
    pi_config::update_pi_settings(&fields).map_err(|e| e.to_string())
}

// ============================================================================
// Pi Provider Sync (from live)
// ============================================================================

/// Import providers from Pi live config (models.json) into the CC Switch database.
#[tauri::command]
pub fn import_pi_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    let models = pi_config::read_models_json().map_err(|e| e.to_string())?;
    let providers = models
        .get("providers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let mut imported = 0usize;
    for (id, config) in &providers {
        if id.starts_with("cc-switch-") {
            continue; // Already managed by CC Switch
        }
        let mut provider =
            crate::provider::Provider::with_id(id.clone(), id.clone(), config.clone(), None);
        provider.category = Some("custom".to_string());
        provider.icon = Some("pi".to_string());
        state
            .db
            .save_provider("pi", &provider)
            .map_err(|e| e.to_string())?;
        imported += 1;
    }

    Ok(imported)
}
