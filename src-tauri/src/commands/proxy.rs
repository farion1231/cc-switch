//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use std::str::FromStr;

fn require_proxy_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.supports_local_proxy() {
        return Err(format!("{} 不支持本地路由", app.as_str()));
    }
    Ok(app)
}

/// 启动代理服务器（仅启动服务，不接管 Live 配置）
#[tauri::command]
pub async fn start_proxy_server(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyServerInfo, String> {
    state.proxy_service.start().await
}

/// 停止代理服务器（仅停止服务，不恢复/清理 Live 接管状态）
#[tauri::command]
pub async fn stop_proxy_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let takeover = state.proxy_service.get_takeover_status().await?;
    if takeover.claude
        || takeover.codex
        || takeover.gemini
        || takeover.grokbuild
        || takeover.opencode
        || takeover.openclaw
        || takeover.cursor
    {
        return Err(
            "仍有应用处于代理接管状态，请先在设置中关闭对应应用接管后再停止本地路由。".to_string(),
        );
    }

    state.proxy_service.stop().await
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
    require_proxy_app(&app_type)?;
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
    let app_type = config.app_type.clone();
    require_proxy_app(&app_type)?;
    let circuit_config = CircuitBreakerConfig::from(&config);

    db.update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    state
        .proxy_service
        .update_circuit_breaker_config_for_app(&app_type, circuit_config)
        .await
}

async fn get_default_cost_multiplier_internal(
    state: &AppState,
    app_type: &str,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_default_cost_multiplier(app_type).await
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
    let db = &state.db;
    db.set_default_cost_multiplier(app_type, value).await
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
    let db = &state.db;
    db.get_pricing_model_source(app_type).await
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
    let db = &state.db;
    db.set_pricing_model_source(app_type, value).await
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
    let app = require_proxy_app(&app_type)?;
    // Codex official account cards can use the client's native OpenAI login
    // through takeover. Other apps' official providers remain blocked.
    let provider = state
        .db
        .get_provider_by_id(&provider_id, &app_type)
        .map_err(|e| format!("读取供应商失败: {e}"))?
        .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    if provider.category.as_deref() == Some("official")
        && !crate::services::provider::official_provider_supports_proxy_takeover(&app, &provider)
    {
        return Err(
            "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                .to_string(),
        );
    }

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
    require_proxy_app(&app_type)?;
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
    require_proxy_app(&app_type)?;
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
    require_proxy_app(&app_type)?;
    // 这个功能需要访问运行中的代理服务器的内存状态
    // 目前先返回 None，后续可以通过 ProxyService 暴露接口来实现
    let _ = (state, provider_id, app_type);
    Ok(None)
}

// ==================== 公网路由（Cursor 经公网隧道接入）（第三方供应商经公网隧道接入 Cursor）====================

/// 公网路由（Cursor 经公网隧道接入）状态（隧道 + 配置）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRouteStatus {
    pub enabled: bool,
    /// 公网路由隧道鉴权密钥（ccsk-*），需填到 Cursor 的 OpenAI API Key 输入框
    pub api_key: Option<String>,
    pub tunnel_running: bool,
    pub public_url: Option<String>,
    pub local_url: Option<String>,
    pub tunnel_error: Option<String>,
    /// 当前 Cursor 供应商名（供 UI 展示要添加的模型）
    pub current_provider_name: Option<String>,
    /// 隧道模式："quick"（默认）或 "named"
    pub tunnel_mode: String,
    /// 命名隧道名（mode = named 时）
    pub named_tunnel: Option<String>,
    /// 命名隧道域名（mode = named 时）
    pub named_hostname: Option<String>,
}

/// 查询 公网路由（Cursor 经公网隧道接入）状态
#[tauri::command]
pub async fn get_public_route_status(
    state: tauri::State<'_, AppState>,
) -> Result<PublicRouteStatus, String> {
    let mut settings = crate::settings::get_settings();

    // 惰性生成密钥：历史版本可能没有，保证启用状态下总有密钥可展示/校验
    if settings.public_route_enabled
        && settings
            .public_route_api_key
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
    {
        settings.public_route_api_key = Some(crate::settings::generate_public_route_api_key());
        if let Err(e) = crate::settings::update_settings(settings.clone()) {
            log::warn!("[Tunnel] 生成 公网路由密钥后保存失败: {e}");
        }
    }

    let tunnel = state.proxy_service.public_route_tunnel_status().await;

    let current_provider_name =
        crate::settings::get_current_provider(&crate::app_config::AppType::Cursor).and_then(|id| {
            state
                .db
                .get_provider_by_id(&id, "cursor")
                .ok()
                .flatten()
                .map(|p| p.name)
        });

    Ok(PublicRouteStatus {
        enabled: settings.public_route_enabled,
        api_key: settings.public_route_api_key.clone(),
        tunnel_running: tunnel.running,
        public_url: tunnel.public_url,
        local_url: tunnel.local_url,
        tunnel_error: tunnel.last_error,
        current_provider_name,
        tunnel_mode: settings
            .public_route_tunnel_mode
            .clone()
            .unwrap_or_else(|| "quick".to_string()),
        named_tunnel: settings.public_route_named_tunnel.clone(),
        named_hostname: settings.public_route_named_hostname.clone(),
    })
}

