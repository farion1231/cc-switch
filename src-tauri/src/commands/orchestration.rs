use std::sync::Arc;
use tauri::State;

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
pub async fn orchestration_reload(
    state: State<'_, OrchestrationState>,
) -> Result<(), String> {
    state.0.reload_config().await
}

#[tauri::command]
pub async fn orchestration_toggle(
    state: State<'_, OrchestrationState>,
    enable: bool,
) -> Result<bool, String> {
    let engine = &state.0;
    engine.set_enabled(enable);
    Ok(enable)
}
