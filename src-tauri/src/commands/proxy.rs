//! 代理服务相关的 Tauri 命令
//!
//! 提供前端调用的 API 接口

use crate::app_config::AppType;
use crate::error::AppError;
use crate::proxy::types::*;
use crate::proxy::CircuitBreakerConfig;
use crate::store::AppState;
use std::str::FromStr;

/// 有 `proxy_config` 行的应用：重试、熔断、故障转移这些设置只对它们有意义。
fn require_proxy_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.spec().local_proxy {
        return Err(format!("{} 不支持本地路由", app.as_str()));
    }
    Ok(app)
}

/// 有直连 / 路由模式的应用（含 Claude Desktop）：进出路由、换路由、看模式。
fn require_mode_app(app_type: &str) -> Result<crate::app_config::AppType, String> {
    let app = crate::app_config::AppType::from_str(app_type)
        .map_err(|error| format!("无效的应用类型: {error}"))?;
    if !app.spec().route_mode {
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
pub async fn get_app_mode(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<crate::mode::controller::AppModeView, String> {
    let app = require_mode_app(&app_type)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::mode::controller::app_mode_view(&state, &app)
    })
    .await
    .map_err(|e| format!("读取应用模式任务失败: {e}"))?
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

#[tauri::command]
pub async fn get_proxy_status(state: tauri::State<'_, AppState>) -> Result<ProxyStatus, String> {
    state.proxy_service.get_status().await
}

// ==================== Global & Per-App Config ====================

/// 获取全局代理配置
///
/// 返回统一的全局配置字段（代理开关、监听地址、端口、日志开关）
#[tauri::command]
pub async fn get_global_proxy_config(
    state: tauri::State<'_, AppState>,
) -> Result<GlobalProxyConfig, AppError> {
    let db = &state.db;
    db.get_global_proxy_config().await
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
    crate::settings::set_proxy_client_host(host)?;
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
    let app = require_proxy_app(&app_type)?;
    let db = &state.db;
    Ok(db.get_proxy_config_for_app(&app).await?)
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
    let app = require_proxy_app(&config.app_type)?;
    let circuit_config = CircuitBreakerConfig::from(&config);
    let guard = crate::mode::controller::lock_settled(state.inner(), &app)
        .await
        .map_err(|e| e.to_string())?;
    // `enabled` 是模式的镜像，只由进入 / 退出代理改写。
    let mut config = config;
    config.enabled = crate::mode::current::is_proxy(&app);

    db.update_proxy_config_for_app(config).await?;
    drop(guard);

    state
        .proxy_service
        .update_circuit_breaker_config_for_app(&app, circuit_config)
        .await
}

async fn get_pricing_model_source_internal(
    state: &AppState,
    app_type: &AppType,
) -> Result<String, AppError> {
    let db = &state.db;
    db.get_pricing_model_source(app_type).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn get_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &AppType,
) -> Result<String, AppError> {
    get_pricing_model_source_internal(state, app_type).await
}

#[tauri::command]
pub async fn get_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<String, AppError> {
    get_pricing_model_source_internal(&state, &AppType::from_str(&app_type)?).await
}

async fn set_pricing_model_source_internal(
    state: &AppState,
    app_type: &AppType,
    value: &str,
) -> Result<(), AppError> {
    let db = &state.db;
    db.set_pricing_model_source(app_type, value).await
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn set_pricing_model_source_test_hook(
    state: &AppState,
    app_type: &AppType,
    value: &str,
) -> Result<(), AppError> {
    set_pricing_model_source_internal(state, app_type, value).await
}

#[tauri::command]
pub async fn set_pricing_model_source(
    state: tauri::State<'_, AppState>,
    app_type: String,
    value: String,
) -> Result<(), AppError> {
    set_pricing_model_source_internal(&state, &AppType::from_str(&app_type)?, &value).await
}

// ==================== 故障转移相关命令 ====================

#[tauri::command]
pub async fn get_provider_health(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<ProviderHealth, String> {
    let app = require_mode_app(&app_type)?;
    let db = &state.db;
    Ok(db.get_provider_health(&provider_id, &app).await?)
}
