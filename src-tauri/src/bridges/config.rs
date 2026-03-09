use std::sync::Arc;

use crate::app_config::AppType;
use crate::database::Database as LegacyDatabase;
use crate::bridges::support::{fresh_core_state, map_core_err};
use crate::error::AppError;
use crate::services::omo::{OmoService as LegacyOmoService, SLIM as LEGACY_SLIM, STANDARD as LEGACY_STANDARD};
use crate::services::provider::ProviderService as LegacyProviderService;
use crate::store::AppState;

pub fn legacy_get_common_config_snippet(app_type: &str) -> Result<Option<String>, AppError> {
    let db = LegacyDatabase::init()?;
    db.get_config_snippet(app_type)
}

pub fn legacy_set_common_config_snippet(
    app_type: &str,
    value: Option<String>,
) -> Result<(), AppError> {
    let db = Arc::new(LegacyDatabase::init()?);
    db.set_config_snippet(app_type, value)?;
    let state = AppState::new(db.clone());

    match app_type {
        "omo" => {
            if db.get_current_omo_provider("opencode", "omo")?.is_some() {
                LegacyOmoService::write_config_to_file(&state, &LEGACY_STANDARD)?;
            }
        }
        "omo-slim" => {
            if db.get_current_omo_provider("opencode", "omo-slim")?.is_some() {
                LegacyOmoService::write_config_to_file(&state, &LEGACY_SLIM)?;
            }
        }
        _ => {}
    }

    Ok(())
}

pub fn get_common_config_snippet(app_type: &str) -> Result<Option<String>, AppError> {
    let state = fresh_core_state()?;
    state.db.get_config_snippet(app_type).map_err(map_core_err)
}

pub fn set_common_config_snippet(app_type: &str, value: Option<String>) -> Result<(), AppError> {
    let state = fresh_core_state()?;
    state
        .db
        .set_config_snippet(app_type, value)
        .map_err(map_core_err)?;

    match app_type {
        "omo" => {
            if state
                .db
                .get_current_omo_provider("opencode", "omo")
                .map_err(map_core_err)?
                .is_some()
            {
                cc_switch_core::OmoService::write_config_to_file(&state, &cc_switch_core::STANDARD)
                    .map_err(map_core_err)?;
            }
        }
        "omo-slim" => {
            if state
                .db
                .get_current_omo_provider("opencode", "omo-slim")
                .map_err(map_core_err)?
                .is_some()
            {
                cc_switch_core::OmoService::write_config_to_file(&state, &cc_switch_core::SLIM)
                    .map_err(map_core_err)?;
            }
        }
        _ => {}
    }

    Ok(())
}

pub fn legacy_extract_common_config_snippet(
    app_type: AppType,
    settings_config: Option<serde_json::Value>,
) -> Result<String, AppError> {
    match settings_config {
        Some(settings) => LegacyProviderService::extract_common_config_snippet_from_settings(
            app_type,
            &settings,
        ),
        None => {
            let state = AppState::new(Arc::new(LegacyDatabase::init()?));
            LegacyProviderService::extract_common_config_snippet(&state, app_type)
        }
    }
}

pub fn extract_common_config_snippet(
    app_type: AppType,
    settings_config: Option<serde_json::Value>,
) -> Result<String, AppError> {
    let state = fresh_core_state()?;
    match settings_config {
        Some(settings) => cc_switch_core::ProviderService::extract_common_config_snippet_from_settings(
            super::support::to_core_app_type(app_type),
            &settings,
        )
        .map_err(map_core_err),
        None => cc_switch_core::ProviderService::extract_common_config_snippet(
            &state,
            super::support::to_core_app_type(app_type),
        )
        .map_err(map_core_err),
    }
}
