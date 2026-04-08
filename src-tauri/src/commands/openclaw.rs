use std::collections::HashMap;
use tauri::State;

use crate::bridges::openclaw as openclaw_bridge;
use crate::openclaw_config;
use crate::store::AppState;

#[tauri::command]
pub fn import_openclaw_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    let _ = state;
    openclaw_bridge::import_openclaw_providers_from_live().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_openclaw_live_provider_ids() -> Result<Vec<String>, String> {
    openclaw_bridge::get_openclaw_live_provider_ids().map_err(|e| e.to_string())
}

/// Get a single OpenClaw provider fragment from live config.
#[tauri::command]
pub fn get_openclaw_live_provider(
    #[allow(non_snake_case)] providerId: String,
) -> Result<Option<serde_json::Value>, String> {
    openclaw_config::get_provider(&providerId).map_err(|e| e.to_string())
}

/// Scan openclaw.json for known configuration hazards.
#[tauri::command]
pub fn scan_openclaw_config_health() -> Result<Vec<openclaw_config::OpenClawHealthWarning>, String>
{
    openclaw_config::scan_openclaw_config_health().map_err(|e| e.to_string())
}

// ============================================================================
// Agents Configuration Commands
// ============================================================================

/// Get OpenClaw default model config (agents.defaults.model)
#[tauri::command]
pub fn get_openclaw_default_model() -> Result<Option<openclaw_config::OpenClawDefaultModel>, String>
{
    openclaw_bridge::get_default_model().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_openclaw_default_model(
    model: openclaw_config::OpenClawDefaultModel,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    openclaw_config::set_default_model(&model).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_openclaw_model_catalog(
) -> Result<Option<HashMap<String, openclaw_config::OpenClawModelCatalogEntry>>, String> {
    openclaw_bridge::get_model_catalog().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_openclaw_model_catalog(
    catalog: HashMap<String, openclaw_config::OpenClawModelCatalogEntry>,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    openclaw_config::set_model_catalog(&catalog).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_openclaw_agents_defaults(
) -> Result<Option<openclaw_config::OpenClawAgentsDefaults>, String> {
    openclaw_bridge::get_agents_defaults().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_openclaw_agents_defaults(
    defaults: openclaw_config::OpenClawAgentsDefaults,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    openclaw_config::set_agents_defaults(&defaults).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_openclaw_env() -> Result<openclaw_config::OpenClawEnvConfig, String> {
    openclaw_bridge::get_env().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_openclaw_env(
    env: openclaw_config::OpenClawEnvConfig,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    openclaw_config::set_env_config(&env).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_openclaw_tools() -> Result<openclaw_config::OpenClawToolsConfig, String> {
    openclaw_bridge::get_tools().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_openclaw_tools(
    tools: openclaw_config::OpenClawToolsConfig,
) -> Result<openclaw_config::OpenClawWriteOutcome, String> {
    openclaw_config::set_tools_config(&tools).map_err(|e| e.to_string())
}
