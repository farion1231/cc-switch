//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::{CircuitBreakerConfig, CircuitBreakerStats};
use crate::store::AppState;
use std::str::FromStr;

/// 有 `proxy_config` 行的应用：重试、熔断、故障转移这些设置只对它们有意义。
fn require_proxy_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.supports_local_proxy() {
        return Err(format!("{} 不支持本地路由", app.as_str()));
    }
    Ok(app)
}

/// 有直连 / 路由模式的应用（含 Claude Desktop）：进出路由、换路由、看模式。
fn require_mode_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.has_route_mode() {
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
        || takeover.claude_desktop
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

/// 关闭本地路由：所有应用退回直连，再停止代理服务器
#[tauri::command]
pub async fn stop_proxy_with_restore(state: tauri::State<'_, AppState>) -> Result<(), String> {
    crate::mode::controller::exit_all(state.inner()).await
}

/// 获取各应用接管状态
#[tauri::command]
pub async fn get_proxy_takeover_status(
    state: tauri::State<'_, AppState>,
) -> Result<ProxyTakeoverStatus, String> {
    state.proxy_service.get_takeover_status().await
}

/// 为指定应用进入 / 退出代理模式。`stack` 为真时进入的是 Stack 模式（和路由模式二选一），
/// `route` 是确认框里选的路由目标（Stack 模式下是默认那家，不传沿用上次的路由）；退出时
/// 两者都不看。
#[tauri::command]
pub async fn set_proxy_takeover_for_app(
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
    stack: Option<bool>,
    route: Option<String>,
) -> Result<(), String> {
    let app = require_mode_app(&app_type)?;
    if enabled {
        crate::mode::controller::enter_with_route(
            state.inner(),
            &app,
            stack.unwrap_or(false),
            route.as_deref(),
        )
        .await
    } else {
        crate::mode::controller::exit(state.inner(), &app).await
    }
}

/// 应用页模式行用：生效的模式、路由目标（直连时是上次路由的那家）、直连那家。
#[tauri::command]
pub fn get_app_mode(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<crate::mode::controller::AppModeView, String> {
    let app = require_mode_app(&app_type)?;
    crate::mode::controller::app_mode_view(state.inner(), &app)
}

/// 指定路由目标（聚合模式下是默认那家）。直连模式下只记下来，下次进入路由 / 聚合模式时用它；
/// 已经在路由 / 聚合模式时当场生效。
#[tauri::command]
pub async fn set_proxy_route(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<crate::mode::controller::RouteSwitchResult, String> {
    let app = require_mode_app(&app_type)?;
    let result = crate::mode::controller::set_route(state.inner(), &app, &provider_id).await;
    // 已经在路由 / 聚合模式时换的是正在用的那家，托盘跟着变
    crate::tray::refresh_tray_menu(&app_handle);
    result
}

/// 启动时没能接上代理、已退回直连的应用（取一次就清空）。
#[tauri::command]
pub fn take_startup_attach_failures() -> Vec<crate::mode::controller::StartupAttachFailure> {
    crate::mode::controller::take_startup_attach_failures()
}

/// 设置里在路由和 Stack 之间换的时候：处于另一种模式（`stack` 为真是 Stack 模式）的
/// Claude Code、Codex 先退回直连。返回退回直连的应用。
#[tauri::command]
pub async fn exit_proxy_apps_in_mode(
    state: tauri::State<'_, AppState>,
    stack: bool,
) -> Result<Vec<String>, String> {
    crate::mode::controller::exit_apps_in_mode(state.inner(), stack).await
}

/// 直连指针：代理模式下退出代理时写回的供应商
#[tauri::command]
pub fn get_direct_provider(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Option<String>, String> {
    let app = require_mode_app(&app_type)?;
    crate::mode::controller::direct_provider_id(state.inner(), &app).map_err(|e| e.to_string())
}

/// Stack 模型：名单里的每一家和它发布的模型 id，以及还在用旧模型列表的 Codex 客户端
#[tauri::command]
pub async fn get_proxy_stack(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<crate::mode::stack::StackView, String> {
    let app = require_mode_app(&app_type)?;
    crate::mode::controller::stack_view_with_clients(state.inner(), &app).await
}

/// 重启 Codex 的托管守护进程（`codex` TUI 连的那个），让它重读模型目录。会中断守护进程里
/// 正在运行的任务，只在用户确认之后调。
#[tauri::command]
pub async fn restart_codex_app_server_daemon(
) -> Result<crate::services::provider::codex_client_catalog::RestartOutcome, String> {
    crate::services::provider::codex_direct::off_runtime(
        crate::services::provider::codex_client_catalog::restart_daemon,
    )
    .await
    .map_err(|error| error.to_string())?
}

/// 用户关掉「Codex 可能还在用旧的」提示（看不到进程时出的那种）：记下现在的模型目录和文件
/// 登录，同一份不再提示。
#[tauri::command]
pub async fn acknowledge_codex_stale_clients() -> Result<(), String> {
    crate::services::provider::codex_direct::off_runtime(|| {
        crate::services::provider::codex_client_catalog::acknowledge(
            &crate::live::engine::DeviceStore::for_device(),
        )
    })
    .await
    .map_err(|error| error.to_string())
}

/// Stack 模型：把一家加入或移出名单（`enabled` 是目标值）。成功时返回客户端看不到或看不全
/// Stack 模型的提示；失败时 `partial` 为真表示已部分写入，下次操作或重启 CC Switch 时补完。
#[tauri::command]
pub async fn set_proxy_stack_member(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
    enabled: bool,
) -> Result<Option<&'static str>, crate::mode::controller::StackWriteError> {
    let app =
        require_mode_app(&app_type).map_err(crate::mode::controller::StackWriteError::unchanged)?;
    let result =
        crate::mode::controller::set_stack_member(state.inner(), &app, &provider_id, enabled).await;
    // 托盘的聚合子菜单只列名单成员；部分写入也已经改了名单，一律重建。
    // 不重建的话，托盘里还列着刚移出的那家，点一下会把它重新加回并设为默认
    crate::tray::refresh_tray_menu(&app_handle);
    result
}

/// Codex 聚合的模型被别的模型目录挡住时，改用 CC Switch 生成的目录（去掉指向别的文件的
/// `model_catalog_json`）。返回之后还剩的提示。
#[tauri::command]
pub async fn adopt_codex_stack_catalog(
    state: tauri::State<'_, AppState>,
) -> Result<Option<&'static str>, String> {
    crate::mode::controller::adopt_codex_stack_catalog(state.inner()).await
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
    if state.proxy_service.update_config(&config).await? {
        // 代理换了地址：按新地址重写接上代理的客户端。
        crate::mode::controller::resync_routes(state.inner()).await?;
    }
    Ok(())
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
/// 更新统一的全局配置字段，四行镜像写，各应用自己的重试和超时不碰。设置页的按钮写着
/// 「保存并重启服务」：服务在跑时地址或端口变了就重启、再按新地址重写接上路由的客户端
/// （含 Claude Desktop 的模型映射卡），日志开关实时生效；只写库的话服务还在旧端口上听、
/// 客户端也还指着旧端口。
#[tauri::command]
pub async fn update_global_proxy_config(
    state: tauri::State<'_, AppState>,
    config: GlobalProxyConfig,
) -> Result<(), String> {
    let restarted = state.proxy_service.update_global_config(&config).await?;
    if restarted {
        resync_proxy_clients(state.inner()).await?;
    }
    Ok(())
}

/// 获取客户端地址：写进客户端配置的代理主机，空表示按监听地址推。
#[tauri::command]
pub async fn get_proxy_client_host() -> Result<String, String> {
    Ok(crate::settings::get_proxy_client_host().unwrap_or_default())
}

/// 保存客户端地址，再按新地址重写接上路由的客户端，不用重启服务。存在本机的
/// settings.json 里：这个地址只对这台机器有意义，不能随数据库同步到别的设备。
/// 每次保存都重写一遍：上次重写失败、改好客户端文件后原样再保存就是重试；
/// 客户端已经是这个地址时重写不碰文件。
#[tauri::command]
pub async fn set_proxy_client_host(
    state: tauri::State<'_, AppState>,
    host: String,
) -> Result<(), String> {
    save_proxy_client_host(state.inner(), &host).await
}

pub(crate) async fn save_proxy_client_host(state: &AppState, host: &str) -> Result<(), String> {
    crate::settings::set_proxy_client_host(host).map_err(|e| e.to_string())?;
    resync_proxy_clients(state).await
}

/// 按当前的代理地址重写接上路由的客户端（含 Claude Desktop 的路由模式）。
async fn resync_proxy_clients(state: &AppState) -> Result<(), String> {
    crate::mode::controller::resync_routes(state).await
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
    let app = require_proxy_app(&app_type)?;
    let circuit_config = CircuitBreakerConfig::from(&config);
    // `enabled` 是模式的镜像，只由进入 / 退出代理改写。
    let mut config = config;
    config.enabled = crate::mode::current::is_proxy(&app);

    db.update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    state
        .proxy_service
        .update_circuit_breaker_config_for_app(&app_type, circuit_config)
        .await
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
    let app = require_mode_app(&app_type)?;
    crate::mode::controller::switch_route(state.inner(), &app, &provider_id).await
}

// ==================== 故障转移相关命令 ====================

/// 获取供应商健康状态
#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    require_mode_app(&app_type)?;
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
    let app = require_proxy_app(&app_type)?;
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

    // 3. 检查是否应该切回优先级更高的供应商
    // 只有当该应用处于代理模式且开启了自动故障转移时才执行
    let app_in_proxy = crate::mode::current::is_proxy(&app);
    let auto_failover_enabled = match db.get_proxy_config_for_app(&app_type).await {
        Ok(config) => config.auto_failover_enabled,
        Err(e) => {
            log::error!("[{app_type}] Failed to read proxy_config: {e}, defaulting to disabled");
            false
        }
    };

    if app_in_proxy && auto_failover_enabled && state.proxy_service.is_running().await {
        // 代理当前路由到的供应商
        let current_id =
            crate::mode::current::provider_for(db, &app, crate::mode::current::Purpose::InUse)
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
                        crate::proxy::failover_switch::FailoverSwitchManager::new();
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
    require_mode_app(&app_type)?;
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

    let current_provider_name = crate::mode::current::provider_for(
        &state.db,
        &crate::app_config::AppType::Cursor,
        crate::mode::current::Purpose::InUse,
    )
    .ok()
    .flatten()
    .and_then(|id| {
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

// ==================== Cursor 接入引导：运行检测 / BYOK / 连通性 ====================

/// Cursor 是否正在运行（运行中会把 BYOK 设置回写覆盖，写前必须先完全退出）。
#[tauri::command]
pub async fn cursor_is_running() -> Result<bool, String> {
    Ok(crate::cursor_config::is_cursor_running())
}

/// 读 Cursor 本地 BYOK 现状（Base URL / 是否用自定义 Key / 可用模型数）。
#[tauri::command]
pub async fn cursor_byok_status() -> Result<crate::cursor_config::CursorByokState, String> {
    Ok(crate::cursor_config::read_byok_state())
}

/// 写 Cursor BYOK（Base URL + 可用模型列表）。密钥是 safeStorage 加密的，读不出也写不进。
/// Cursor 运行中直接报错，不写。
#[tauri::command]
pub async fn cursor_write_byok(
    base_url: String,
    model_ids: Vec<String>,
) -> Result<crate::cursor_config::CursorWriteByokResult, String> {
    crate::cursor_config::write_byok(&base_url, &model_ids).map_err(|e| e.to_string())
}

/// 最新的 BYOK 回滚点（没有则 `null`）。界面据此决定退出接管后是否提示「一键还原」。
#[tauri::command]
pub async fn cursor_byok_backup() -> Result<Option<crate::cursor_config::ByokBackup>, String> {
    Ok(crate::cursor_config::latest_byok_backup())
}

/// 还原 Cursor BYOK 到最新回滚点（写入前的原始行）。
///
/// **只提示、不代劳**：由用户在退出接管后显式点击触发；Cursor 运行中直接报错，不写。
#[tauri::command]
pub async fn cursor_restore_byok() -> Result<crate::cursor_config::CursorRestoreByokResult, String>
{
    crate::cursor_config::restore_byok().map_err(|e| e.to_string())
}

/// 连通性探测结果分档（纯函数分类，便于单测；测试不发真网络请求）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectivityOutcome {
    /// 该层未启动：`"本地代理"` 或 `"公网隧道"`。
    LayerDown(&'static str),
    /// DNS 解析失败。
    DnsFailure,
    /// 连接超时。
    Timeout,
    /// 其它网络错误（连接被拒等），带原始信息。
    Network(String),
    /// 收到 HTTP 响应（状态码）。
    Http(u16),
}

/// 连通性探测结果（`test_public_route_connectivity` 的载荷）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRouteConnectivityResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub detail: String,
}

/// 纯函数：把探测结果分成可读文案（各档文案各不相同）。
pub(crate) fn classify_connectivity(outcome: ConnectivityOutcome) -> PublicRouteConnectivityResult {
    match outcome {
        ConnectivityOutcome::LayerDown(layer) => PublicRouteConnectivityResult {
            ok: false,
            status: None,
            detail: format!("{layer}未启动，请先在「设置 → 路由」中启动后再测试"),
        },
        ConnectivityOutcome::DnsFailure => PublicRouteConnectivityResult {
            ok: false,
            status: None,
            detail: "DNS 解析失败：公网地址无效或本机 DNS 不可用".to_string(),
        },
        ConnectivityOutcome::Timeout => PublicRouteConnectivityResult {
            ok: false,
            status: None,
            detail: "连接超时：网络不通或对端无响应".to_string(),
        },
        ConnectivityOutcome::Network(message) => PublicRouteConnectivityResult {
            ok: false,
            status: None,
            detail: format!("连接失败：{message}"),
        },
        ConnectivityOutcome::Http(401) => PublicRouteConnectivityResult {
            ok: false,
            status: Some(401),
            detail: "鉴权失败（401）：公网路由密钥不正确，请重新复制密钥并粘贴到 Cursor"
                .to_string(),
        },
        ConnectivityOutcome::Http(status) if (200..300).contains(&status) => {
            PublicRouteConnectivityResult {
                ok: true,
                status: Some(status),
                detail: format!("连通正常（HTTP {status}）"),
            }
        }
        ConnectivityOutcome::Http(status) => PublicRouteConnectivityResult {
            ok: false,
            status: Some(status),
            detail: format!("对端返回 HTTP {status}（非 2xx）"),
        },
    }
}

/// 纯函数：从网络错误文本与「是否超时」判分档。
pub(crate) fn classify_network_error(description: &str, is_timeout: bool) -> ConnectivityOutcome {
    if is_timeout {
        return ConnectivityOutcome::Timeout;
    }
    let lower = description.to_ascii_lowercase();
    if lower.contains("dns")
        || lower.contains("failed to lookup address")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname")
    {
        ConnectivityOutcome::DnsFailure
    } else {
        ConnectivityOutcome::Network(description.to_string())
    }
}

/// 把错误链拼成一段文本（含所有 source），便于纯函数判分档。
fn error_chain_text(err: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(entry) = current {
        parts.push(entry.to_string());
        current = entry.source();
    }
    parts.join(": ")
}

/// 把 reqwest 错误映射成分档（超时优先，其次判 DNS）。
pub(crate) fn classify_reqwest_error(err: &reqwest::Error) -> ConnectivityOutcome {
    if err.is_timeout() {
        return ConnectivityOutcome::Timeout;
    }
    classify_network_error(&error_chain_text(err), false)
}

/// 分层测公网路由连通性：`local` 打本机代理，`public` 打公网隧道地址。
///
/// 只证明「这一层在服务 / 密钥有效 / `/cursor/v1` 已接管」，**不能证明外部网络可达**。
#[tauri::command]
pub async fn test_public_route_connectivity(
    state: tauri::State<'_, AppState>,
    scope: String,
) -> Result<PublicRouteConnectivityResult, String> {
    let (base, bearer, bypass_system_proxy) = match scope.as_str() {
        "local" => {
            if !state.proxy_service.is_running().await {
                return Ok(classify_connectivity(ConnectivityOutcome::LayerDown(
                    "本地代理",
                )));
            }
            let (proxy_url, _) = state.proxy_service.build_proxy_urls().await?;
            (proxy_url, None, true)
        }
        "public" => {
            let tunnel = state.proxy_service.public_route_tunnel_status().await;
            if !tunnel.running {
                return Ok(classify_connectivity(ConnectivityOutcome::LayerDown(
                    "公网隧道",
                )));
            }
            let Some(public_url) = tunnel.public_url else {
                return Ok(classify_connectivity(ConnectivityOutcome::LayerDown(
                    "公网隧道",
                )));
            };
            let key = crate::settings::get_settings().public_route_api_key;
            (public_url, key, false)
        }
        other => return Err(format!("未知探测范围: {other}")),
    };

    let url = format!("{}/cursor/v1/models", base.trim_end_matches('/'));
    Ok(probe_models_endpoint(&url, bearer.as_deref(), bypass_system_proxy).await)
}

/// 对 `GET {base}/cursor/v1/models` 发一次探测（8s 超时）。
///
/// `bypass_system_proxy` 用于本机地址：macOS 的 PAC/系统代理会把 `127.0.0.1` 也劫持出去，
/// 那样「本地探测」会假失败。
async fn probe_models_endpoint(
    url: &str,
    bearer: Option<&str>,
    bypass_system_proxy: bool,
) -> PublicRouteConnectivityResult {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8));
    if bypass_system_proxy {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(e) => {
            return classify_connectivity(ConnectivityOutcome::Network(e.to_string()));
        }
    };
    let mut request = client.get(url);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    match request.send().await {
        Ok(response) => {
            classify_connectivity(ConnectivityOutcome::Http(response.status().as_u16()))
        }
        Err(e) => classify_connectivity(classify_reqwest_error(&e)),
    }
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

#[cfg(test)]
mod connectivity_tests {
    use super::*;

    #[test]
    fn classify_2xx_is_ok() {
        let result = classify_connectivity(ConnectivityOutcome::Http(200));
        assert!(result.ok);
        assert_eq!(result.status, Some(200));
        assert!(result.detail.contains("200"));
    }

    #[test]
    fn classify_401_is_auth_failure() {
        let result = classify_connectivity(ConnectivityOutcome::Http(401));
        assert!(!result.ok);
        assert_eq!(result.status, Some(401));
        assert!(
            result.detail.contains("401") && result.detail.contains("密钥"),
            "401 须给「密钥不对」的可读文案，实际：{}",
            result.detail
        );
    }

    #[test]
    fn classify_other_non_2xx() {
        let result = classify_connectivity(ConnectivityOutcome::Http(502));
        assert!(!result.ok);
        assert_eq!(result.status, Some(502));
        assert!(result.detail.contains("502"));
    }

    #[test]
    fn classify_layer_down_names_the_layer() {
        let local = classify_connectivity(ConnectivityOutcome::LayerDown("本地代理"));
        assert!(!local.ok);
        assert_eq!(local.status, None);
        assert!(local.detail.contains("本地代理"));

        let public = classify_connectivity(ConnectivityOutcome::LayerDown("公网隧道"));
        assert!(!public.ok);
        assert_eq!(public.status, None);
        assert!(public.detail.contains("公网隧道"));
    }

    #[test]
    fn classify_network_error_buckets() {
        assert_eq!(
            classify_network_error(
                "error trying to connect: dns error: failed to lookup address information",
                false
            ),
            ConnectivityOutcome::DnsFailure
        );
        assert_eq!(
            classify_network_error("operation timed out", true),
            ConnectivityOutcome::Timeout
        );
        match classify_network_error(
            "error trying to connect: Connection refused (os error 61)",
            false,
        ) {
            ConnectivityOutcome::Network(text) => assert!(text.contains("Connection refused")),
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn every_bucket_has_distinct_readable_detail() {
        let details = [
            classify_connectivity(ConnectivityOutcome::LayerDown("公网隧道")).detail,
            classify_connectivity(ConnectivityOutcome::DnsFailure).detail,
            classify_connectivity(ConnectivityOutcome::Timeout).detail,
            classify_connectivity(ConnectivityOutcome::Network("boom".to_string())).detail,
            classify_connectivity(ConnectivityOutcome::Http(401)).detail,
            classify_connectivity(ConnectivityOutcome::Http(502)).detail,
            classify_connectivity(ConnectivityOutcome::Http(200)).detail,
        ];
        let unique: std::collections::HashSet<&String> = details.iter().collect();
        assert_eq!(
            unique.len(),
            details.len(),
            "各档文案必须各不相同：{details:?}"
        );
    }
}