/// 列出 Cloudflare 账户下已有的命名隧道（供 公网路由 设置选择）。
/// 未登录（缺 cert.pem）时返回带引导文案的错误。
#[tauri::command]
pub async fn list_named_tunnels(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::proxy::cloudflared::NamedTunnel>, String> {
    state.proxy_service.list_named_tunnels().await
}

/// 保存 公网路由隧道配置（模式 + 命名隧道参数）。
/// 公网路由开启中时，保存后立即按新配置重建隧道。
#[tauri::command]
pub async fn set_public_route_tunnel_config(
    state: tauri::State<'_, AppState>,
    mode: String,
    named_tunnel: Option<String>,
    named_hostname: Option<String>,
) -> Result<PublicRouteStatus, String> {
    if mode != "quick" && mode != "named" {
        return Err(format!("未知隧道模式: {mode}"));
    }
    if mode == "named" {
        let ok = named_tunnel
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
            && named_hostname
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| !s.is_empty());
        if !ok {
            return Err("命名隧道模式需要填写隧道名和域名".to_string());
        }
    }

    let mut settings = crate::settings::get_settings();
    settings.public_route_tunnel_mode = Some(mode);
    settings.public_route_named_tunnel = named_tunnel;
    settings.public_route_named_hostname = named_hostname;
    crate::settings::update_settings(settings.clone()).map_err(|e| e.to_string())?;

    // 托管开启中：按新配置重建隧道（先停后起）
    if settings.public_route_enabled {
        let _ = state.proxy_service.stop_public_route_tunnel().await;
        state
            .proxy_service
            .start_public_route_tunnel()
            .await
            .map_err(|e| format!("按新配置重建隧道失败: {e}"))?;
    }

    get_public_route_status(state).await
}

/// 启用 公网路由（Cursor 经公网隧道接入）：保存配置并启动 cloudflared 隧道。
/// 要求本地代理已在运行且接管了 Cursor。
#[tauri::command]
pub async fn enable_public_route(
    state: tauri::State<'_, AppState>,
) -> Result<PublicRouteStatus, String> {
    // 本地代理必须在运行
    if !state.proxy_service.is_running().await {
        return Err("请先在「本地路由」中启动代理服务器".to_string());
    }

    // 持久化配置（密钥没有则生成——隧道暴露公网，/cursor/v1/* 强制 Bearer 校验）
    let mut settings = crate::settings::get_settings();
    settings.public_route_enabled = true;
    if settings
        .public_route_api_key
        .as_deref()
        .map(str::is_empty)
        .unwrap_or(true)
    {
        settings.public_route_api_key = Some(crate::settings::generate_public_route_api_key());
    }
    crate::settings::update_settings(settings).map_err(|e| e.to_string())?;

    // 启动隧道
    state
        .proxy_service
        .start_public_route_tunnel()
        .await
        .map_err(|e| format!("启动隧道失败: {e}"))?;

    get_public_route_status(state).await
}

/// 禁用 公网路由（Cursor 经公网隧道接入）：停止隧道并清除开关。
#[tauri::command]
pub async fn disable_public_route(
    state: tauri::State<'_, AppState>,
) -> Result<PublicRouteStatus, String> {
    let _ = state.proxy_service.stop_public_route_tunnel().await;

    let mut settings = crate::settings::get_settings();
    settings.public_route_enabled = false;
    crate::settings::update_settings(settings).map_err(|e| e.to_string())?;

    get_public_route_status(state).await
}

// ==================== 公网路由密钥重新生成（TDD 测试） ====================

/// 重新生成 公网路由 隧道鉴权密钥并持久化，返回新密钥。
/// Cursor 的 key 走云端账户同步、本地写无效，重新生成后需用户把新密钥
/// 手工重新粘贴进 Cursor（#review ccsk 手工重新生成）。
fn regenerate_public_route_key_in_settings() -> Result<String, String> {
    let mut settings = crate::settings::get_settings();
    let new_key = crate::settings::generate_public_route_api_key();
    settings.public_route_api_key = Some(new_key.clone());
    crate::settings::update_settings(settings).map_err(|e| e.to_string())?;
    Ok(new_key)
}

/// 重新生成 公网路由 隧道鉴权密钥（ccsk-*）。返回最新状态供前端展示/复制新密钥。
#[tauri::command]
pub async fn regenerate_public_route_api_key(
    state: tauri::State<'_, AppState>,
) -> Result<PublicRouteStatus, String> {
    regenerate_public_route_key_in_settings()?;
    get_public_route_status(state).await
}

#[cfg(test)]
mod public_route_regenerate_tests {
    use super::regenerate_public_route_key_in_settings;
    use std::env;

    fn with_temp_home<T>(test: impl FnOnce() -> T) -> T {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_home = env::var_os("HOME");
        let old_test = env::var_os("CC_SWITCH_TEST_HOME");
        env::set_var("HOME", temp.path());
        env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::reload_settings().expect("reload settings");
        let result = test();
        match old_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
        match old_test {
            Some(v) => env::set_var("CC_SWITCH_TEST_HOME", v),
            None => env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        result
    }

    // with_temp_home 改写了进程级 HOME/CC_SWITCH_TEST_HOME，须与其他 env 测试互斥
    #[serial_test::serial]
    #[test]
    fn regenerate_changes_and_persists_key() {
        with_temp_home(|| {
            crate::settings::update_settings(crate::settings::AppSettings {
                public_route_enabled: true,
                public_route_api_key: Some("ccsk-old-key".to_string()),
                ..Default::default()
            })
            .expect("seed old key");

            let new = regenerate_public_route_key_in_settings().expect("regenerate");
            assert!(
                new.starts_with("ccsk-"),
                "key must keep ccsk- prefix, got {new}"
            );
            assert_ne!(new, "ccsk-old-key", "regenerated key must differ from old");
            assert_eq!(
                crate::settings::get_settings()
                    .public_route_api_key
                    .as_deref(),
                Some(new.as_str()),
                "new key must be persisted"
            );
        });
    }
}
