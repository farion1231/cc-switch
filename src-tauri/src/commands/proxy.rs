//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await
}

/// 停止代理服务器（恢复 Live 配置）
#[tauri::command]
pub async fn stop_proxy_with_restore(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.proxy_service.stop_with_restore().await
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    state.proxy_service.get_takeover_status().await
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

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, String> {
    let db = &state.db;
    db.get_global_proxy_config()
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
    let db = &state.db;
    db.update_global_proxy_config(config)
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
    let db = &state.db;
    db.get_proxy_config_for_app(&app_type)
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
    let db = &state.db;
    db.update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())
}

/// 检查代理服务器是否正在运行
#[tauri::command]
pub async fn is_proxy_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(state.proxy_service.is_running().await)
}

/// 检查是否处于 Live 接管模式
#[tauri::command]
pub async fn is_live_takeover_active(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state.proxy_service.is_takeover_active().await
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
}

// ==================== 故障转移相关命令 ====================

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
    // 1. 重置数据库健康状态
    let db = &state.db;
    db.update_provider_health(&provider_id, &app_type, true, None)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，重置内存中的熔断器状态
    state
        .proxy_service
        .reset_provider_circuit_breaker(&provider_id, &app_type)
        .await?;

    // 3. 检查是否应该切回优先级更高的供应商（从 proxy_config 表读取）
    // 只有当该应用已被代理接管（enabled=true）且开启了自动故障转移时才执行
    let (app_enabled, auto_failover_enabled) = match db.get_proxy_config_for_app(&app_type).await {
        Ok(config) => (config.enabled, config.auto_failover_enabled),
        Err(e) => {
            log::error!("[{app_type}] Failed to read proxy_config: {e}, defaulting to disabled");
            (false, false)
        }
    };

    if app_enabled && auto_failover_enabled && state.proxy_service.is_running().await {
        // 获取当前供应商 ID
        let current_id = db
            .get_current_provider(&app_type)
            .map_err(|e| e.to_string())?;

        if let Some(current_id) = current_id {
            // 获取故障转移队列
            let queue = db
                .get_failover_queue(&app_type)
                .map_err(|e| e.to_string())?;

            // 找到恢复的供应商和当前供应商在队列中的位置（使用 sort_index）
            let restored_order = queue
                .iter()
                .find(|item| item.provider_id == provider_id)
                .and_then(|item| item.sort_index);

            let current_order = queue
                .iter()
                .find(|item| item.provider_id == current_id)
                .and_then(|item| item.sort_index);

            // 如果恢复的供应商优先级更高（sort_index 更小），则切换
            if let (Some(restored), Some(current)) = (restored_order, current_order) {
                if restored < current {
                    log::info!(
                        "[Recovery] 供应商 {provider_id} 已恢复且优先级更高 (P{restored} vs P{current})，自动切换"
                    );

                    // 获取供应商名称用于日志和事件
                    let provider_name = db
                        .get_all_providers(&app_type)
                        .ok()
                        .and_then(|providers| providers.get(&provider_id).map(|p| p.name.clone()))
                        .unwrap_or_else(|| provider_id.clone());

                    // 创建故障转移切换管理器并执行切换
                    let switch_manager =
                        crate::proxy::failover_switch::FailoverSwitchManager::new(db.clone());
                    if let Err(e) = switch_manager
                        .try_switch(Some(&app_handle), &app_type, &provider_id, &provider_name)
                        .await
                    {
                        log::error!("[Recovery] 自动切换失败: {e}");
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

    // 1. 更新数据库配置
    db.update_circuit_breaker_config(&config)
        .await
        .map_err(|e| e.to_string())?;

    // 2. 如果代理正在运行，热更新内存中的熔断器配置
    state
        .proxy_service
        .update_circuit_breaker_configs(config)
        .await?;

    Ok(())
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
