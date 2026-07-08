use tauri::State;

use crate::store::AppState;

#[tauri::command]
pub fn import_zcode_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_zcode_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_zcode_live_provider_ids() -> Result<Vec<String>, String> {
    crate::zcode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}
