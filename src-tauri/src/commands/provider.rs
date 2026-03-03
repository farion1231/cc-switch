use indexmap::IndexMap;
use serde_json::Value;
use tauri::State;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::{
    EndpointLatency, ProviderService, ProviderSortUpdate, SpeedtestService, SwitchResult,
};
use crate::store::AppState;
use std::collections::HashSet;
use std::str::FromStr;

#[tauri::command]
pub fn get_providers(
    state: State<'_, AppState>,
    app: String,
) -> Result<IndexMap<String, Provider>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::list(state.inner(), app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_provider(state: State<'_, AppState>, app: String) -> Result<String, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::current(state.inner(), app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::add(state.inner(), app_type, provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::update(state.inner(), app_type, provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::delete(state.inner(), app_type, &id)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_provider_from_live_config(
    state: tauri::State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::remove_from_live_config(state.inner(), app_type, &id)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

fn switch_provider_internal(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<SwitchResult, AppError> {
    ProviderService::switch(state, app_type, id)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn switch_provider_test_hook(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<SwitchResult, AppError> {
    switch_provider_internal(state, app_type, id)
}

#[tauri::command]
pub fn switch_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<SwitchResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    switch_provider_internal(&state, app_type, &id).map_err(|e| e.to_string())
}

fn import_default_config_internal(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    let imported = ProviderService::import_default_config(state, app_type.clone())?;

    if imported {
        // Extract common config snippet (mirrors old startup logic in lib.rs)
        if state
            .db
            .get_config_snippet(app_type.as_str())
            .ok()
            .flatten()
            .is_none()
        {
            match ProviderService::extract_common_config_snippet(state, app_type.clone()) {
                Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                    let _ = state
                        .db
                        .set_config_snippet(app_type.as_str(), Some(snippet));
                }
                _ => {}
            }
        }
    }

    Ok(imported)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn import_default_config_test_hook(
    state: &AppState,
    app_type: AppType,
) -> Result<bool, AppError> {
    import_default_config_internal(state, app_type)
}

#[tauri::command]
pub fn import_default_config(state: State<'_, AppState>, app: String) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    import_default_config_internal(&state, app_type).map_err(Into::into)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn queryProviderUsage(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] providerId: String, // 使用 camelCase 匹配前端
    app: String,
) -> Result<crate::provider::UsageResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::query_usage(state.inner(), app_type, &providerId)
        .await
        .map_err(|e| e.to_string())
}

#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn testUsageScript(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] providerId: String,
    app: String,
    #[allow(non_snake_case)] scriptCode: String,
    timeout: Option<u64>,
    #[allow(non_snake_case)] apiKey: Option<String>,
    #[allow(non_snake_case)] baseUrl: Option<String>,
    #[allow(non_snake_case)] accessToken: Option<String>,
    #[allow(non_snake_case)] userId: Option<String>,
    #[allow(non_snake_case)] templateType: Option<String>,
) -> Result<crate::provider::UsageResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::test_usage_script(
        state.inner(),
        app_type,
        &providerId,
        &scriptCode,
        timeout.unwrap_or(10),
        apiKey.as_deref(),
        baseUrl.as_deref(),
        accessToken.as_deref(),
        userId.as_deref(),
        templateType.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_live_provider_settings(app: String) -> Result<serde_json::Value, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::read_live_settings(app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_api_endpoints(
    urls: Vec<String>,
    #[allow(non_snake_case)] timeoutSecs: Option<u64>,
) -> Result<Vec<EndpointLatency>, String> {
    SpeedtestService::test_endpoints(urls, timeoutSecs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_custom_endpoints(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
) -> Result<Vec<crate::settings::CustomEndpoint>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::get_custom_endpoints(state.inner(), app_type, &providerId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::add_custom_endpoint(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::remove_custom_endpoint(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_endpoint_last_used(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::update_endpoint_last_used(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_providers_sort_order(
    state: State<'_, AppState>,
    app: String,
    updates: Vec<ProviderSortUpdate>,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::update_sort_order(state.inner(), app_type, updates).map_err(|e| e.to_string())
}

use crate::provider::UniversalProvider;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter};

#[derive(Clone, serde::Serialize)]
pub struct UniversalProviderSyncedEvent {
    pub action: String,
    pub id: String,
}

fn emit_universal_provider_synced(app: &AppHandle, action: &str, id: &str) {
    let _ = app.emit(
        "universal-provider-synced",
        UniversalProviderSyncedEvent {
            action: action.to_string(),
            id: id.to_string(),
        },
    );
}

#[tauri::command]
pub fn get_universal_providers(
    state: State<'_, AppState>,
) -> Result<HashMap<String, UniversalProvider>, String> {
    ProviderService::list_universal(state.inner()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_universal_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<UniversalProvider>, String> {
    ProviderService::get_universal(state.inner(), &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: UniversalProvider,
) -> Result<bool, String> {
    let id = provider.id.clone();
    let result =
        ProviderService::upsert_universal(state.inner(), provider).map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "upsert", &id);

    Ok(result)
}

#[tauri::command]
pub fn delete_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let result =
        ProviderService::delete_universal(state.inner(), &id).map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "delete", &id);

    Ok(result)
}

#[tauri::command]
pub fn sync_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let result =
        ProviderService::sync_universal_to_apps(state.inner(), &id).map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "sync", &id);

    Ok(result)
}

#[tauri::command]
pub fn import_opencode_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_opencode_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_opencode_live_provider_ids() -> Result<Vec<String>, String> {
    crate::opencode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

fn extract_model_id(item: &Value) -> Option<String> {
    let raw = item
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("name").and_then(|v| v.as_str()))?;

    let normalized = raw
        .strip_prefix("models/")
        .unwrap_or(raw)
        .trim()
        .to_string();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn extract_models_from_payload(payload: &Value) -> Vec<String> {
    let mut output: Vec<String> = Vec::new();

    if let Some(data) = payload.get("data").and_then(|v| v.as_array()) {
        output.extend(data.iter().filter_map(extract_model_id));
    }

    if let Some(models) = payload.get("models").and_then(|v| v.as_array()) {
        output.extend(models.iter().filter_map(extract_model_id));
    }

    if let Some(items) = payload.as_array() {
        output.extend(items.iter().filter_map(extract_model_id));
    }

    let mut seen = HashSet::new();
    output.retain(|model| seen.insert(model.clone()));
    output
}

fn build_model_endpoint_candidates(app_type: &AppType, base_url: &str) -> Vec<String> {
    let base = base_url.trim_end_matches('/');
    let mut candidates = Vec::new();

    match app_type {
        AppType::Claude => {
            if base.ends_with("/v1") || base.contains("/v1/") {
                candidates.push(format!("{base}/models"));
            } else {
                candidates.push(format!("{base}/v1/models"));
                candidates.push(format!("{base}/models"));
            }
        }
        AppType::Codex => {
            if base.ends_with("/v1") || base.contains("/v1/") {
                candidates.push(format!("{base}/models"));
            } else {
                candidates.push(format!("{base}/v1/models"));
                candidates.push(format!("{base}/models"));
            }
        }
        AppType::Gemini => {
            if base.contains("/v1beta") || base.contains("/v1/") {
                candidates.push(format!("{base}/models"));
            } else {
                candidates.push(format!("{base}/v1beta/models"));
                candidates.push(format!("{base}/models"));
            }
        }
        AppType::OpenCode | AppType::OpenClaw => {}
    }

    let mut seen = HashSet::new();
    candidates.retain(|url| seen.insert(url.clone()));
    candidates
}

#[allow(clippy::collapsible_if)]
async fn fetch_provider_models_internal(
    app_type: &AppType,
    provider: &Provider,
) -> Result<Vec<String>, AppError> {
    if matches!(app_type, AppType::OpenCode | AppType::OpenClaw) {
        return Ok(Vec::new());
    }

    let adapter = crate::proxy::providers::get_adapter(app_type);
    let auth = adapter
        .extract_auth(provider)
        .ok_or_else(|| AppError::Message("未检测到 API Key，无法自动获取模型".to_string()))?;
    let base_url = adapter
        .extract_base_url(provider)
        .map_err(|e| AppError::Message(format!("提取 API 地址失败: {e}")))?;

    let candidates = build_model_endpoint_candidates(app_type, &base_url);
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let proxy_config = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.proxy_config.as_ref());
    let client = crate::proxy::http_client::get_for_provider(proxy_config);
    let timeout = std::time::Duration::from_secs(12);

    for url in candidates {
        let mut request = adapter
            .add_auth_headers(client.get(&url), &auth)
            .header("accept", "application/json")
            .timeout(timeout);

        if matches!(app_type, AppType::Claude)
            && matches!(
                auth.strategy,
                crate::proxy::providers::AuthStrategy::Anthropic
            )
        {
            request = request.header("anthropic-version", "2023-06-01");
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(_) => continue,
        };

        if !response.status().is_success() {
            continue;
        }

        let payload: Value = match response.json().await {
            Ok(payload) => payload,
            Err(_) => continue,
        };

        let models = extract_models_from_payload(&payload);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(Vec::new())
}

#[tauri::command]
pub async fn fetch_provider_models(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
) -> Result<Vec<String>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let providers = state
        .db
        .get_all_providers(app_type.as_str())
        .map_err(|e| e.to_string())?;

    let provider = providers
        .get(&providerId)
        .ok_or_else(|| format!("Provider not found: {providerId}"))?;

    fetch_provider_models_internal(&app_type, provider)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// OpenClaw 专属命令 → 已迁移至 commands/openclaw.rs
// ============================================================================
