use std::collections::HashMap;
use tauri::State;

use crate::app::AppState;

// ============================================================================
// OpenClaw Provider Commands (migrated from provider.rs)
// ============================================================================

/// Import providers from OpenClaw live config to database.
///
/// OpenClaw uses additive mode — users may already have providers
/// configured in openclaw.json.
#[tauri::command]
pub fn import_openclaw_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_openclaw_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider IDs in the OpenClaw live config.
#[tauri::command]
pub fn get_openclaw_live_provider_ids() -> Result<Vec<String>, String> {
    crate::live_config::openclaw::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

/// Get a single OpenClaw provider fragment from live config.
#[tauri::command]
pub fn get_openclaw_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    crate::live_config::openclaw::get_provider(&providerId).map_err(|e| e.to_string())
}

/// Scan openclaw.json for known configuration hazards.
#[tauri::command]
pub fn scan_openclaw_config_health(
) -> Result<Vec<crate::live_config::openclaw::OpenClawHealthWarning>, String> {
    crate::live_config::openclaw::scan_openclaw_config_health().map_err(|e| e.to_string())
}

// ============================================================================
// Agents Configuration Commands
// ============================================================================

/// Get OpenClaw default model config (agents.defaults.model)
#[tauri::command]
pub fn get_openclaw_default_model(
) -> Result<Option<crate::live_config::openclaw::OpenClawDefaultModel>, String> {
    crate::live_config::openclaw::get_default_model().map_err(|e| e.to_string())
}

/// Set OpenClaw default model config (agents.defaults.model)
#[tauri::command]
pub fn set_openclaw_default_model(
    model: crate::live_config::openclaw::OpenClawDefaultModel,
) -> Result<crate::live_config::openclaw::OpenClawWriteOutcome, String> {
    crate::live_config::openclaw::set_default_model(&model).map_err(|e| e.to_string())
}

/// Get OpenClaw model catalog/allowlist (agents.defaults.models)
#[tauri::command]
pub fn get_openclaw_model_catalog(
) -> Result<Option<HashMap<String, crate::live_config::openclaw::OpenClawModelCatalogEntry>>, String>
{
    crate::live_config::openclaw::get_model_catalog().map_err(|e| e.to_string())
}

/// Set OpenClaw model catalog/allowlist (agents.defaults.models)
#[tauri::command]
pub fn set_openclaw_model_catalog(
    catalog: HashMap<String, crate::live_config::openclaw::OpenClawModelCatalogEntry>,
) -> Result<crate::live_config::openclaw::OpenClawWriteOutcome, String> {
    crate::live_config::openclaw::set_model_catalog(&catalog).map_err(|e| e.to_string())
}

/// Get full agents.defaults config (all fields)
#[tauri::command]
pub fn get_openclaw_agents_defaults(
) -> Result<Option<crate::live_config::openclaw::OpenClawAgentsDefaults>, String> {
    crate::live_config::openclaw::get_agents_defaults().map_err(|e| e.to_string())
}

/// Set full agents.defaults config (all fields)
#[tauri::command]
pub fn set_openclaw_agents_defaults(
    defaults: crate::live_config::openclaw::OpenClawAgentsDefaults,
) -> Result<crate::live_config::openclaw::OpenClawWriteOutcome, String> {
    crate::live_config::openclaw::set_agents_defaults(&defaults).map_err(|e| e.to_string())
}

// ============================================================================
// Env Configuration Commands
// ============================================================================

/// Get OpenClaw env config (env section of openclaw.json)
#[tauri::command]
pub fn get_openclaw_env() -> Result<crate::live_config::openclaw::OpenClawEnvConfig, String> {
    crate::live_config::openclaw::get_env_config().map_err(|e| e.to_string())
}

/// Set OpenClaw env config (env section of openclaw.json)
#[tauri::command]
pub fn set_openclaw_env(
    env: crate::live_config::openclaw::OpenClawEnvConfig,
) -> Result<crate::live_config::openclaw::OpenClawWriteOutcome, String> {
    crate::live_config::openclaw::set_env_config(&env).map_err(|e| e.to_string())
}

// ============================================================================
// Tools Configuration Commands
// ============================================================================

/// Get OpenClaw tools config (tools section of openclaw.json)
#[tauri::command]
pub fn get_openclaw_tools() -> Result<crate::live_config::openclaw::OpenClawToolsConfig, String> {
    crate::live_config::openclaw::get_tools_config().map_err(|e| e.to_string())
}

/// Set OpenClaw tools config (tools section of openclaw.json)
#[tauri::command]
pub fn set_openclaw_tools(
    tools: crate::live_config::openclaw::OpenClawToolsConfig,
) -> Result<crate::live_config::openclaw::OpenClawWriteOutcome, String> {
    crate::live_config::openclaw::set_tools_config(&tools).map_err(|e| e.to_string())
}
