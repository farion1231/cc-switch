use tauri::State;

use crate::hermes_config;
use crate::store::AppState;

// ============================================================================
// Hermes Provider Commands
// ============================================================================

/// Import providers from Hermes live config to database.
///
/// Hermes uses additive mode — users may already have providers
/// configured in config.yaml.
#[tauri::command]
pub fn import_hermes_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_hermes_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider names in the Hermes live config.
#[tauri::command]
pub fn get_hermes_live_provider_ids() -> Result<Vec<String>, String> {
    hermes_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single Hermes provider fragment from live config.
#[tauri::command]
pub fn get_hermes_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    hermes_config::get_provider(&providerId).map_err(|e| e.to_string())
}

/// Scan config.yaml for known configuration hazards.
#[tauri::command]
pub fn scan_hermes_config_health() -> Result<Vec<hermes_config::HermesHealthWarning>, String> {
    hermes_config::scan_hermes_config_health().map_err(|e| e.to_string())
}

// ============================================================================
// Model Configuration Commands
// ============================================================================

/// Get Hermes model config (model section of config.yaml)
#[tauri::command]
pub fn get_hermes_model_config() -> Result<Option<hermes_config::HermesModelConfig>, String> {
    hermes_config::get_model_config().map_err(|e| e.to_string())
}

/// Set Hermes model config (model section of config.yaml)
#[tauri::command]
pub fn set_hermes_model_config(
    model: hermes_config::HermesModelConfig,
) -> Result<hermes_config::HermesWriteOutcome, String> {
    hermes_config::set_model_config(&model).map_err(|e| e.to_string())
}

// ============================================================================
// Agent Configuration Commands
// ============================================================================

/// Get Hermes agent config (agent section of config.yaml)
#[tauri::command]
pub fn get_hermes_agent_config() -> Result<Option<hermes_config::HermesAgentConfig>, String> {
    hermes_config::get_agent_config().map_err(|e| e.to_string())
}

/// Set Hermes agent config (agent section of config.yaml)
#[tauri::command]
pub fn set_hermes_agent_config(
    agent: hermes_config::HermesAgentConfig,
) -> Result<hermes_config::HermesWriteOutcome, String> {
    hermes_config::set_agent_config(&agent).map_err(|e| e.to_string())
}

// ============================================================================
// Env Configuration Commands
// ============================================================================

/// Get Hermes env config (.env file)
#[tauri::command]
pub fn get_hermes_env() -> Result<hermes_config::HermesEnvConfig, String> {
    hermes_config::read_env().map_err(|e| e.to_string())
}

/// Set Hermes env config (.env file)
#[tauri::command]
pub fn set_hermes_env(
    env: hermes_config::HermesEnvConfig,
) -> Result<hermes_config::HermesWriteOutcome, String> {
    hermes_config::write_env(&env).map_err(|e| e.to_string())
}
