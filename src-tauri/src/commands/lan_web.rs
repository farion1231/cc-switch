//! Desktop-only Tauri commands for configuring the LAN Mobile Web UI.
//!
//! These commands are intentionally never exposed on the HTTP control plane so
//! a remote browser cannot view or modify LAN access settings or rotate tokens.

use crate::lan_web::{LanWebConfigUpdate, LanWebStatus};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_lan_web_status(state: State<'_, AppState>) -> Result<LanWebStatus, String> {
    Ok(state.lan_web_service.get_status().await)
}

#[tauri::command]
pub async fn update_lan_web_config(
    state: State<'_, AppState>,
    config: LanWebConfigUpdate,
) -> Result<LanWebStatus, String> {
    state
        .lan_web_service
        .update_config(state.inner().clone(), config)
        .await
}

#[tauri::command]
pub async fn regenerate_lan_web_token(state: State<'_, AppState>) -> Result<LanWebStatus, String> {
    state.lan_web_service.regenerate_token().await
}
