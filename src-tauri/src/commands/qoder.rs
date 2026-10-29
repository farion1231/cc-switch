use crate::{
    app_config::AppType,
    qoder_config,
    qoder_models::{self, QoderModels},
};
use crate::{provider::UsageScript, store::AppState};
use serde_json::Value;
use std::str::FromStr;
use tauri::State;

fn app_type(app: &str) -> Result<AppType, String> {
    let app = AppType::from_str(app).map_err(|error| error.to_string())?;
    if !qoder_config::is_qoder(&app) {
        return Err("Select a Qoder edition".into());
    }
    Ok(app)
}

#[tauri::command]
pub fn get_qoder_models(state: State<'_, AppState>, app: String) -> Result<QoderModels, String> {
    let app = app_type(&app)?;
    qoder_models::get(&app)
        .and_then(|models| qoder_models::with_metadata(&state.db, &app, models))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_qoder_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
    provider: Value,
    expected: Option<Value>,
    make_default: bool,
) -> Result<QoderModels, String> {
    let app = app_type(&app)?;
    qoder_models::save_provider(&app, &id, provider, expected, make_default)
        .and_then(|models| qoder_models::with_metadata(&state.db, &app, models))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_qoder_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
    expected: Value,
) -> Result<QoderModels, String> {
    let app = app_type(&app)?;
    let models =
        qoder_models::delete_provider(&app, &id, expected).map_err(|error| error.to_string())?;
    state
        .db
        .delete_provider(app.as_str(), &id)
        .map_err(|error| error.to_string())?;
    qoder_models::with_metadata(&state.db, &app, models).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_qoder_model(
    state: State<'_, AppState>,
    app: String,
    model: String,
) -> Result<QoderModels, String> {
    let app = app_type(&app)?;
    qoder_models::set_model(&app, &model)
        .and_then(|models| qoder_models::with_metadata(&state.db, &app, models))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_qoder_usage_script(
    state: State<'_, AppState>,
    app: String,
    id: String,
    script: UsageScript,
) -> Result<QoderModels, String> {
    let app = app_type(&app)?;
    qoder_models::save_usage_script(&state.db, &app, &id, script).map_err(|error| error.to_string())
}
