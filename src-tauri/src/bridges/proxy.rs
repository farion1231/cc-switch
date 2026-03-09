use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::bridges::support::{convert, map_core_err};
use crate::database::FailoverQueueItem as LegacyFailoverQueueItem;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::types::{
    AppProxyConfig, GlobalProxyConfig, ProviderHealth, ProxyConfig, ProxyServerInfo,
    ProxyStatus, ProxyTakeoverStatus,
};
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};

struct RuntimeProxyState {
    key: PathBuf,
    state: cc_switch_core::AppState,
}

fn runtime_slot() -> &'static Mutex<Option<RuntimeProxyState>> {
    static SLOT: OnceLock<Mutex<Option<RuntimeProxyState>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn runtime_key() -> PathBuf {
    cc_switch_core::config::config_dir()
}

fn runtime_state() -> Result<cc_switch_core::AppState, AppError> {
    let key = runtime_key();
    let slot = runtime_slot();
    let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());

    if let Some(runtime) = guard.as_ref() {
        if runtime.key == key {
            return Ok(runtime.state.clone());
        }
    }

    let state = cc_switch_core::AppState::new(cc_switch_core::Database::new().map_err(map_core_err)?);
    state.run_startup_maintenance();
    let cloned = state.clone();
    *guard = Some(RuntimeProxyState { key, state });
    Ok(cloned)
}

#[allow(dead_code)]
pub fn reset_runtime_for_tests() {
    let slot = runtime_slot();
    let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());
    *guard = None;
}

fn map_proxy_err(err: String) -> AppError {
    AppError::Message(err)
}

fn convert_failover_queue(items: Vec<cc_switch_core::proxy::FailoverQueueItem>) -> Vec<LegacyFailoverQueueItem> {
    items.into_iter()
        .map(|item| LegacyFailoverQueueItem {
            provider_id: item.provider_id,
            provider_name: item.provider_name,
            sort_index: match item.priority {
                999_999 => None,
                priority => usize::try_from(priority).ok(),
            },
        })
        .collect()
}

pub async fn start_proxy_server() -> Result<ProxyServerInfo, AppError> {
    let state = runtime_state()?;
    let info = state.proxy_service.start().await.map_err(map_proxy_err)?;
    convert(info)
}

pub async fn stop_proxy_with_restore() -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .stop_with_restore()
        .await
        .map_err(map_proxy_err)
}

pub async fn get_proxy_takeover_status() -> Result<ProxyTakeoverStatus, AppError> {
    let state = runtime_state()?;
    let status = state
        .proxy_service
        .get_takeover_status()
        .await
        .map_err(map_proxy_err)?;
    convert(status)
}

pub async fn set_proxy_takeover_for_app(app_type: &str, enabled: bool) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .set_takeover_for_app(app_type, enabled)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_proxy_status() -> Result<ProxyStatus, AppError> {
    let state = runtime_state()?;
    let status = state.proxy_service.get_status().await.map_err(map_proxy_err)?;
    convert(status)
}

pub async fn get_proxy_config() -> Result<ProxyConfig, AppError> {
    let state = runtime_state()?;
    let config = state.proxy_service.get_config().await.map_err(map_proxy_err)?;
    convert(config)
}

pub async fn update_proxy_config(config: ProxyConfig) -> Result<(), AppError> {
    let state = runtime_state()?;
    let config = convert(config)?;
    state
        .proxy_service
        .update_config(&config)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_global_proxy_config() -> Result<GlobalProxyConfig, AppError> {
    let state = runtime_state()?;
    let config = state
        .proxy_service
        .get_global_proxy_config()
        .await
        .map_err(map_proxy_err)?;
    convert(config)
}

pub async fn update_global_proxy_config(config: GlobalProxyConfig) -> Result<(), AppError> {
    let state = runtime_state()?;
    let config = convert(config)?;
    state
        .proxy_service
        .update_global_proxy_config(config)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_proxy_config_for_app(app_type: &str) -> Result<AppProxyConfig, AppError> {
    let state = runtime_state()?;
    let config = state
        .proxy_service
        .get_app_proxy_config(app_type)
        .await
        .map_err(map_proxy_err)?;
    convert(config)
}

pub async fn update_proxy_config_for_app(config: AppProxyConfig) -> Result<(), AppError> {
    let state = runtime_state()?;
    let config = convert(config)?;
    state
        .proxy_service
        .update_app_proxy_config(config)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_default_cost_multiplier(app_type: &str) -> Result<String, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .get_default_cost_multiplier(app_type)
        .await
        .map_err(map_proxy_err)
}

pub async fn set_default_cost_multiplier(app_type: &str, value: &str) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .set_default_cost_multiplier(app_type, value)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_pricing_model_source(app_type: &str) -> Result<String, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .get_pricing_model_source(app_type)
        .await
        .map_err(map_proxy_err)
}

pub async fn set_pricing_model_source(app_type: &str, value: &str) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .set_pricing_model_source(app_type, value)
        .await
        .map_err(map_proxy_err)
}

