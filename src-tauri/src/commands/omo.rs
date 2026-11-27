use crate::app_config::AppType;
use crate::error::AppError;
use tauri::State;

use crate::services::omo::{OmoLocalFileData, SLIM, STANDARD};
use crate::services::OmoService;
use crate::store::AppState;

#[tauri::command]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, AppError> {
    OmoService::read_local_file(&STANDARD)
}

#[tauri::command]
pub async fn get_current_omo_provider_id(state: State<'_, AppState>) -> Result<String, String> {
    let provider = state
        .db
        .get_current_omo_provider(&AppType::OpenCode, "omo")?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

#[tauri::command]
pub async fn disable_current_omo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .db
        .clear_omo_category_current(&AppType::OpenCode, "omo")?;
    OmoService::delete_config_file(&STANDARD)?;
    crate::tray::refresh_tray_menu(&app);
    Ok(())
}

// ── OMO Slim commands ───────────────────────────────────────

#[tauri::command]
pub async fn read_omo_slim_local_file() -> Result<OmoLocalFileData, AppError> {
    OmoService::read_local_file(&SLIM)
}

#[tauri::command]
pub async fn get_current_omo_slim_provider_id(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let provider = state
        .db
        .get_current_omo_provider(&AppType::OpenCode, "omo-slim")?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

#[tauri::command]
pub async fn disable_current_omo_slim(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .db
        .clear_omo_category_current(&AppType::OpenCode, "omo-slim")?;
    OmoService::delete_config_file(&SLIM)?;
    crate::tray::refresh_tray_menu(&app);
    Ok(())
}
