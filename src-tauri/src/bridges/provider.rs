use std::collections::HashMap;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, UniversalProvider, UsageResult};
use crate::services::{
    EndpointLatency, ProviderService as LegacyProviderService, ProviderSortUpdate, SpeedtestService,
    SwitchResult,
};
use crate::settings::CustomEndpoint;
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

pub fn legacy_get_providers(
    state: &AppState,
    app_type: AppType,
) -> Result<IndexMap<String, Provider>, AppError> {
    LegacyProviderService::list(state, app_type)
}

pub fn get_providers(app_type: AppType) -> Result<IndexMap<String, Provider>, AppError> {
    let providers = with_core_state(|state| {
        cc_switch_core::ProviderService::list(state, to_core_app_type(app_type.clone()))
    })?;
    convert(providers)
}

pub fn legacy_get_current_provider(state: &AppState, app_type: AppType) -> Result<String, AppError> {
    LegacyProviderService::current(state, app_type)
}

pub fn get_current_provider(app_type: AppType) -> Result<String, AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::current(state, to_core_app_type(app_type))
    })
}

pub fn legacy_add_provider(
    state: &AppState,
    app_type: AppType,
    provider: Provider,
) -> Result<bool, AppError> {
    LegacyProviderService::add(state, app_type, provider)
}

pub fn add_provider(app_type: AppType, provider: Provider) -> Result<bool, AppError> {
    let provider = convert(provider)?;
    with_core_state(|state| {
        cc_switch_core::ProviderService::add(state, to_core_app_type(app_type), provider)
    })
}

pub fn legacy_update_provider(
    state: &AppState,
    app_type: AppType,
    provider: Provider,
) -> Result<bool, AppError> {
    LegacyProviderService::update(state, app_type, provider)
}

pub fn update_provider(app_type: AppType, provider: Provider) -> Result<bool, AppError> {
    let provider = convert(provider)?;
    with_core_state(|state| {
        cc_switch_core::ProviderService::update(state, to_core_app_type(app_type), provider)
    })
}

pub fn legacy_delete_provider(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<(), AppError> {
    LegacyProviderService::delete(state, app_type, id)
}

pub fn delete_provider(app_type: AppType, id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::ProviderService::delete(state, to_core_app_type(app_type), id))
}

pub fn legacy_remove_provider_from_live_config(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<(), AppError> {
    LegacyProviderService::remove_from_live_config(state, app_type, id)
}

pub fn remove_provider_from_live_config(app_type: AppType, id: &str) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::remove_from_live_config(
            state,
            to_core_app_type(app_type),
            id,
        )
    })
}

pub fn legacy_switch_provider(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<SwitchResult, AppError> {
    LegacyProviderService::switch(state, app_type, id)
}

pub fn switch_provider(app_type: AppType, id: &str) -> Result<SwitchResult, AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::switch(state, to_core_app_type(app_type), id)?;
        Ok(SwitchResult::default())
    })
}

pub fn legacy_import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    let imported = LegacyProviderService::import_default_config(state, app_type.clone())?;
    if imported && state.db.get_config_snippet(app_type.as_str())?.is_none() {
        match LegacyProviderService::extract_common_config_snippet(state, app_type.clone()) {
            Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                let _ = state
                    .db
                    .set_config_snippet(app_type.as_str(), Some(snippet));
            }
            _ => {}
        }
    }
    Ok(imported)
}

pub fn import_default_config(app_type: AppType) -> Result<bool, AppError> {
    with_core_state(|state| {
        let imported =
            cc_switch_core::ProviderService::import_default_config(state, to_core_app_type(app_type.clone()))?;
        if imported && state.db.get_config_snippet(app_type.as_str())?.is_none() {
            match cc_switch_core::ProviderService::extract_common_config_snippet(
                state,
                to_core_app_type(app_type.clone()),
            ) {
                Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                    let _ = state
                        .db
                        .set_config_snippet(app_type.as_str(), Some(snippet));
                }
                _ => {}
            }
        }
        Ok(imported)
    })
}

