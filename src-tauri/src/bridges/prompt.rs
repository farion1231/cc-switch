use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::services::PromptService as LegacyPromptService;
use crate::store::AppState;

fn map_core_err(err: cc_switch_core::AppError) -> AppError {
    AppError::Message(err.to_string())
}

fn convert<T, U>(value: T) -> Result<U, AppError>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    serde_json::from_value(value).map_err(|e| AppError::Config(e.to_string()))
}

fn to_core_app_type(app_type: AppType) -> cc_switch_core::AppType {
    match app_type {
        AppType::Claude => cc_switch_core::AppType::Claude,
        AppType::Codex => cc_switch_core::AppType::Codex,
        AppType::Gemini => cc_switch_core::AppType::Gemini,
        AppType::OpenCode => cc_switch_core::AppType::OpenCode,
        AppType::OpenClaw => cc_switch_core::AppType::OpenClaw,
    }
}

fn core_state() -> Result<cc_switch_core::AppState, AppError> {
    let state = cc_switch_core::AppState::new(
        cc_switch_core::Database::new().map_err(map_core_err)?,
    );
    state.run_startup_maintenance();
    Ok(state)
}

fn with_core_state<T>(
    f: impl FnOnce(&cc_switch_core::AppState) -> Result<T, cc_switch_core::AppError>,
) -> Result<T, AppError> {
    let state = core_state()?;
    f(&state).map_err(map_core_err)
}

pub fn legacy_get_prompts(
    state: &AppState,
    app: AppType,
) -> Result<IndexMap<String, Prompt>, AppError> {
    LegacyPromptService::get_prompts(state, app)
}

pub fn get_prompts(app: AppType) -> Result<IndexMap<String, Prompt>, AppError> {
    let prompts = with_core_state(|state| {
        cc_switch_core::PromptService::list(state, to_core_app_type(app))
    })?;
    convert(prompts)
}

pub fn legacy_upsert_prompt(
    state: &AppState,
    app: AppType,
    id: &str,
    prompt: Prompt,
) -> Result<(), AppError> {
    LegacyPromptService::upsert_prompt(state, app, id, prompt)
}

pub fn upsert_prompt(app: AppType, id: &str, prompt: Prompt) -> Result<(), AppError> {
    let prompt = convert(prompt)?;
    with_core_state(|state| {
        cc_switch_core::PromptService::upsert_prompt(state, to_core_app_type(app), id, prompt)
    })
}

pub fn legacy_delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
    LegacyPromptService::delete_prompt(state, app, id)
}

pub fn delete_prompt(app: AppType, id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::PromptService::delete_prompt(state, to_core_app_type(app), id))
}

pub fn legacy_enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
    LegacyPromptService::enable_prompt(state, app, id)
}

pub fn enable_prompt(app: AppType, id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::PromptService::enable_prompt(state, to_core_app_type(app), id))
}

pub fn legacy_import_prompt_from_file(state: &AppState, app: AppType) -> Result<String, AppError> {
    LegacyPromptService::import_from_file(state, app)
}

pub fn import_prompt_from_file(app: AppType) -> Result<String, AppError> {
    with_core_state(|state| cc_switch_core::PromptService::import_from_file(state, to_core_app_type(app)))
}

pub fn legacy_get_current_prompt_file_content(app: AppType) -> Result<Option<String>, AppError> {
    LegacyPromptService::get_current_file_content(app)
}

pub fn get_current_prompt_file_content(app: AppType) -> Result<Option<String>, AppError> {
    cc_switch_core::PromptService::get_current_file_content(to_core_app_type(app))
        .map_err(map_core_err)
}
