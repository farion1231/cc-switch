use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashSet;
use tauri::{AppHandle, State};

use crate::cursor::projector;
use crate::cursor::types::{
    CursorEndpoint, CursorModelConfig, CursorModelTestResult, CursorRuntimeState,
};
use crate::provider::Provider;
use crate::store::AppState;

const CURSOR_APP_TYPE: &str = "cursor";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorProviderChanges {
    endpoint: CursorEndpoint,
    upserts: Vec<Provider>,
    deleted_provider_ids: Vec<String>,
}

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
pub fn get_cursor_endpoints(state: State<'_, AppState>) -> Result<Vec<CursorEndpoint>, String> {
    state
        .db
        .get_cursor_endpoints()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_cursor_provider(
    state: State<'_, AppState>,
    provider: Provider,
) -> Result<bool, String> {
    let config: CursorModelConfig = serde_json::from_value(provider.settings_config.clone())
        .map_err(|error| format!("Cursor Provider 配置无效: {error}"))?;
    if config.endpoint_id.trim().is_empty()
        || state
            .db
            .get_cursor_endpoint(&config.endpoint_id)
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err("Cursor Provider 必须属于一个已存在的 Endpoint".to_string());
    }
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
pub async fn save_cursor_providers(
    state: State<'_, AppState>,
    changes: CursorProviderChanges,
) -> Result<bool, String> {
    if changes.endpoint.id.trim().is_empty()
        || changes.endpoint.name.trim().is_empty()
        || changes.endpoint.base_url.trim().is_empty()
        || changes.endpoint.api_key.trim().is_empty()
        || !matches!(
            changes.endpoint.provider_type.as_str(),
            "openai" | "anthropic"
        )
    {
        return Err("Cursor Endpoint 配置无效".to_string());
    }

    let upsert_ids: HashSet<_> = changes
        .upserts
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    if upsert_ids.len() != changes.upserts.len() {
        return Err("Cursor Endpoint 中存在重复的 Provider ID".to_string());
    }

    let deleted_ids: HashSet<_> = changes
        .deleted_provider_ids
        .iter()
        .map(String::as_str)
        .collect();
    if deleted_ids.len() != changes.deleted_provider_ids.len() {
        return Err("Cursor Endpoint 中存在重复的待删除 Provider ID".to_string());
    }
    if let Some(id) = upsert_ids.intersection(&deleted_ids).next() {
        return Err(format!("Cursor Provider '{id}' 不能同时保存和删除"));
    }

    for provider in &changes.upserts {
        let config: CursorModelConfig = serde_json::from_value(provider.settings_config.clone())
            .map_err(|error| format!("Cursor Provider '{}' 配置无效: {error}", provider.name))?;
        if config.endpoint_id != changes.endpoint.id {
            return Err(format!(
                "Cursor Provider '{}' 不属于当前 Endpoint",
                provider.name
            ));
        }
        projector::project_single_model(&provider.id, &provider.name, config)
            .map_err(|error| error.to_string())?;
    }

    state
        .db
        .save_cursor_endpoint_with_provider_changes(
            &changes.endpoint,
            &changes.upserts,
            &changes.deleted_provider_ids,
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