pub async fn legacy_query_provider_usage(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, AppError> {
    LegacyProviderService::query_usage(state, app_type, provider_id).await
}

pub async fn query_provider_usage(
    app_type: AppType,
    provider_id: &str,
) -> Result<UsageResult, AppError> {
    let state = core_state()?;
    let usage = cc_switch_core::ProviderService::query_usage(
        &state,
        to_core_app_type(app_type),
        provider_id,
    )
    .await
    .map_err(map_core_err)?;
    convert(usage)
}

#[allow(clippy::too_many_arguments)]
pub async fn legacy_test_usage_script(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    script_code: &str,
    timeout: u64,
    api_key: Option<&str>,
    base_url: Option<&str>,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    LegacyProviderService::test_usage_script(
        state,
        app_type,
        provider_id,
        script_code,
        timeout,
        api_key,
        base_url,
        access_token,
        user_id,
        template_type,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn test_usage_script(
    app_type: AppType,
    provider_id: &str,
    script_code: &str,
    timeout: u64,
    api_key: Option<&str>,
    base_url: Option<&str>,
    access_token: Option<&str>,
    user_id: Option<&str>,
    template_type: Option<&str>,
) -> Result<UsageResult, AppError> {
    let state = core_state()?;
    let usage = cc_switch_core::ProviderService::test_usage_script(
        &state,
        to_core_app_type(app_type),
        provider_id,
        script_code,
        timeout,
        api_key,
        base_url,
        access_token,
        user_id,
        template_type,
    )
    .await
    .map_err(map_core_err)?;
    convert(usage)
}

pub fn legacy_read_live_provider_settings(app_type: AppType) -> Result<Value, AppError> {
    LegacyProviderService::read_live_settings(app_type)
}

pub fn read_live_provider_settings(app_type: AppType) -> Result<Value, AppError> {
    cc_switch_core::ProviderService::read_live_settings(to_core_app_type(app_type))
        .map_err(map_core_err)
}

pub async fn legacy_test_api_endpoints(
    urls: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<EndpointLatency>, AppError> {
    SpeedtestService::test_endpoints(urls, timeout_secs)
        .await
        .map_err(|err| AppError::Message(err.to_string()))
}

pub async fn test_api_endpoints(
    urls: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<EndpointLatency>, AppError> {
    cc_switch_core::SpeedtestService::test_endpoints(urls, timeout_secs)
        .await
        .map_err(map_core_err)
        .map(|latencies| {
            latencies
                .into_iter()
                .map(|latency| EndpointLatency {
                    url: latency.url,
                    latency: latency.latency_ms.map(u128::from),
                    status: None,
                    error: latency.error,
                })
                .collect()
        })
}

pub fn legacy_get_custom_endpoints(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<Vec<CustomEndpoint>, AppError> {
    LegacyProviderService::get_custom_endpoints(state, app_type, provider_id)
}

pub fn get_custom_endpoints(
    app_type: AppType,
    provider_id: &str,
) -> Result<Vec<CustomEndpoint>, AppError> {
    let endpoints = with_core_state(|state| {
        cc_switch_core::ProviderService::get_custom_endpoints(
            state,
            to_core_app_type(app_type),
            provider_id,
        )
    })?;
    convert(endpoints)
}

pub fn legacy_add_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    LegacyProviderService::add_custom_endpoint(state, app_type, provider_id, url)
}

pub fn add_custom_endpoint(app_type: AppType, provider_id: &str, url: String) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::add_custom_endpoint(
            state,
            to_core_app_type(app_type),
            provider_id,
            url,
        )
    })
}

pub fn legacy_remove_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    LegacyProviderService::remove_custom_endpoint(state, app_type, provider_id, url)
}

pub fn remove_custom_endpoint(
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::remove_custom_endpoint(
            state,
            to_core_app_type(app_type),
            provider_id,
            url,
        )
    })
}

pub fn legacy_update_endpoint_last_used(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    LegacyProviderService::update_endpoint_last_used(state, app_type, provider_id, url)
}

pub fn update_endpoint_last_used(
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::ProviderService::update_endpoint_last_used(
            state,
            to_core_app_type(app_type),
            provider_id,
            url,
        )
    })
}

pub fn legacy_update_providers_sort_order(
    state: &AppState,
    app_type: AppType,
    updates: Vec<ProviderSortUpdate>,
) -> Result<bool, AppError> {
    LegacyProviderService::update_sort_order(state, app_type, updates)
}

pub fn update_providers_sort_order(
    app_type: AppType,
    updates: Vec<ProviderSortUpdate>,
) -> Result<bool, AppError> {
    let updates = updates
        .into_iter()
        .map(|update| cc_switch_core::services::provider::ProviderSortUpdate {
            id: update.id,
            sort_index: update.sort_index,
        })
        .collect();
    with_core_state(|state| {
        cc_switch_core::ProviderService::update_sort_order(
            state,
            to_core_app_type(app_type),
            updates,
        )
    })
}

pub fn legacy_get_universal_providers(
    state: &AppState,
) -> Result<HashMap<String, UniversalProvider>, AppError> {
    LegacyProviderService::list_universal(state)
}

pub fn get_universal_providers() -> Result<HashMap<String, UniversalProvider>, AppError> {
    let providers = with_core_state(cc_switch_core::ProviderService::list_universal)?;
    convert(providers)
}

pub fn legacy_get_universal_provider(
    state: &AppState,
    id: &str,
) -> Result<Option<UniversalProvider>, AppError> {
    LegacyProviderService::get_universal(state, id)
}

pub fn get_universal_provider(id: &str) -> Result<Option<UniversalProvider>, AppError> {
    let provider = with_core_state(|state| cc_switch_core::ProviderService::get_universal(state, id))?;
    convert(provider)
}

pub fn legacy_upsert_universal_provider(
    state: &AppState,
    provider: UniversalProvider,
) -> Result<bool, AppError> {
    LegacyProviderService::upsert_universal(state, provider)
}

pub fn upsert_universal_provider(provider: UniversalProvider) -> Result<bool, AppError> {
    let provider = convert(provider)?;
    with_core_state(|state| cc_switch_core::ProviderService::upsert_universal(state, provider))
}

pub fn legacy_delete_universal_provider(state: &AppState, id: &str) -> Result<(), AppError> {
    LegacyProviderService::delete_universal(state, id).map(|_| ())
}

pub fn delete_universal_provider(id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::ProviderService::delete_universal(state, id))
}

pub fn legacy_sync_universal_provider(state: &AppState, id: &str) -> Result<(), AppError> {
    LegacyProviderService::sync_universal_to_apps(state, id).map(|_| ())
}

pub fn sync_universal_provider(id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::ProviderService::sync_universal_to_apps(state, id))
}

pub fn legacy_import_opencode_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    crate::services::provider::import_opencode_providers_from_live(state)
}

pub fn import_opencode_providers_from_live() -> Result<usize, AppError> {
    with_core_state(cc_switch_core::ProviderService::import_opencode_providers_from_live)
}

pub fn get_opencode_live_provider_ids() -> Result<Vec<String>, AppError> {
    cc_switch_core::opencode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(map_core_err)
}
