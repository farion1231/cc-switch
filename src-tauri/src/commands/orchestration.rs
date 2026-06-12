use std::sync::Arc;
use tauri::State;

use crate::orchestration::loader::StrategyLoader;
use crate::orchestration::OrchestrationEngine;

pub struct OrchestrationState(pub Arc<OrchestrationEngine>);

#[tauri::command]
pub async fn orchestration_status(
    state: State<'_, OrchestrationState>,
) -> Result<serde_json::Value, String> {
    let enabled = state.0.is_enabled().await;
    Ok(serde_json::json!({
        "enabled": enabled,
    }))
}

#[tauri::command]
pub async fn orchestration_reload(state: State<'_, OrchestrationState>) -> Result<(), String> {
    state.0.reload_config().await
}

#[tauri::command]
pub async fn orchestration_toggle(
    state: State<'_, OrchestrationState>,
    enable: bool,
) -> Result<bool, String> {
    let engine = &state.0;
    engine.persist_enabled(enable).await?;
    Ok(enable)
}

/// Return the full strategies configuration as a JSON value for the frontend editor.
#[tauri::command]
pub async fn get_strategies_config(
    state: State<'_, OrchestrationState>,
) -> Result<serde_json::Value, String> {
    let config = state.0.get_config().await;
    serde_json::to_value(&config).map_err(|e| format!("Failed to serialize config: {}", e))
}

/// Save the strategies configuration back to the YAML file.
/// Accepts the full OrchestrationConfig JSON from the frontend editor.
#[tauri::command]
pub async fn save_strategies_config(
    state: State<'_, OrchestrationState>,
    config_json: serde_json::Value,
) -> Result<(), String> {
    let config: crate::orchestration::config::OrchestrationConfig =
        serde_json::from_value(config_json)
            .map_err(|e| format!("Invalid config JSON: {}", e))?;
    // Validate before saving
    config.validate()?;
    let yaml = serde_yaml::to_string(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    let path = StrategyLoader::default_strategies_path();
    std::fs::write(&path, yaml)
        .map_err(|e| format!("Failed to write config to {:?}: {}", path, e))?;
    // Reload the engine with the new config
    state.0.reload_config().await?;
    log::info!("[Orchestration] Saved and reloaded strategies config from {:?}", path);
    Ok(())
}

/// Return the filesystem path to the strategies YAML file.
#[tauri::command]
pub fn get_strategies_config_path() -> String {
    StrategyLoader::default_strategies_path()
        .to_string_lossy()
        .to_string()
}
