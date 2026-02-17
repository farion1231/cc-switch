use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::{EndpointLatency, ProviderService, ProviderSortUpdate, SpeedtestService};
use crate::store::AppState;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn switch_provider_internal(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
    ProviderService::switch(state, app_type, id)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn switch_provider_test_hook(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<(), AppError> {
    switch_provider_internal(state, app_type, id)
}

#[tauri::command]
pub fn switch_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    switch_provider_internal(&state, app_type, &id)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

fn import_default_config_internal(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    ProviderService::import_default_config(state, app_type)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

const REMOTE_MODELS_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const REMOTE_MODELS_CACHE_NAMESPACE: &str = "cc-switch/remote-models";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteModelsCacheEntry {
    expires_at_epoch_ms: i64,
    models: Vec<RemoteModelInfo>,
}

fn now_epoch_ms() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if elapsed > i64::MAX as u128 {
        i64::MAX
    } else {
        elapsed as i64
    }
}

fn remote_models_cache_base_dir() -> PathBuf {
    let tmp_root = Path::new("/tmp");
    if tmp_root.is_dir() {
        return tmp_root.join(REMOTE_MODELS_CACHE_NAMESPACE);
    }
    std::env::temp_dir().join(REMOTE_MODELS_CACHE_NAMESPACE)
}

fn remote_models_cache_path(
    base_url: &str,
    api_key: &str,
    api_format: &str,
    proxy_config: Option<&crate::provider::ProviderProxyConfig>,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(api_format.as_bytes());
    hasher.update(b"|");
    hasher.update(base_url.as_bytes());
    hasher.update(b"|");
    hasher.update(api_key.as_bytes());
    hasher.update(b"|");

    if let Some(config) = proxy_config {
        match serde_json::to_vec(config) {
            Ok(bytes) => hasher.update(bytes),
            Err(err) => log::debug!(
                "[RemoteModels] Failed to serialize proxy config for cache key: {err}"
            ),
        }
    }

    let digest = format!("{:x}", hasher.finalize());
    remote_models_cache_base_dir().join(format!("{digest}.json"))
}

fn read_remote_models_cache(path: &Path) -> Option<Vec<RemoteModelInfo>> {
    let content = fs::read(path).ok()?;
    let entry: RemoteModelsCacheEntry = serde_json::from_slice(&content).ok()?;
    if entry.expires_at_epoch_ms <= now_epoch_ms() {
        let _ = fs::remove_file(path);
        return None;
    }
    if entry.models.is_empty() {
        return None;
    }
    Some(entry.models)
}

fn write_remote_models_cache(path: &Path, models: &[RemoteModelInfo]) {
    if models.is_empty() {
        return;
    }

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::debug!("[RemoteModels] Failed to create cache dir {:?}: {err}", parent);
            return;
        }
    }

    let entry = RemoteModelsCacheEntry {
        expires_at_epoch_ms: now_epoch_ms().saturating_add(REMOTE_MODELS_CACHE_TTL_MS),
        models: models.to_vec(),
    };

    let payload = match serde_json::to_vec(&entry) {
        Ok(value) => value,
        Err(err) => {
            log::debug!("[RemoteModels] Failed to serialize cache payload: {err}");
            return;
        }
    };

    let tmp_path = path.with_extension(format!("{}.{}.tmp", std::process::id(), now_epoch_ms()));

    if let Err(err) = fs::write(&tmp_path, payload) {
        log::debug!(
            "[RemoteModels] Failed to write cache file {:?}: {err}",
            tmp_path
        );
        return;
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        log::debug!(
            "[RemoteModels] Failed to finalize cache file {:?}: {err}",
            path
        );
    }
}

fn extract_string_from_keys(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| obj.get(*key))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn is_likely_model_id(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    if !trimmed.chars().any(|c| c.is_ascii_alphanumeric()) {
        return false;
    }

    let normalized = trimmed.to_ascii_lowercase();
    !matches!(
        normalized.as_str(),
        "data"
            | "model"
            | "models"
            | "object"
            | "meta"
            | "metadata"
            | "status"
            | "error"
            | "errors"
            | "message"
            | "messages"
            | "id"
            | "list"
            | "items"
            | "count"
            | "total"
    )
}

fn parse_model_entry(entry: &Value) -> Option<RemoteModelInfo> {
    let obj = entry.as_object()?;
    let id = extract_string_from_keys(obj, &["id", "modelId", "model", "name"])?;
    if !is_likely_model_id(&id) {
        return None;
    }
    let display_name = extract_string_from_keys(obj, &["display_name", "displayName", "label"])
        .filter(|name| name != &id);
    let provider = extract_string_from_keys(obj, &["owned_by", "ownedBy", "provider", "owner", "vendor"]);

    Some(RemoteModelInfo {
        id,
        provider,
        display_name,
    })
}

