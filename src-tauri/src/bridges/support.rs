use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::app_config::AppType;
use crate::error::AppError;

pub fn map_core_err(err: cc_switch_core::AppError) -> AppError {
    AppError::Message(err.to_string())
}

pub fn convert<T, U>(value: T) -> Result<U, AppError>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    serde_json::from_value(value).map_err(|e| AppError::Config(e.to_string()))
}

pub fn to_core_app_type(app_type: AppType) -> cc_switch_core::AppType {
    match app_type {
        AppType::Claude => cc_switch_core::AppType::Claude,
        AppType::Codex => cc_switch_core::AppType::Codex,
        AppType::Gemini => cc_switch_core::AppType::Gemini,
        AppType::OpenCode => cc_switch_core::AppType::OpenCode,
        AppType::OpenClaw => cc_switch_core::AppType::OpenClaw,
    }
}

pub fn fresh_core_state() -> Result<cc_switch_core::AppState, AppError> {
    let state =
        cc_switch_core::AppState::new(cc_switch_core::Database::new().map_err(map_core_err)?);
    state.run_startup_maintenance();
    Ok(state)
}

pub fn with_core_state<T>(
    f: impl FnOnce(&cc_switch_core::AppState) -> Result<T, cc_switch_core::AppError>,
) -> Result<T, AppError> {
    let state = fresh_core_state()?;
    f(&state).map_err(map_core_err)
}