pub async fn is_proxy_running() -> Result<bool, AppError> {
    let state = runtime_state()?;
    Ok(state.proxy_service.is_running().await)
}

pub async fn is_live_takeover_active() -> Result<bool, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .is_takeover_active()
        .await
        .map_err(map_proxy_err)
}

pub async fn switch_proxy_provider(app_type: &str, provider_id: &str) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .switch_proxy_target(app_type, provider_id)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_provider_health(
    provider_id: &str,
    app_type: &str,
) -> Result<ProviderHealth, AppError> {
    let state = runtime_state()?;
    let health = state
        .proxy_service
        .get_provider_health(provider_id, app_type)
        .await
        .map_err(map_proxy_err)?;
    convert(health)
}

pub async fn get_circuit_breaker_config() -> Result<CircuitBreakerConfig, AppError> {
    let state = runtime_state()?;
    let config = state
        .proxy_service
        .get_circuit_breaker_config()
        .await
        .map_err(map_proxy_err)?;
    convert(config)
}

pub async fn update_circuit_breaker_config(config: CircuitBreakerConfig) -> Result<(), AppError> {
    let state = runtime_state()?;
    let config = convert(config)?;
    state
        .proxy_service
        .save_circuit_breaker_config(config)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_circuit_breaker_stats(
    provider_id: &str,
    app_type: &str,
) -> Result<Option<CircuitBreakerStats>, AppError> {
    let state = runtime_state()?;
    let stats = state
        .proxy_service
        .get_circuit_breaker_stats(provider_id, app_type)
        .await
        .map_err(map_proxy_err)?;
    stats.map(convert).transpose()
}

pub async fn reset_circuit_breaker(
    provider_id: &str,
    app_type: &str,
) -> Result<Option<String>, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .reset_provider_circuit(provider_id, app_type)
        .await
        .map_err(map_proxy_err)?;

    let config = state
        .proxy_service
        .get_app_proxy_config(app_type)
        .await
        .map_err(map_proxy_err)?;

    if !(config.enabled && config.auto_failover_enabled && state.proxy_service.is_running().await) {
        return Ok(None);
    }

    let Some(current_id) = state.db.get_current_provider(app_type).map_err(map_core_err)? else {
        return Ok(None);
    };

    let queue = state.db.get_failover_queue(app_type).map_err(map_core_err)?;
    let restored_priority = queue
        .iter()
        .find(|item| item.provider_id == provider_id)
        .map(|item| item.priority);
    let current_priority = queue
        .iter()
        .find(|item| item.provider_id == current_id)
        .map(|item| item.priority);

    if let (Some(restored), Some(current)) = (restored_priority, current_priority) {
        if restored < current {
            state
                .proxy_service
                .switch_proxy_target(app_type, provider_id)
                .await
                .map_err(map_proxy_err)?;
            return Ok(Some(provider_id.to_string()));
        }
    }

    Ok(None)
}

pub async fn get_failover_queue(app_type: &str) -> Result<Vec<LegacyFailoverQueueItem>, AppError> {
    let state = runtime_state()?;
    let queue = state
        .proxy_service
        .get_failover_queue(app_type)
        .await
        .map_err(map_proxy_err)?;
    Ok(convert_failover_queue(queue))
}

pub async fn get_available_providers_for_failover(app_type: &str) -> Result<Vec<Provider>, AppError> {
    let state = runtime_state()?;
    let providers = state
        .proxy_service
        .get_available_providers_for_failover(app_type)
        .await
        .map_err(map_proxy_err)?;
    convert(providers)
}

pub async fn add_to_failover_queue(app_type: &str, provider_id: &str) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .add_to_failover_queue(app_type, provider_id)
        .await
        .map_err(map_proxy_err)
}

pub async fn remove_from_failover_queue(app_type: &str, provider_id: &str) -> Result<(), AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .remove_from_failover_queue(app_type, provider_id)
        .await
        .map_err(map_proxy_err)
}

pub async fn get_auto_failover_enabled(app_type: &str) -> Result<bool, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .get_auto_failover_enabled(app_type)
        .await
        .map_err(map_proxy_err)
}

pub async fn set_auto_failover_enabled(
    app_type: &str,
    enabled: bool,
) -> Result<Option<String>, AppError> {
    let state = runtime_state()?;
    state
        .proxy_service
        .set_auto_failover_enabled(app_type, enabled)
        .await
        .map_err(map_proxy_err)
}
