use cc_command_api::command_api;

use crate::error::AppError;
use crate::services::omo::{OmoLocalFileData, SLIM, STANDARD};
use crate::services::OmoService;
use crate::store::AppState;

#[command_api]
pub async fn read_omo_local_file() -> Result<OmoLocalFileData, AppError> {
    OmoService::read_local_file(&STANDARD).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn get_current_omo_provider_id(state: &AppState) -> Result<String, AppError> {
    let provider = state.db.get_current_omo_provider("opencode", "omo")?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

#[command_api]
pub async fn disable_current_omo(state: &AppState) -> Result<(), AppError> {
    let providers = state.db.get_all_providers("opencode")?;
    for (id, p) in &providers {
        if p.category.as_deref() == Some("omo") {
            state.db.clear_omo_provider_current("opencode", id, "omo")?;
        }
    }
    OmoService::delete_config_file(&STANDARD).map_err(|e| AppError::Config(e.to_string()))?;
    Ok(())
}

// ── OMO Slim commands ───────────────────────────────────────

#[command_api]
pub async fn read_omo_slim_local_file() -> Result<OmoLocalFileData, AppError> {
    OmoService::read_local_file(&SLIM).map_err(|e| AppError::Config(e.to_string()))
}

#[command_api]
pub async fn get_current_omo_slim_provider_id(state: &AppState) -> Result<String, AppError> {
    let provider = state.db.get_current_omo_provider("opencode", "omo-slim")?;
    Ok(provider.map(|p| p.id).unwrap_or_default())
}

#[command_api]
pub async fn disable_current_omo_slim(state: &AppState) -> Result<(), AppError> {
    let providers = state.db.get_all_providers("opencode")?;
    for (id, p) in &providers {
        if p.category.as_deref() == Some("omo-slim") {
            state
                .db
                .clear_omo_provider_current("opencode", id, "omo-slim")?;
        }
    }
    OmoService::delete_config_file(&SLIM).map_err(|e| AppError::Config(e.to_string()))?;
    Ok(())
}
