//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::bridges::proxy as proxy_bridge;
use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use tauri::Emitter;

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    _state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    proxy_bridge::start_proxy_server()
        .await
        .map_err(|e| e.to_string())
}

/// 停止代理服务器（恢复 Live 配置）
#[tauri::command]
pub async fn stop_proxy_with_restore(_state: tauri::State<'_, AppState>) -> Result<(), String> {
    proxy_bridge::stop_proxy_with_restore()
        .await
        .map_err(|e| e.to_string())
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    _state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    proxy_bridge::get_proxy_takeover_status()
        .await
        .map_err(|e| e.to_string())
}

/// 为指定应用开启/关闭接管
#[tauri::command]
pub async fn set_proxy_takeover_for_app(
    _state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    proxy_bridge::set_proxy_takeover_for_app(&app_type, enabled)
        .await
        .map_err(|e| e.to_string())
}

/// 获取代理服务器状态
#[tauri::command]
pub async fn get_proxy_status(_state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    proxy_bridge::get_proxy_status()
        .await
        .map_err(|e| e.to_string())
}

/// 获取代理配置
#[tauri::command]
pub async fn get_proxy_config(_state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    proxy_bridge::get_proxy_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新代理配置
#[tauri::command]
pub async fn update_proxy_config(
    _state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    proxy_bridge::update_proxy_config(config)
        .await
        .map_err(|e| e.to_string())
}

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    _state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, String> {
    proxy_bridge::get_global_proxy_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新全局代理配置
///
/// 更新统一的全局配置字段，会同时更新三行（claude/codex/gemini）
#[tauri::command]
pub async fn update_global_proxy_config(
    _state: tauri::State<'_, AppState>,
    config: GlobalProxyConfig,
) -> Result<(), String> {
    proxy_bridge::update_global_proxy_config(config)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的代理配置
///
/// 返回应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn get_proxy_config_for_app(
    _state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<AppProxyConfig, String> {
    proxy_bridge::get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 更新指定应用的代理配置
///
/// 更新应用级配置（enabled、auto_failover、超时、熔断器等）
#[tauri::command]
pub async fn update_proxy_config_for_app(
    _state: tauri::State<'_, AppState>,
    config: AppProxyConfig,
) -> Result<(), String> {
    proxy_bridge::update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())
}

async fn get_default_cost_multiplier_internal(
    _state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    proxy_bridge::get_default_cost_multiplier(app_type).await
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
    _state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    proxy_bridge::set_default_cost_multiplier(app_type, value)
        .await
        .map_err(|err| match err {
            AppError::Message(message) if message.contains("Invalid multiplier:") => {
                AppError::localized(
                    "error.invalidMultiplier",
                    format!("无效倍率: {value}"),
                    format!("Invalid multiplier: {value}"),
                )
            }
            AppError::Message(message) if message.contains("Multiplier cannot be empty") => {
                AppError::localized(
                    "error.multiplierEmpty",
                    "倍率不能为空",
                    "Multiplier cannot be empty",
                )
            }
            other => other,
        })
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
    _state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    proxy_bridge::get_pricing_model_source(app_type).await
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
    _state: &AppState,
    app_type: &str,
    value: &str,
) -> Result<(), AppError> {
    proxy_bridge::set_pricing_model_source(app_type, value)
        .await
        .map_err(|err| match err {
            AppError::Message(message) if message.contains("Invalid pricing mode:") => {
                AppError::localized(
                    "error.invalidPricingMode",
                    format!("无效计费模式: {value}"),
                    format!("Invalid pricing mode: {value}"),
                )
            }
            other => other,
        })
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

/// 检查代理服务器是否正在运行
#[tauri::command]
pub async fn is_proxy_running(_state: tauri::State<'_, AppState>) -> Result<bool, String> {
    proxy_bridge::is_proxy_running()
        .await
        .map_err(|e| e.to_string())
}

/// 检查是否处于 Live 接管模式
#[tauri::command]
pub async fn is_live_takeover_active(_state: tauri::State<'_, AppState>) -> Result<bool, String> {
    proxy_bridge::is_live_takeover_active()
        .await
        .map_err(|e| e.to_string())
}

/// 代理模式下切换供应商（热切换）
#[tauri::command]
pub async fn switch_proxy_provider(
    _state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    proxy_bridge::switch_proxy_provider(&app_type, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== 故障转移相关命令 ====================

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    _state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    proxy_bridge::get_provider_health(&provider_id, &app_type)
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
    if let Some(switched_provider_id) =
        proxy_bridge::reset_circuit_breaker(&provider_id, &app_type)
            .await
            .map_err(|e| e.to_string())?
    {
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

    Ok(())
}

/// 获取熔断器配置
#[tauri::command]
pub async fn get_circuit_breaker_config(
    _state: tauri::State<'_, AppState>,
) -> Result<CircuitBreakerConfig, String> {
    proxy_bridge::get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新熔断器配置
#[tauri::command]
pub async fn update_circuit_breaker_config(
    _state: tauri::State<'_, AppState>,
    config: CircuitBreakerConfig,
) -> Result<(), String> {
    proxy_bridge::update_circuit_breaker_config(config)
        .await
        .map_err(|e| e.to_string())
}

/// 获取熔断器统计信息（仅当代理服务器运行时）
#[tauri::command]
pub async fn get_circuit_breaker_stats(
    _state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<Option<CircuitBreakerStats>, String> {
    proxy_bridge::get_circuit_breaker_stats(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())
}
