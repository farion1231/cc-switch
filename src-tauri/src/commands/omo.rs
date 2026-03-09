use tauri::State;

use crate::bridges::omo as omo_bridge;
use crate::services::omo::OmoLocalFileData;
use crate::store::AppState;

#[tauri::command]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, String> {
    omo_bridge::read_local_file(&cc_switch_core::STANDARD).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_omo_provider_id(state: State<'_, AppState>) -> Result<String, String> {
    let _ = state;
    omo_bridge::get_current_provider_id(&cc_switch_core::STANDARD)
        .map(|provider| provider.unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn disable_current_omo(state: State<'_, AppState>) -> Result<(), String> {
    let _ = state;
    omo_bridge::disable_current(&cc_switch_core::STANDARD).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_omo_slim_local_file() -> Result<OmoLocalFileData, String> {
    omo_bridge::read_local_file(&cc_switch_core::SLIM).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_omo_slim_provider_id(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _ = state;
    omo_bridge::get_current_provider_id(&cc_switch_core::SLIM)
        .map(|provider| provider.unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn disable_current_omo_slim(state: State<'_, AppState>) -> Result<(), String> {
    let _ = state;
    omo_bridge::disable_current(&cc_switch_core::SLIM).map_err(|e| e.to_string())
}
