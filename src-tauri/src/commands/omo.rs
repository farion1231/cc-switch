use tauri::State;

use crate::services::omo::OmoLocalFileData;
use crate::services::OmoService;
use crate::store::AppState;

#[tauri::command]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, String> {
    OmoService::read_local_file().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_omo_provider_id(state: State<'_, AppState>) -> Result<String, String> {
    let provider = state
        .db
        .get_current_omo_provider("opencode")
        .map_err(|e| e.to_string())?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

#[tauri::command]
pub async fn disable_current_omo(state: State<'_, AppState>) -> Result<(), String> {
    let providers = state
        .db
        .get_all_providers("opencode")
        .map_err(|e| e.to_string())?;
    for (id, p) in &providers {
        if p.category.as_deref() == Some("omo") {
            state
                .db
                .clear_omo_provider_current("opencode", id)
                .map_err(|e| e.to_string())?;
        }
    }
    OmoService::delete_config_file().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_omo_provider_count(state: State<'_, AppState>) -> Result<usize, String> {
    let providers = state
        .db
        .get_all_providers("opencode")
        .map_err(|e| e.to_string())?;
    let count = providers
        .values()
        .filter(|p| p.category.as_deref() == Some("omo"))
        .count();
    Ok(count)
}
