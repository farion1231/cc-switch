//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use tauri::Emitter;

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await.map_err(|e| e.to_string())
}

/// 停止代理服务器（恢复 Live 配置）
#[tauri::command]
pub async fn stop_proxy_with_restore(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .proxy_service
        .stop_with_restore()
        .await
        .map_err(|e| e.to_string())
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    state
        .proxy_service
        .get_takeover_status()
        .await
        .map_err(|e| e.to_string())
}

/// 为指定应用开启/关闭接管
#[tauri::command]
pub async fn set_proxy_takeover_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .proxy_service
        .set_takeover_for_app(&app_type, enabled)
        .await
        .map_err(|e| e.to_string())
}

/// 获取代理服务器状态
#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    state
        .proxy_service
        .get_status()
        .await
        .map_err(|e| e.to_string())
}

/// 获取代理配置
#[tauri::command]
pub async fn get_proxy_config(state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    state
        .proxy_service
        .get_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新代理配置
#[tauri::command]
pub async fn update_proxy_config(
    state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    state
        .proxy_service
        .update_config(&config)
        .await
        .map_err(|e| e.to_string())
}

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, String> {
    state
        .db
        .get_global_proxy_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新全局代理配置
///
/// 更新统一的全局配置字段，会同时更新三行（claude/codex/gemini）
#[tauri::command]
pub async fn update_global_proxy_config(
    state: tauri::State<'_, AppState>,
    config: GlobalProxyConfig,
) -> Result<(), String> {
    state
        .db
        .update_global_proxy_config(config)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的代理配置
///
/// 返回应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn get_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<AppProxyConfig, String> {
    state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 更新指定应用的代理配置
///
/// 更新应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn update_proxy_config_for_app(
    state: tauri::State<'_, AppState>,
    config: AppProxyConfig,
) -> Result<(), String> {
    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())
}

async fn get_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    state.db.get_default_cost_multiplier(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_default_cost_multiplier_internal(state, app_type).await
}

/// 获取默认成本倍率
#[tauri::command]
pub async fn get_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_default_cost_multiplier_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    state.db.set_default_cost_multiplier(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_default_cost_multiplier_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_default_cost_multiplier_internal(state, app_type, value).await
}

/// 设置默认成本倍率
#[tauri::command]
pub async fn set_default_cost_multiplier(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_default_cost_multiplier_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

async fn get_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    state.db.get_pricing_model_source(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    get_pricing_model_source_internal(state, app_type).await
}

/// 获取计费模式来源
#[tauri::command]
pub async fn get_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, String> {
    get_pricing_model_source_internal(&state, &app_type)
        .await
        .map_err(|e| e.to_string())
}

async fn set_pricing_model_source_internal(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    state.db.set_pricing_model_source(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    set_pricing_model_source_internal(state, app_type, value).await
}

/// 设置计费模式来源
#[tauri::command]
pub async fn set_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), String> {
    set_pricing_model_source_internal(&state, &app_type, &value)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_proxy_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(state.proxy_service.is_running().await)
}

#[tauri::command]
pub async fn is_live_takeover_active(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state
        .proxy_service
        .is_takeover_active()
        .await
        .map_err(|e| e.to_string())
}

/// 代理模式下切换供应商（热切换）
#[tauri::command]
pub async fn switch_proxy_provider(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    state
        .proxy_service
        .switch_proxy_target(&app_type, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== 故障转移相关命令 ====================

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    state
        .db
        .get_provider_health(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 重置熔断器
///
/// 重置后会检查是否应该切回队列中优先级更高的供应商：
/// 1. 检查自动故障转移是否开启
/// 2. 如果恢复的供应商在队列中优先级更高（queue_order 更小），则自动切换
#[tauri::command]
pub async fn reset_circuit_breaker(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<(), String> {
    state
        .proxy_service
        .reset_provider_circuit_breaker(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    let config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    if config.enabled && config.auto_failover_enabled && state.proxy_service.is_running().await {
        let current_id = state
            .db
            .get_current_provider(&app_type)
            .map_err(|e| e.to_string())?;
        let queue = state
            .db
            .get_failover_queue(&app_type)
            .map_err(|e| e.to_string())?;

        let restored_priority = queue
            .iter()
            .find(|item| item.provider_id == provider_id)
            .and_then(|item| item.sort_index);
        let current_priority = current_id
            .as_ref()
            .and_then(|current| queue.iter().find(|item| item.provider_id == *current))
            .and_then(|item| item.sort_index);

        if let (Some(restored), Some(current)) = (restored_priority, current_priority) {
            if restored < current {
                state
                    .proxy_service
                    .switch_proxy_target(&app_type, &provider_id)
                    .await
                    .map_err(|e| e.to_string())?;
                let switched_provider_id = provider_id.clone();
                let event_data = serde_json::json!({
                    "appType": app_type,
                    "providerId": switched_provider_id,
                    "source": "circuitReset"
                });
                let _ = app_handle.emit("provider-switched", event_data);
                if let Ok(new_menu) = crate::tray::create_tray_menu(&app_handle, &state) {
                    if let Some(tray) = app_handle.tray_by_id("main") {
                        let _ = tray.set_menu(Some(new_menu));
                    }
                }
            }
        }
    }

    Ok(())
}

/// 获取熔断器配置
#[tauri::command]
pub async fn get_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
) -> Result<CircuitBreakerConfig, String> {
    state
        .db
        .get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新熔断器配置
#[tauri::command]
pub async fn update_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
    config: CircuitBreakerConfig,
) -> Result<(), String> {
    state
        .db
        .update_circuit_breaker_config(&config)
        .await
        .map_err(|e| e.to_string())?;
    state
        .proxy_service
        .update_circuit_breaker_configs(config)
        .await
        .map_err(|e| e.to_string())
}

/// 获取熔断器统计信息（仅当代理服务器运行时）
#[tauri::command]
pub async fn get_circuit_breaker_stats(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Option<CircuitBreakerStats>, String> {
    let health = state
        .db
        .get_provider_health(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())?;
    let config = state
        .db
        .get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())?;

    Ok(Some(CircuitBreakerStats {
        state: if health.is_healthy {
            crate::proxy::CircuitState::Closed
        } else {
            crate::proxy::CircuitState::Open
        },
        consecutive_failures: health.consecutive_failures,
        consecutive_successes: 0,
        total_requests: health.consecutive_failures,
        failed_requests: if health.is_healthy {
            0
        } else {
            health.consecutive_failures.max(config.failure_threshold)
        },
    }))
}
