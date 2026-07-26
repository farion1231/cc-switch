use indexmap::IndexMap;
use tauri::{AppHandle, State};

use crate::cursor::projector;
use crate::cursor::types::{CursorModelConfig, CursorModelTestResult, CursorRuntimeState};
use crate::provider::Provider;
use crate::store::AppState;

const CURSOR_APP_TYPE: &str = "cursor";

#[tauri::command]
pub fn get_cursor_providers(
    state: State<'_, AppState>,
) -> Result<IndexMap<String, Provider>, String> {
    state
        .db
        .get_all_providers(CURSOR_APP_TYPE)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_cursor_provider(
    state: State<'_, AppState>,
    provider: Provider,
) -> Result<bool, String> {
    let config: CursorModelConfig = serde_json::from_value(provider.settings_config.clone())
        .map_err(|error| format!("Cursor Provider 配置无效: {error}"))?;
    projector::project_single_model(&provider.id, &provider.name, config)
        .map_err(|error| error.to_string())?;
    state
        .db
        .save_provider(CURSOR_APP_TYPE, &provider)
        .map_err(|error| error.to_string())?;
    state
        .cursor_runtime
        .sync_config()
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn delete_cursor_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    state
        .db
        .delete_provider(CURSOR_APP_TYPE, &id)
        .map_err(|error| error.to_string())?;
    state
        .cursor_runtime
        .sync_config()
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn set_cursor_provider_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    let provider = state
        .db
        .get_provider_by_id(&id, CURSOR_APP_TYPE)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Cursor Provider '{id}' 不存在"))?;
    let mut config: CursorModelConfig = serde_json::from_value(provider.settings_config)
        .map_err(|error| format!("Cursor Provider 配置无效: {error}"))?;
    config.enabled = enabled;
    state
        .db
        .update_provider_settings_config(
            CURSOR_APP_TYPE,
            &id,
            &serde_json::to_value(config).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    state
        .cursor_runtime
        .sync_config()
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn get_cursor_runtime_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CursorRuntimeState, String> {
    state
        .cursor_runtime
        .observe_state(&app)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_cursor_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CursorRuntimeState, String> {
    let result = state
        .cursor_runtime
        .start(&app)
        .await
        .map_err(|error| error.to_string())?;
    crate::cursor::usage::spawn_usage_sync(state.cursor_runtime.clone(), state.db.clone());
    Ok(result)
}

#[tauri::command]
pub async fn stop_cursor_runtime(state: State<'_, AppState>) -> Result<CursorRuntimeState, String> {
    let _ = crate::cursor::usage::sync_usage(&state.cursor_runtime, &state.db).await;
    state
        .cursor_runtime
        .stop()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_cursor_ca(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CursorRuntimeState, String> {
    state
        .cursor_runtime
        .install_ca(&app)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_cursor_ca(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CursorRuntimeState, String> {
    state
        .cursor_runtime
        .remove_ca(&app)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sync_cursor_usage(state: State<'_, AppState>) -> Result<u64, String> {
    if !state.cursor_runtime.is_running().await {
        return Ok(0);
    }
    crate::cursor::usage::sync_usage(&state.cursor_runtime, &state.db)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_cursor_model(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<CursorModelTestResult, String> {
    let provider = state
        .db
        .get_provider_by_id(&provider_id, CURSOR_APP_TYPE)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Cursor Provider '{provider_id}' 不存在"))?;
    let config: CursorModelConfig = serde_json::from_value(provider.settings_config)
        .map_err(|error| format!("Cursor Provider 配置无效: {error}"))?;
    let adapter = projector::project_single_model(&provider.id, &provider.name, config)
        .map_err(|error| error.to_string())?;
    state
        .cursor_runtime
        .test_model(&app, &adapter)
        .await
        .map_err(|error| error.to_string())
}