fn parse_remote_models(payload: &Value) -> Vec<RemoteModelInfo> {
    let mut collected: Vec<RemoteModelInfo> = Vec::new();

    if let Some(data) = payload.get("data").and_then(|v| v.as_array()) {
        for item in data {
            if let Some(model) = parse_model_entry(item) {
                collected.push(model);
            }
        }
    } else if let Some(models) = payload.get("models") {
        if let Some(arr) = models.as_array() {
            for item in arr {
                if let Some(model) = parse_model_entry(item) {
                    collected.push(model);
                }
            }
        } else if let Some(obj) = models.as_object() {
            for (model_id, model_value) in obj {
                if let Some(mut parsed) = parse_model_entry(model_value) {
                    if parsed.id.is_empty() {
                        parsed.id = model_id.trim().to_string();
                    }
                    if !parsed.id.is_empty() {
                        collected.push(parsed);
                    }
                } else if model_value.is_object() && is_likely_model_id(model_id) {
                    let trimmed = model_id.trim();
                    if !trimmed.is_empty() {
                        collected.push(RemoteModelInfo {
                            id: trimmed.to_string(),
                            provider: None,
                            display_name: None,
                        });
                    }
                }
            }
        }
    } else if let Some(arr) = payload.as_array() {
        for item in arr {
            if let Some(model) = parse_model_entry(item) {
                collected.push(model);
            }
        }
    } else if let Some(single) = parse_model_entry(payload) {
        collected.push(single);
    }

    let mut seen = HashSet::new();
    let mut deduped: Vec<RemoteModelInfo> = collected
        .into_iter()
        .filter(|m| is_likely_model_id(&m.id) && seen.insert(m.id.clone()))
        .collect();
    deduped.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    deduped
}

#[cfg(test)]
mod remote_models_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_remote_models_filters_non_model_map_entries() {
        let payload = json!({
            "models": {
                "meta": { "note": "not a model" },
                "status": { "healthy": true },
                "gpt-4o": { "id": "gpt-4o", "owned_by": "openai" }
            }
        });

        let models = parse_remote_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-4o");
    }

    #[test]
    fn parse_remote_models_keeps_likely_model_ids_from_object_map_keys() {
        let payload = json!({
            "models": {
                "claude-3-5-sonnet": {
                    "input_cost": "3"
                }
            }
        });

        let models = parse_remote_models(&payload);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "claude-3-5-sonnet");
    }
}

fn build_model_urls(base_url: &str) -> Vec<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let primary = if trimmed.ends_with("/v1") {
        format!("{trimmed}/models")
    } else {
        format!("{trimmed}/v1/models")
    };
    let fallback = if trimmed.ends_with("/v1") {
        format!("{trimmed}/v1/models")
    } else {
        format!("{trimmed}/models")
    };

    let mut seen = HashSet::new();
    vec![primary, fallback]
        .into_iter()
        .filter(|url| seen.insert(url.clone()))
        .collect()
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn enumerate_provider_models(
    #[allow(non_snake_case)] baseUrl: String,
    #[allow(non_snake_case)] apiKey: String,
    #[allow(non_snake_case)] apiFormat: Option<String>,
    #[allow(non_snake_case)] proxyConfig: Option<crate::provider::ProviderProxyConfig>,
    #[allow(non_snake_case)] forceRefresh: Option<bool>,
) -> Result<Vec<RemoteModelInfo>, String> {
    let base_url = baseUrl.trim();
    let api_key = apiKey.trim();
    if base_url.is_empty() {
        return Err("baseUrl is required".to_string());
    }
    if api_key.is_empty() {
        return Err("apiKey is required".to_string());
    }

    let format = apiFormat
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("openai_chat")
        .to_lowercase();
    if format != "openai_chat" && format != "anthropic" {
        return Err(format!("Unsupported apiFormat: {format}"));
    }

    let force_refresh = forceRefresh.unwrap_or(false);
    let cache_path = remote_models_cache_path(base_url, api_key, &format, proxyConfig.as_ref());
    if !force_refresh {
        if let Some(cached_models) = read_remote_models_cache(&cache_path) {
            log::info!(
                "[RemoteModels] Using cached model list ({}) from {:?}",
                cached_models.len(),
                cache_path
            );
            return Ok(cached_models);
        }
    }

    let urls = build_model_urls(base_url);
    let client = crate::proxy::http_client::get_for_provider(proxyConfig.as_ref());
    let mut errors: Vec<String> = Vec::new();

    for url in urls {
        log::info!("[RemoteModels] Fetching models from: {url}");

        let mut request = client
            .get(&url)
            .header("accept", "application/json")
            .header("authorization", format!("Bearer {api_key}"))
            .timeout(std::time::Duration::from_secs(20));

        if format == "anthropic" {
            request = request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01");
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(err) => {
                errors.push(format!("{url}: request failed: {err}"));
                continue;
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let body_snippet = truncate_for_log(&body, 240);
            log::warn!("[RemoteModels] API returned {status}: {body_snippet}");
            errors.push(format!("{url}: API returned {status}: {body_snippet}"));
            continue;
        }

        let payload: Value = match response.json().await {
            Ok(json) => json,
            Err(err) => {
                errors.push(format!("{url}: failed to parse response: {err}"));
                continue;
            }
        };

        let models = parse_remote_models(&payload);
        if !models.is_empty() {
            log::info!("[RemoteModels] Fetched {} model(s)", models.len());
            write_remote_models_cache(&cache_path, &models);
            return Ok(models);
        }

        errors.push(format!("{url}: API returned an empty model list"));
    }

    Err(format!("Failed to fetch models: {}", errors.join(" | ")))
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

// ============================================================================
// OpenClaw 专属命令 → 已迁移至 commands/openclaw.rs
// ============================================================================
