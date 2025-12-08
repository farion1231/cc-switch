//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::provider::Provider;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;

/// 启动代理服务器
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await
}

/// 停止代理服务器
#[tauri::command]
pub async fn stop_proxy_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.proxy_service.stop().await
}

/// 获取代理服务器状态
#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    state.proxy_service.get_status().await
}

/// 获取代理配置
#[tauri::command]
pub async fn get_proxy_config(state: tauri::State<'_, AppState>) -> Result<ProxyConfig, String> {
    state.proxy_service.get_config().await
}

/// 更新代理配置
#[tauri::command]
pub async fn update_proxy_config(
    state: tauri::State<'_, AppState>,
    config: ProxyConfig,
) -> Result<(), String> {
    state.proxy_service.update_config(&config).await
}

/// 检查代理服务器是否正在运行
#[tauri::command]
pub async fn is_proxy_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(state.proxy_service.is_running().await)
}

// ==================== 故障转移相关命令 ====================

/// 获取代理目标列表
#[tauri::command]
pub async fn get_proxy_targets(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    let db = &state.db;
    db.get_proxy_targets(&app_type)
        .await
        .map_err(|e| e.to_string())
        .map(|providers| providers.into_values().collect())
}

/// 设置代理目标
#[tauri::command]
pub async fn set_proxy_target(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    let db = &state.db;

    // 设置代理目标状态
    db.set_proxy_target(&provider_id, &app_type, enabled)
        .await
        .map_err(|e| e.to_string())?;

    // 如果是禁用代理目标，重置健康状态
    if !enabled {
        log::info!(
            "Resetting health status for provider {provider_id} (app: {app_type}) after disabling proxy target"
        );
        if let Err(e) = db.reset_provider_health(&provider_id, &app_type).await {
            log::warn!("Failed to reset provider health: {e}");
        }
    }

    Ok(())
}

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    let db = &state.db;
    db.get_provider_health(&provider_id, &app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 重置熔断器
#[tauri::command]
pub async fn reset_circuit_breaker(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<(), String> {
    // 重置数据库健康状态
    let db = &state.db;
    db.update_provider_health(&provider_id, &app_type, true, None)
        .await
        .map_err(|e| e.to_string())?;

    // 注意：熔断器状态在内存中，重启代理服务器后会重置
    // 如果代理服务器正在运行，需要通知它重置熔断器
    // 目前先通过数据库重置健康状态，熔断器会在下次超时后自动尝试半开

    Ok(())
}

/// 获取熔断器配置
#[tauri::command]
pub async fn get_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
) -> Result<CircuitBreakerConfig, String> {
    let db = &state.db;
    db.get_circuit_breaker_config()
        .await
        .map_err(|e| e.to_string())
}

/// 更新熔断器配置
#[tauri::command]
pub async fn update_circuit_breaker_config(
    state: tauri::State<'_, AppState>,
    config: CircuitBreakerConfig,
) -> Result<(), String> {
    let db = &state.db;
    db.update_circuit_breaker_config(&config)
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
    // 这个功能需要访问运行中的代理服务器的内存状态
    // 目前先返回 None，后续可以通过 ProxyService 暴露接口来实现
    let _ = (state, provider_id, app_type);
    Ok(None)
}
