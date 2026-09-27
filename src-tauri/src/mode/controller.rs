//! 双模式控制器：进入、退出、分离、接上代理，以及代理内换路由。
//!
//! 每个可代理的应用只有一个持久化的模式（`live-state.json`，设备本地）。所有操作都是
//! 「DB + 模式状态 → 客户端文件」的投影，可以重复执行，没有一个依赖快照：
//!
//! | 操作 | 客户端文件 | 模式状态 |
//! |---|---|---|
//! | 进入 | 写代理契约（路由供应商的） | proxy，接上，记下契约；路由没有值时用直连指针初始化 |
//! | 退出 | 写回直连指针的供应商 | direct；路由保留 |
//! | 分离（退出 CC Switch） | 同退出 | 模式、路由不变，未接上 |
//! | 接上（启动） | 按保存的路由写代理契约 | 接上 |
//! | 换路由 | 契约没变就不碰；变了先改写客户端 | 路由、契约 |
//!
//! Claude Code、Codex、Gemini CLI 的客户端文件经写入引擎写，文件和模式状态在同一个
//! 操作里提交，崩溃后按 pending 前滚。Grok Build 暂时沿用原有的整份写入函数（改成只写
//! 关键字段之前的过渡做法）：先写文件，成功后再落定状态；进入代理前先把 live 回填进
//! 直连供应商的行，和直连切走时一样。
//!
//! 调用方持有这个应用的代理切换锁（`ProxyService::lock_switch_for_app`）；写引擎的应用
//! 写锁在更里面拿，两把锁不反向嵌套。

use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::live::engine::{lock_app, DeviceStore, LiveFile};
use crate::live::patch::dotenv::DotenvPatch;
use crate::live::project::claude::{
    direct_patch, proxy_projection, ClaudeProjection, ProxyAuth, PROXY_TOKEN_PLACEHOLDER,
};
use crate::provider::Provider;
use crate::services::provider::codex_direct::{self, Owner};
use crate::services::provider::{claude_direct, ProviderService, SwitchResult};
use crate::services::McpService;
use crate::store::AppState;

use super::contract;
use super::current::{self, Purpose};
use super::operation::{self, FileChange};
use super::state::{op, Contract, Mode, ModeState, PendingTarget};

/// 支持代理模式的应用。
pub const PROXY_APPS: [AppType; 4] = [
    AppType::Claude,
    AppType::Codex,
    AppType::Gemini,
    AppType::GrokBuild,
];

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn provider(state: &AppState, app: &AppType, id: &str) -> Result<Option<Provider>, String> {
    state.db.get_provider_by_id(id, app.as_str()).map_err(err)
}

fn direct_provider(state: &AppState, app: &AppType) -> Result<Option<Provider>, String> {
    match current::provider_for(&state.db, app, Purpose::Direct).map_err(err)? {
        Some(id) => provider(state, app, &id),
        None => Ok(None),
    }
}

fn route_provider(
    state: &AppState,
    app: &AppType,
    mode: &ModeState,
) -> Result<Option<Provider>, String> {
    match mode.proxy_route.as_deref() {
        Some(id) => provider(state, app, id),
        None => Ok(None),
    }
}

/// 客户端文件现在对应的是谁。
enum LiveNow {
    /// 直连投影（或还没写过任何东西）。
    Direct(Option<Provider>),
    /// 代理契约；`route` 是写入契约时的路由供应商。
    Proxy {
        contract: Option<Contract>,
        route: Option<Provider>,
    },
}

impl LiveNow {
    fn of(state: &AppState, app: &AppType, mode: &ModeState) -> Result<Self, String> {
        if mode.attached {
            // 旧版接管留下的状态没有路由记录，那时路由就是直连指针。
            let route = match route_provider(state, app, mode)? {
                Some(route) => Some(route),
                None => direct_provider(state, app)?,
            };
            Ok(Self::Proxy {
                contract: mode.contract.clone(),
                route,
            })
        } else {
            Ok(Self::Direct(direct_provider(state, app)?))
        }
    }

    /// live 里现在由哪个供应商带进来的独有字段。
    fn claude_exclusive_owner(&self) -> Option<ClaudeProjection> {
        match self {
            Self::Direct(provider) => provider
                .as_ref()
                .map(|provider| ClaudeProjection::of(&provider.settings_config)),
            Self::Proxy {
                contract: Some(contract),
                ..
            } => Some(ClaudeProjection {
                exclusive: contract.exclusive.clone(),
                ..ClaudeProjection::default()
            }),
            Self::Proxy {
                contract: None,
                route,
            } => route
                .as_ref()
                .map(|provider| ClaudeProjection::of(&provider.settings_config)),
        }
    }

    /// Codex 的 live 现在是谁写进去的。
    fn codex_owner(&self) -> Owner<'_> {
        match self {
            Self::Direct(provider) => provider.as_ref().map_or(Owner::None, Owner::Provider),
            Self::Proxy {
                contract: Some(contract),
                route,
            } => Owner::Contract {
                contract,
                route: route.as_ref(),
            },
            Self::Proxy {
                contract: None,
                route,
            } => route.as_ref().map_or(Owner::None, Owner::Provider),
        }
    }
}

fn claude_proxy_auth(provider: &Provider) -> ProxyAuth {
    if provider.uses_managed_account_auth() {
        ProxyAuth::Managed {
            auth_token: !provider.is_github_copilot() || !provider.claude_uses_api_key_field(),
        }
    } else {
        ProxyAuth::FollowRow
    }
}

fn claude_contract(route: &Provider, proxy_url: &str) -> (ClaudeProjection, Contract) {
    let projection = proxy_projection(
        &ClaudeProjection::of(&route.settings_config),
        proxy_url,
        claude_proxy_auth(route),
    );
    let contract = contract::claude(&projection);
    (projection, contract)
}

fn gemini_env_file() -> LiveFile {
    LiveFile::private(crate::gemini_config::get_gemini_env_path())
}

/// 客户端文件不经引擎的应用（Codex、Grok Build）写完文件后落定状态。
fn commit_state(state: &AppState, app: &AppType, target: &PendingTarget) -> Result<(), String> {
    operation::commit_target(&state.db, &DeviceStore::for_device(), app.as_str(), target)
        .map_err(err)
}

/// 经引擎提交一次只有 `.env` 的 Gemini 操作；`patch` 为空时只落定状态。
fn run_gemini(
    state: &AppState,
    op: &str,
    patch: Option<&DotenvPatch>,
    target: PendingTarget,
) -> Result<(), AppError> {
    let app = AppType::Gemini.as_str();
    let guard = lock_app(app);
    let store = DeviceStore::for_device();
    let changes: Vec<FileChange<'_>> = patch
        .into_iter()
        .map(|patch| FileChange {
            file: gemini_env_file(),
            patch,
        })
        .collect();
    operation::run(&store, &guard, op, &changes, target, &|target| {
        operation::commit_target(&state.db, &store, app, target)
    })?;
    Ok(())
}

fn gemini_proxy_patch(proxy_url: &str) -> DotenvPatch {
    DotenvPatch {
        set: vec![
            ("GOOGLE_GEMINI_BASE_URL".to_string(), proxy_url.to_string()),
            (
                "GEMINI_API_KEY".to_string(),
                PROXY_TOKEN_PLACEHOLDER.to_string(),
            ),
        ],
        ..DotenvPatch::default()
    }
}

/// 进入代理前把 live 回填进直连供应商的行（Gemini、Grok Build 的过渡做法）。
/// live 里已经是代理占位符（旧版接管的遗留）时不回填，否则会把占位符存进行里。
fn backfill_direct(state: &AppState, app: &AppType, live_now: &LiveNow) {
    let LiveNow::Direct(Some(direct)) = live_now else {
        return;
    };
    let mut ignored = SwitchResult::default();
    ProviderService::backfill_current_from_live(state, app, direct, &mut ignored);
}

/// 写代理契约。`target` 的 `contract` 由这里填好。契约没变时不碰客户端文件；接上
/// （启动时）一律重写：顺带核对路由供应商还能用（比如托管账号还在），并修正 CC Switch
/// 没运行期间客户端文件里的漂移。
async fn write_proxy(
    state: &AppState,
    app: &AppType,
    op_name: &str,
    route: &Provider,
    live_now: &LiveNow,
    mut target: ModeState,
) -> Result<(), String> {
    let (proxy_url, _) = state.proxy_service.build_proxy_urls().await?;
    let force = op_name == op::ATTACH;
    match app {
        AppType::Claude => {
            let (projection, contract) = claude_contract(route, &proxy_url);
            let unchanged = !force
                && matches!(live_now, LiveNow::Proxy { contract: Some(c), .. } if c.key == contract.key);
            let patch = direct_patch(live_now.claude_exclusive_owner().as_ref(), &projection);
            target.contract = Some(contract);
            claude_direct::run(
                &state.db,
                op_name,
                (!unchanged).then_some(&patch),
                PendingTarget {
                    state: Some(target),
                    ..PendingTarget::default()
                },
            )
            .map_err(err)?;
        }
        AppType::Gemini => {
            backfill_direct(state, app, live_now);
            let contract = contract::gemini(&proxy_url);
            let unchanged = !force
                && matches!(live_now, LiveNow::Proxy { contract: Some(c), .. } if c.key == contract.key);
            target.contract = Some(contract);
            let patch = gemini_proxy_patch(&proxy_url);
            run_gemini(
                state,
                op_name,
                (!unchanged).then_some(&patch),
                PendingTarget {
                    state: Some(target),
                    ..PendingTarget::default()
                },
            )
            .map_err(err)?;
        }
        AppType::Codex => {
            let (_, base_url) = state.proxy_service.build_proxy_urls().await?;
            let owner = live_now.codex_owner();
            let spec = codex_direct::Target::Proxy {
                route,
                base_url: &base_url,
            };
            let prepared =
                codex_direct::prepare(&state.codex_oauth_manager, &owner, &spec).map_err(err)?;
            let planned = codex_direct::plan(&state.db, &owner, &spec, &prepared).map_err(err)?;
            let unchanged = !force
                && matches!(live_now, LiveNow::Proxy { contract: Some(c), .. } if c.key == planned.contract.key);
            target.contract = Some(planned.contract.clone());
            let pending = PendingTarget {
                state: Some(target),
                ..PendingTarget::default()
            };
            if unchanged {
                commit_state(state, app, &pending)?;
            } else {
                codex_direct::run(&state.db, op_name, planned, &prepared, pending).map_err(err)?;
            }
        }
        AppType::GrokBuild => {
            let contract = contract::whole_row(app.as_str(), &proxy_url, route);
            let unchanged = !force
                && matches!(live_now, LiveNow::Proxy { contract: Some(c), .. } if c.key == contract.key);
            if !unchanged {
                backfill_direct(state, app, live_now);
                state
                    .proxy_service
                    .sync_grok_live_from_provider_while_proxy_active(route)
                    .await?;
            }
            target.contract = Some(contract);
            commit_state(
                state,
                app,
                &PendingTarget {
                    state: Some(target),
                    ..PendingTarget::default()
                },
            )?;
        }
        _ => return Err(format!("{} 不支持本地路由", app.as_str())),
    }
    Ok(())
}

/// 写回直连投影（直连指针的供应商）。
fn write_direct(
    state: &AppState,
    app: &AppType,
    op_name: &str,
    live_now: &LiveNow,
    target: ModeState,
) -> Result<(), String> {
    let direct = direct_provider(state, app)?;
    let pending_target = PendingTarget {
        state: Some(target),
        ..PendingTarget::default()
    };
    let attached = matches!(live_now, LiveNow::Proxy { .. });
    match app {
        AppType::Claude => {
            let empty = ClaudeProjection::default();
            // 行里本身带着占位符（旧版接管期间被导入的残留）时不能照写，否则客户端会
            // 一直指着已经不在的本地代理：只清空关键字段。
            let projection = direct
                .as_ref()
                .filter(|provider| {
                    let polluted = crate::services::ProxyService::config_has_proxy_placeholder(
                        app,
                        &provider.settings_config,
                    );
                    if polluted {
                        log::warn!(
                            "直连供应商 {} 的行里带着代理占位符，只清空关键字段",
                            provider.id
                        );
                    }
                    !polluted
                })
                .map(|provider| ClaudeProjection::of(&provider.settings_config));
            let patch = direct_patch(
                live_now.claude_exclusive_owner().as_ref(),
                projection.as_ref().unwrap_or(&empty),
            );
            claude_direct::run(
                &state.db,
                op_name,
                attached.then_some(&patch),
                pending_target,
            )
            .map_err(err)?;
        }
        AppType::Codex => {
            if !attached {
                commit_state(state, app, &pending_target)?;
                return Ok(());
            }
            // 行里本身带着占位符（旧版接管期间被导入的残留）时不能照写：只清空关键字段。
            let target = direct.as_ref().filter(|provider| {
                !crate::services::ProxyService::config_has_proxy_placeholder(
                    app,
                    &provider.settings_config,
                )
            });
            let owner = live_now.codex_owner();
            if let Err(error) = codex_direct::write_direct(
                &state.db,
                &state.codex_oauth_manager,
                op_name,
                owner,
                target,
                pending_target.clone(),
            ) {
                // 直连供应商写不出来（比如绑定的托管账号已被删除）也不能让客户端一直指着
                // 代理：退一步只清空关键字段。
                log::warn!("写回直连的 Codex 配置失败，只清空关键字段: {error}");
                codex_direct::write_direct(
                    &state.db,
                    &state.codex_oauth_manager,
                    op_name,
                    owner,
                    None,
                    pending_target,
                )
                .map_err(err)?;
            }
        }
        AppType::Gemini | AppType::GrokBuild => {
            if attached {
                state.proxy_service.restore_live_from_direct_provider(app)?;
                if let Err(error) = McpService::sync_enabled_for_app(state, app) {
                    log::warn!("写回直连配置后重投影 {app:?} MCP 失败（下次同步时自愈）: {error}");
                }
            }
            if matches!(app, AppType::Gemini) {
                run_gemini(state, op_name, None, pending_target).map_err(err)?;
            } else {
                commit_state(state, app, &pending_target)?;
            }
        }
        _ => return Err(format!("{} 不支持本地路由", app.as_str())),
    }
    Ok(())
}

fn require_proxy_app(app: &AppType) -> Result<(), String> {
    if app.supports_local_proxy() {
        Ok(())
    } else {
        Err(format!("{} 不支持本地路由", app.as_str()))
    }
}

/// 进入代理模式。
pub async fn enter(state: &AppState, app: &AppType) -> Result<(), String> {
    require_proxy_app(app)?;
    let result = {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        enter_locked(state, app, op::ENTER).await
    };
    if result.is_err() {
        stop_server_if_unused(state).await;
    }
    result
}

async fn enter_locked(state: &AppState, app: &AppType, op_name: &str) -> Result<(), String> {
    if !state.proxy_service.is_running().await {
        state.proxy_service.start().await?;
    }
    let mode = current::mode_state(app);
    let route = match route_provider(state, app, &mode)? {
        Some(route) => route,
        None => direct_provider(state, app)?.ok_or_else(|| {
            format!(
                "{} 没有当前供应商，无法进入代理模式 (No current provider for {})",
                app.as_str(),
                app.as_str()
            )
        })?,
    };
    let live_now = LiveNow::of(state, app, &mode)?;
    write_proxy(
        state,
        app,
        op_name,
        &route,
        &live_now,
        ModeState {
            mode: Some(Mode::Proxy),
            attached: true,
            proxy_route: Some(route.id.clone()),
            contract: None,
        },
    )
    .await?;
    state.proxy_service.set_active_target(app, &route).await;
    warn_if_official_route(state, app, &route).await;
    Ok(())
}

async fn warn_if_official_route(state: &AppState, app: &AppType, route: &Provider) {
    if route.category.as_deref() == Some("official")
        && !crate::services::provider::official_provider_supports_proxy_takeover(app, route)
    {
        state
            .proxy_service
            .emit(
                "proxy-official-warning",
                json!({ "appType": app.as_str(), "providerName": route.name }),
            )
            .await;
    }
}

/// 退出代理模式：客户端写回直连指针的供应商，路由保留。
pub async fn exit(state: &AppState, app: &AppType) -> Result<(), String> {
    require_proxy_app(app)?;
    {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        exit_locked(state, app, false)?;
    }
    if let Err(error) = state.db.clear_provider_health_for_app(app.as_str()).await {
        log::warn!("清除 {} 健康状态失败: {error}", app.as_str());
    }
    stop_server_if_unused(state).await;
    Ok(())
}

/// `keep_mode` 为真是分离（退出 CC Switch）：模式和路由不变，只把客户端指回直连。
fn exit_locked(state: &AppState, app: &AppType, keep_mode: bool) -> Result<(), String> {
    let mode = current::mode_state(app);
    if keep_mode && !mode.attached {
        return Ok(());
    }
    if !keep_mode && !mode.is_proxy() && !mode.attached {
        return Ok(());
    }
    let live_now = LiveNow::of(state, app, &mode)?;
    write_direct(
        state,
        app,
        if keep_mode { op::DETACH } else { op::EXIT },
        &live_now,
        ModeState {
            mode: Some(if keep_mode && mode.is_proxy() {
                Mode::Proxy
            } else {
                Mode::Direct
            }),
            attached: false,
            proxy_route: mode.proxy_route,
            contract: None,
        },
    )
}

/// 没有应用在代理模式了就停掉代理服务（Claude Desktop 的模型映射另外自己启停）。
async fn stop_server_if_unused(state: &AppState) {
    if PROXY_APPS.iter().any(current::is_proxy) {
        return;
    }
    if state.proxy_service.is_running().await {
        if let Err(error) = state.proxy_service.stop().await {
            log::warn!("停止代理服务失败: {error}");
        }
    }
}

/// 「关闭本地路由」：全部退回直连，再停掉代理服务。
pub async fn exit_all(state: &AppState) -> Result<(), String> {
    let mut errors = Vec::new();
    for app in PROXY_APPS {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        if let Err(error) = exit_locked(state, &app, false) {
            errors.push(format!("{}: {error}", app.as_str()));
        }
    }
    if state.proxy_service.is_running().await {
        if let Err(error) = state.proxy_service.stop().await {
            log::warn!("停止代理服务失败: {error}");
        }
    }
    if let Err(error) = state.db.clear_all_provider_health().await {
        log::warn!("重置健康状态失败: {error}");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

/// 退出 CC Switch 时：把接上代理的客户端都指回直连（模式不变，下次启动再接上），再停掉
/// 代理服务。
pub async fn detach_all(state: &AppState) {
    for app in PROXY_APPS {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        if let Err(error) = exit_locked(state, &app, true) {
            log::error!("退出时把 {} 指回直连失败: {error}", app.as_str());
        }
    }
    if state.proxy_service.is_running().await {
        if let Err(error) = state.proxy_service.stop().await {
            log::warn!("退出时停止代理服务失败: {error}");
        }
    }
}

/// 代理模式下手动换路由。调用方持有切换锁。直连指针不变。
pub async fn switch_route_locked(
    state: &AppState,
    app: &AppType,
    target: &Provider,
) -> Result<(), String> {
    let mode = current::mode_state(app);
    if !mode.is_proxy() {
        return Err(format!("{} 不在代理模式", app.as_str()));
    }
    let new_state = ModeState {
        proxy_route: Some(target.id.clone()),
        ..mode.clone()
    };
    if !mode.attached {
        commit_state(
            state,
            app,
            &PendingTarget {
                state: Some(new_state),
                ..PendingTarget::default()
            },
        )?;
    } else {
        let live_now = LiveNow::of(state, app, &mode)?;
        write_proxy(state, app, op::ROUTE, target, &live_now, new_state).await?;
    }
    state.proxy_service.set_active_target(app, target).await;
    Ok(())
}

/// 代理模式下手动换路由（拿切换锁）。不支持代理的官方供应商拒绝切入。
pub async fn switch_route(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
) -> Result<(), String> {
    require_proxy_app(app)?;
    let target =
        provider(state, app, provider_id)?.ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    reject_unsupported_official(app, &target)?;
    let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
    switch_route_locked(state, app, &target).await
}

/// 代理模式下不能切到不支持代理的官方供应商（Codex 官方账号走客户端自己的登录，除外）。
pub fn reject_unsupported_official(app: &AppType, provider: &Provider) -> Result<(), String> {
    if provider.category.as_deref() == Some("official")
        && !crate::services::provider::official_provider_supports_proxy_takeover(app, provider)
    {
        return Err(
            "代理模式下不能切换到官方供应商 (Cannot switch to an official provider in proxy mode)"
                .to_string(),
        );
    }
    Ok(())
}

/// 路由供应商的行或代理地址变了：按新契约重写客户端（契约没变就什么都不做）。调用方
/// 持有切换锁。
pub async fn resync_route_locked(state: &AppState, app: &AppType) -> Result<(), String> {
    let mode = current::mode_state(app);
    if !mode.is_proxy() || !mode.attached {
        return Ok(());
    }
    let Some(route) = route_provider(state, app, &mode)? else {
        return Ok(());
    };
    switch_route_locked(state, app, &route).await
}

pub async fn resync_route(state: &AppState, app: &AppType) -> Result<(), String> {
    let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
    resync_route_locked(state, app).await
}

/// 故障转移成功后记下新路由：只换代理的上游，不写客户端文件（契约兼容性本轮不检查）。
/// 返回路由是否真的变了。
pub async fn record_failover_route(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
) -> Result<bool, String> {
    let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
    let mode = current::mode_state(app);
    if !mode.is_proxy() || mode.proxy_route.as_deref() == Some(provider_id) {
        return Ok(false);
    }
    let Some(target) = provider(state, app, provider_id)? else {
        return Err(format!("供应商不存在: {provider_id}"));
    };
    commit_state(
        state,
        app,
        &PendingTarget {
            state: Some(ModeState {
                proxy_route: Some(provider_id.to_string()),
                ..mode
            }),
            ..PendingTarget::default()
        },
    )?;
    state.proxy_service.set_active_target(app, &target).await;
    Ok(true)
}

/// 启动时：先按旧版遗留的接管状态定下每个应用的模式（首次运行新版、降级后再升级），
/// 再把代理模式的应用接上。要在补完上次未完成的写入之后、自动提取通用配置片段之后。
///
/// | `proxy_config.enabled` | 备份行或占位符 | 处理 |
/// |---|---|---|
/// | 1 | 无 | 代理模式，接上 |
/// | 1 | 有 | 不回放备份，直接写代理契约；备份行转存到本机文件后删除 |
/// | 0 | 有 | 写回直连投影 |
/// | 0 | 无 | 直连 |
///
/// 有遗留物时以 `enabled` 为准（旧版是最后一个写入者）；没有时以 `live-state.json`
/// 为准，它还没有值就按 `enabled` 定。
pub async fn startup(state: &AppState) {
    for app in PROXY_APPS {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        if let Err(error) = startup_app(state, &app).await {
            log::error!("启动时恢复 {} 的模式失败: {error}", app.as_str());
        }
    }
    // 接上失败退回直连的应用可能已经把代理拉起来了。
    stop_server_if_unused(state).await;
}

async fn startup_app(state: &AppState, app: &AppType) -> Result<(), String> {
    let had_backup = drain_legacy_backup(state, app).await;
    let mut mode = current::mode_state(app);
    // 新版自己接上时写的占位符不算遗留物（比如重启更新时没来得及分离）。
    let placeholder = !mode.attached && state.proxy_service.live_has_proxy_placeholder(app);
    let legacy = had_backup || placeholder;
    let (enabled, _) = state.db.get_proxy_flags_sync(app.as_str());
    let want_proxy = if legacy {
        enabled
    } else {
        mode.mode.map_or(enabled, |mode| mode == Mode::Proxy)
    };

    if placeholder {
        // 旧版留下的接管态：客户端里是旧契约。按「已接上」处理，下面的投影会整体换掉它。
        mode.attached = true;
    }

    if want_proxy {
        let mode = ModeState {
            mode: Some(Mode::Proxy),
            ..mode
        };
        commit_state(
            state,
            app,
            &PendingTarget {
                state: Some(mode),
                ..PendingTarget::default()
            },
        )?;
        match enter_locked(state, app, op::ATTACH).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                log::error!("启动时接上 {} 的代理失败，退回直连: {error}", app.as_str());
                exit_locked(state, app, false)?;
                return Err(error);
            }
        }
    }

    if mode.attached || mode.mode != Some(Mode::Direct) {
        if placeholder {
            log::warn!(
                "{} 的客户端配置里有旧版接管留下的代理占位符，已写回直连配置",
                app.as_str()
            );
        }
        let live_now = if mode.attached {
            LiveNow::Proxy {
                contract: mode.contract.clone(),
                route: route_provider(state, app, &mode)?,
            }
        } else {
            LiveNow::Direct(direct_provider(state, app)?)
        };
        write_direct(
            state,
            app,
            op::EXIT,
            &live_now,
            ModeState {
                mode: Some(Mode::Direct),
                attached: false,
                proxy_route: mode.proxy_route,
                contract: None,
            },
        )?;
    }
    Ok(())
}

/// 旧版的接管备份行：不回放，转存到本机文件后删除。留着的话，降级后旧版启动时会把这份
/// 陈旧的快照写回客户端。
async fn drain_legacy_backup(state: &AppState, app: &AppType) -> bool {
    let backup = match state.db.get_live_backup(app.as_str()).await {
        Ok(Some(backup)) => backup,
        Ok(None) => return false,
        Err(error) => {
            log::warn!("读取 {} 的旧接管备份失败: {error}", app.as_str());
            return false;
        }
    };
    let dir = crate::config::get_home_dir()
        .join(".cc-switch")
        .join("backups")
        .join("proxy-live-backup");
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let path = dir.join(format!("{}-{stamp}.json", app.as_str()));
    let saved = serde_json::to_vec_pretty(&json!({
        "app": app.as_str(),
        "backedUpAt": backup.backed_up_at,
        "originalConfig": serde_json::from_str::<Value>(&backup.original_config)
            .unwrap_or(Value::String(backup.original_config.clone())),
    }))
    .map_err(err)
    .and_then(|bytes| {
        std::fs::create_dir_all(&dir).map_err(err)?;
        crate::config::atomic_write_private(&path, &bytes).map_err(err)
    });
    match saved {
        Ok(()) => {
            if let Err(error) = state.db.delete_live_backup(app.as_str()).await {
                log::warn!("删除 {} 的旧接管备份失败: {error}", app.as_str());
            } else {
                log::info!(
                    "{} 的旧接管备份已转存到 {} 并从数据库删除",
                    app.as_str(),
                    path.display()
                );
            }
        }
        Err(error) => log::warn!(
            "转存 {} 的旧接管备份失败，保留数据库里的备份行: {error}",
            app.as_str()
        ),
    }
    true
}

/// 给前端：直连指针（代理模式下退出代理时写回的那家）。
pub fn direct_provider_id(state: &AppState, app: &AppType) -> Result<Option<String>, AppError> {
    current::provider_for(&state.db, app, Purpose::Direct)
}

#[cfg(test)]
mod tests {
    //! 代理契约里的凭据和模型别名（从旧的接管字段测试迁过来：#3784、#4919、#1049）。
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::Map;
    use std::path::Path;

    fn assert_env_str(env: &Map<String, Value>, key: &str, expected: Option<&str>) {
        assert_eq!(env.get(key).and_then(Value::as_str), expected, "{key}");
    }

    /// 以 `live` 为底写入 `provider` 的代理契约，和进入代理时的补丁相同。
    fn takeover(live: &Value, provider: &Provider) -> Value {
        let (projection, _) = claude_contract(provider, "http://127.0.0.1:15721");
        let mut doc = live.clone();
        direct_patch(None, &projection)
            .apply_to(Path::new("settings.json"), &mut doc)
            .expect("apply proxy contract");
        doc
    }

    #[test]
    fn managed_account_claude_takeover_uses_auth_token_placeholder() {
        let mut provider = Provider::with_id(
            "copilot".to_string(),
            "GitHub Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                    "ANTHROPIC_MODEL": "claude-haiku-4.5"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });

        let mut live_config = provider.settings_config.clone();
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|value| value.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(
            env.get("ANTHROPIC_API_KEY").is_none(),
            "API_KEY placeholders trigger Claude Code's custom-key approval prompt (defaults to No), landing users in Not logged in"
        );
    }

    #[test]
    fn managed_account_claude_takeover_sources_copilot_models_from_provider() {
        let mut provider = Provider::with_id(
            "copilot".to_string(),
            "GitHub Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com",
                    "ANTHROPIC_MODEL": "claude-sonnet-4.6",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4.5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-sonnet-4.6",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "claude-sonnet-4.6[1M]"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://stale.example.com",
                "ANTHROPIC_API_KEY": "stale-key",
                "ANTHROPIC_MODEL": "stale-model",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
                "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus",
                "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_MODEL", None);
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            Some("claude-haiku-4-5"),
        );
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            Some("claude-haiku-4.5"),
        );
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            Some("claude-sonnet-5"),
        );
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            Some("claude-sonnet-4.6"),
        );
        assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-5"));
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            Some("claude-sonnet-4.6"),
        );
        assert_env_str(
            env,
            "CLAUDE_CODE_SUBAGENT_MODEL",
            Some("claude-sonnet-4.6[1M]"),
        );
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
    }

    #[test]
    fn managed_account_claude_takeover_removes_stale_subagent_model_when_provider_omits_it() {
        let mut provider = Provider::with_id(
            "codex".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "provider-sonnet"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://stale.example.com",
                "ANTHROPIC_API_KEY": "stale-key",
                "CLAUDE_CODE_SUBAGENT_MODEL": "stale-subagent"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "CLAUDE_CODE_SUBAGENT_MODEL", None);
    }

    #[test]
    fn managed_account_claude_takeover_sources_codex_models_from_provider() {
        let mut provider = Provider::with_id(
            "codex".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex",
                    "ANTHROPIC_MODEL": "gpt-5.4",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.4-mini",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.4",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "gpt-5.4"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://stale.example.com",
                "ANTHROPIC_AUTH_TOKEN": "stale-token",
                "ANTHROPIC_MODEL": "stale-model",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "stale-haiku",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "Stale Haiku",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "stale-sonnet",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Stale Sonnet",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "stale-opus",
                "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "Stale Opus"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_MODEL", None);
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            Some("claude-haiku-4-5"),
        );
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            Some("gpt-5.4-mini"),
        );
        assert_env_str(
            env,
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            Some("claude-sonnet-5"),
        );
        assert_env_str(env, "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME", Some("gpt-5.4"));
        assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL", Some("claude-opus-5"));
        assert_env_str(env, "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME", Some("gpt-5.4"));
        // Codex 系只保留 AUTH_TOKEN；双键会触发 Claude Code 告警（#4919）
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    }

    #[test]
    fn managed_account_claude_takeover_codex_injects_auth_token_without_preexisting_key() {
        let mut provider = Provider::with_id(
            "codex".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        // 全新安装/热切换形态：传入的 env 没有任何 token 键。
        let mut live_config = provider.settings_config.clone();
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    }

    #[test]
    fn managed_account_claude_takeover_xai_keeps_one_auth_key() {
        let mut provider = Provider::with_id(
            "xai".to_string(),
            "xAI".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.x.ai/v1"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "old-token",
                "ANTHROPIC_API_KEY": "old-key",
                "OPENAI_API_KEY": "old-openai-key"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(Value::as_object)
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
        // Claude Code 不读这个键：不是关键字段，归用户，契约不碰。
        assert_env_str(env, "OPENAI_API_KEY", Some("old-openai-key"));
    }

    #[test]
    fn managed_account_claude_takeover_codex_by_base_url_keeps_auth_token() {
        // 无 provider_type meta、仅凭 base_url 识别为受管 codex 的供应商，
        // 也必须保留 AUTH_TOKEN 占位符（与策略选择共用同一判定族）。
        let provider = Provider::with_id(
            "codex-url-only".to_string(),
            "Codex (URL only)".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            None,
        );
        assert!(provider.uses_managed_account_auth());
        assert!(!provider.is_codex_oauth());

        let mut live_config = provider.settings_config.clone();
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
    }

    // #4919 复现场景：从第三方 Claude 供应商（live 已有 AUTH_TOKEN）切换到
    // Codex 受管供应商时，只应保留 AUTH_TOKEN 占位符，不得同时写入 API_KEY。
    #[test]
    fn managed_account_claude_takeover_codex_from_third_party_keeps_single_auth_key() {
        let mut provider = Provider::with_id(
            "codex".to_string(),
            "Codex".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://chatgpt.com/backend-api/codex"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "sk-third-party"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
    }

    #[test]
    fn managed_account_claude_takeover_copilot_defaults_to_auth_token() {
        let mut provider = Provider::with_id(
            "copilot".to_string(),
            "GitHub Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://stale.example.com",
                "ANTHROPIC_AUTH_TOKEN": "stale-token",
                "ANTHROPIC_API_KEY": "stale-key"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        // Default Copilot takeover injects AUTH_TOKEN: the API_KEY placeholder
        // triggers Claude Code's custom-key approval prompt (defaults to
        // "No (recommended)"), which lands users in "Not logged in".
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", Some(PROXY_TOKEN_PLACEHOLDER));
        assert_env_str(env, "ANTHROPIC_API_KEY", None);
    }

    #[test]
    fn managed_account_claude_takeover_copilot_honors_api_key_field_choice() {
        let mut provider = Provider::with_id(
            "copilot".to_string(),
            "GitHub Copilot".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
            ..Default::default()
        });

        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://stale.example.com",
                "ANTHROPIC_AUTH_TOKEN": "stale-token"
            }
        });
        live_config = takeover(&live_config, &provider);

        let env = live_config
            .get("env")
            .and_then(|value| value.as_object())
            .expect("env should exist");
        // Explicit API-key-field choice keeps the API_KEY placeholder to avoid
        // conflicting with the /login-managed key (#1049).
        assert_env_str(env, "ANTHROPIC_API_KEY", Some(PROXY_TOKEN_PLACEHOLDER));
        assert_env_str(env, "ANTHROPIC_AUTH_TOKEN", None);
    }

    #[test]
    fn normal_claude_takeover_without_token_keeps_auth_token_fallback() {
        let mut live_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_MODEL": "claude-haiku-4.5"
            }
        });

        let plain = Provider::with_id(
            "plain".to_string(),
            "Plain".to_string(),
            live_config.clone(),
            None,
        );
        live_config = takeover(&live_config, &plain);

        assert_eq!(
            live_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(|value| value.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(
            live_config
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .is_none(),
            "non-managed providers should retain the legacy fallback behavior"
        );
    }
}

#[cfg(test)]
mod mode_tests {
    //! 双模式的验收：进入 / 退出只动关键字段和独有字段；契约相同的换路由不碰客户端文件；
    //! 代理路由和直连指针互相独立；每一步崩溃都能按 pending 补完；旧版遗留的接管状态
    //! 在启动时迁移掉。
    use super::*;
    use crate::database::Database;
    use crate::live::engine::DeviceStore;
    use crate::mode::operation::failpoint;
    use crate::mode::state::{self, Mode};
    use crate::proxy::types::ProxyConfig;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct Home {
        dir: TempDir,
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl Home {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp home");
            let saved = ["HOME", "USERPROFILE", "CC_SWITCH_TEST_HOME"]
                .into_iter()
                .map(|key| {
                    let old = std::env::var_os(key);
                    std::env::set_var(key, dir.path());
                    (key, old)
                })
                .collect();
            crate::settings::reload_settings().expect("reload settings");
            Self { dir, saved }
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            for (key, old) in self.saved.drain(..) {
                match old {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            let _ = crate::settings::reload_settings();
        }
    }

    fn claude(id: &str, url: &str, extra: Value) -> Provider {
        let mut env = json!({
            "ANTHROPIC_BASE_URL": url,
            "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}"),
            "ANTHROPIC_MODEL": "claude-sonnet-4-6"
        });
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                env[key] = value.clone();
            }
        }
        Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({ "env": env }),
            None,
        )
    }

    async fn state_with(app: AppType, rows: &[Provider], current: &str) -> AppState {
        let db = Arc::new(Database::memory().expect("memory db"));
        for row in rows {
            db.save_provider(app.as_str(), row).expect("save provider");
        }
        db.set_current_provider(app.as_str(), current)
            .expect("set current");
        crate::settings::set_current_provider(&app, Some(current)).expect("local current");
        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("ephemeral port");
        AppState::new(db)
    }

    fn settings_path() -> std::path::PathBuf {
        crate::config::get_claude_settings_path()
    }

    fn seed_settings(text: &str) {
        let path = settings_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn settings() -> Value {
        serde_json::from_slice(&fs::read(settings_path()).unwrap()).unwrap()
    }

    fn mode(app: &AppType) -> ModeState {
        state::mode_state(&DeviceStore::for_device(), app.as_str()).unwrap()
    }

    fn in_use(state: &AppState, app: &AppType) -> Option<String> {
        current::provider_for(&state.db, app, Purpose::InUse).unwrap()
    }

    fn direct(state: &AppState, app: &AppType) -> Option<String> {
        current::provider_for(&state.db, app, Purpose::Direct).unwrap()
    }

    const USER_SETTINGS: &str = r#"{
  "hooks": {
    "Stop": []
  },
  "env": {
    "ANTHROPIC_BASE_URL": "https://a.example",
    "ANTHROPIC_AUTH_TOKEN": "sk-a",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
    "DISABLE_TELEMETRY": "1"
  },
  "permissions": {
    "allow": [
      "Bash"
    ]
  }
}
"#;

    /// 回到直连后：值和用户的原文件相同；非关键字段的顺序不变（关键字段可能挪到末尾）。
    fn assert_back_to_user_settings() {
        let original: Value = serde_json::from_str(USER_SETTINGS).unwrap();
        let live = settings();
        assert_eq!(live, original);
        let keys = |value: &Value| -> Vec<String> {
            value
                .as_object()
                .unwrap()
                .keys()
                .filter(|key| !crate::live::floor::claude_floor_env(key))
                .cloned()
                .collect()
        };
        assert_eq!(keys(&live), keys(&original));
        assert_eq!(keys(&live["env"]), keys(&original["env"]));
    }

    #[tokio::test]
    #[serial]
    async fn entering_and_leaving_proxy_mode_only_touches_key_and_exclusive_fields() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;

        enter(&state, &AppType::Claude).await.expect("enter");
        let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        let live = settings();
        assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
        assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], PROXY_TOKEN_PLACEHOLDER);
        assert_eq!(
            live["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-sonnet-5"
        );
        assert!(live["env"].get("ANTHROPIC_MODEL").is_none());
        assert_eq!(live["env"]["DISABLE_TELEMETRY"], "1");
        assert_eq!(live["hooks"], json!({ "Stop": [] }));
        let entered = mode(&AppType::Claude);
        assert_eq!(entered.mode, Some(Mode::Proxy));
        assert!(entered.attached);
        assert_eq!(entered.proxy_route.as_deref(), Some("a"));
        assert!(entered.contract.is_some());
        assert!(
            state.db.get_proxy_flags_sync("claude").0,
            "enabled mirrors the mode"
        );

        exit(&state, &AppType::Claude).await.expect("exit");
        assert_back_to_user_settings();
        // 第一次写入可能挪动关键字段的位置，之后的往返字节稳定。
        let settled = fs::read(settings_path()).unwrap();
        enter(&state, &AppType::Claude).await.expect("enter again");
        exit(&state, &AppType::Claude).await.expect("exit again");
        assert_eq!(fs::read(settings_path()).unwrap(), settled);
        let left = mode(&AppType::Claude);
        assert_eq!(left.mode, Some(Mode::Direct));
        assert_eq!(left.proxy_route.as_deref(), Some("a"), "the route is kept");
        assert!(!state.db.get_proxy_flags_sync("claude").0);
        assert!(!state.proxy_service.is_running().await);
    }

    #[tokio::test]
    #[serial]
    async fn a_route_switch_with_the_same_contract_leaves_the_client_file_alone() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[
                claude("a", "https://a.example", json!({})),
                claude("b", "https://b.example", json!({})),
            ],
            "a",
        )
        .await;
        enter(&state, &AppType::Claude).await.expect("enter");
        let before = fs::read(settings_path()).unwrap();
        let mtime = fs::metadata(settings_path()).unwrap().modified().unwrap();

        // 契约相同时客户端文件不读也不写：连读权限都拿掉，换路由照样成功。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(settings_path(), fs::Permissions::from_mode(0o000)).unwrap();
        }
        ProviderService::switch(&state, AppType::Claude, "b").expect("switch route");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(settings_path(), fs::Permissions::from_mode(0o600)).unwrap();
        }

        assert_eq!(fs::read(settings_path()).unwrap(), before);
        assert_eq!(
            fs::metadata(settings_path()).unwrap().modified().unwrap(),
            mtime
        );
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));
        assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));

        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
    }

    #[tokio::test]
    #[serial]
    async fn the_route_providers_exclusive_fields_follow_the_contract() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[
                claude("a", "https://a.example", json!({})),
                claude(
                    "deepseek",
                    "https://deepseek.example",
                    json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
                ),
            ],
            "a",
        )
        .await;
        enter(&state, &AppType::Claude).await.expect("enter");
        ProviderService::switch(&state, AppType::Claude, "deepseek").expect("switch route");
        assert_eq!(settings()["env"]["CLAUDE_CODE_DISABLE_ARTIFACT"], "1");
        assert_eq!(
            mode(&AppType::Claude).contract.unwrap().exclusive["CLAUDE_CODE_DISABLE_ARTIFACT"],
            "1"
        );

        exit(&state, &AppType::Claude).await.expect("exit");
        assert!(
            settings()["env"]
                .get("CLAUDE_CODE_DISABLE_ARTIFACT")
                .is_none(),
            "the direct provider does not need it, so leaving proxy mode removes it"
        );
        assert_back_to_user_settings();
    }

    #[tokio::test]
    #[serial]
    async fn the_proxy_route_is_independent_of_the_direct_pointer() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[
                claude("a", "https://a.example", json!({})),
                claude("b", "https://b.example", json!({})),
            ],
            "a",
        )
        .await;
        enter(&state, &AppType::Claude).await.expect("enter");
        ProviderService::switch(&state, AppType::Claude, "b").expect("route to b");
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));

        enter(&state, &AppType::Claude).await.expect("enter again");
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));

        // 退出 CC Switch 再启动：分离时写回直连，启动时按保存的路由接上。
        detach_all(&state).await;
        assert!(!mode(&AppType::Claude).attached);
        assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
        startup(&state).await;
        let restarted = mode(&AppType::Claude);
        assert!(restarted.attached);
        assert_eq!(restarted.proxy_route.as_deref(), Some("b"));
        assert_eq!(
            settings()["env"]["ANTHROPIC_AUTH_TOKEN"],
            PROXY_TOKEN_PLACEHOLDER
        );
        exit(&state, &AppType::Claude).await.expect("exit");
    }

    #[tokio::test]
    #[serial]
    async fn failover_moves_the_route_without_writing_client_files() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[
                claude("a", "https://a.example", json!({})),
                claude(
                    "b",
                    "https://b.example",
                    json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
                ),
            ],
            "a",
        )
        .await;
        enter(&state, &AppType::Claude).await.expect("enter");
        let before = fs::read(settings_path()).unwrap();

        assert!(record_failover_route(&state, &AppType::Claude, "b")
            .await
            .expect("record failover"));
        assert_eq!(fs::read(settings_path()).unwrap(), before);
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("b"));
        assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));
        exit(&state, &AppType::Claude).await.expect("exit");
    }

    #[tokio::test]
    #[serial]
    async fn a_crash_at_any_step_is_finished_or_discarded_on_startup() {
        for (point, expect_proxy) in [("pending", false), ("published:0", true), ("target", true)] {
            let _home = Home::new();
            seed_settings(USER_SETTINGS);
            let state = state_with(
                AppType::Claude,
                &[claude("a", "https://a.example", json!({}))],
                "a",
            )
            .await;
            failpoint::crash_at(Some(point));
            let result = enter(&state, &AppType::Claude).await;
            failpoint::crash_at(None);
            assert!(result.is_err(), "{point}");

            crate::mode::operation::recover_on_startup(&state.db);
            let recovered = mode(&AppType::Claude);
            let live = settings();
            let live_is_proxy = live["env"]["ANTHROPIC_AUTH_TOKEN"] == PROXY_TOKEN_PLACEHOLDER;
            assert_eq!(recovered.is_proxy(), expect_proxy, "{point}");
            assert_eq!(recovered.attached, expect_proxy, "{point}");
            assert_eq!(live_is_proxy, expect_proxy, "{point}: file and state agree");
            assert_eq!(
                state.db.get_proxy_flags_sync("claude").0,
                expect_proxy,
                "{point}"
            );
            assert!(state::pending(&DeviceStore::for_device(), "claude")
                .unwrap()
                .is_none());
            if !expect_proxy {
                assert_eq!(fs::read_to_string(settings_path()).unwrap(), USER_SETTINGS);
            }
            if state.proxy_service.is_running().await {
                state.proxy_service.stop().await.unwrap();
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn a_crash_while_leaving_proxy_mode_is_finished_on_startup() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;
        enter(&state, &AppType::Claude).await.expect("enter");
        failpoint::crash_at(Some("published:0"));
        let result = exit(&state, &AppType::Claude).await;
        failpoint::crash_at(None);
        assert!(result.is_err());

        crate::mode::operation::recover_on_startup(&state.db);
        assert_eq!(mode(&AppType::Claude).mode, Some(Mode::Direct));
        assert_back_to_user_settings();
        if state.proxy_service.is_running().await {
            state.proxy_service.stop().await.unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn startup_moves_legacy_takeover_state_over_to_the_modes() {
        for enabled in [true, false] {
            let home = Home::new();
            seed_settings(
                r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:15721","ANTHROPIC_AUTH_TOKEN":"PROXY_MANAGED"},"hooks":{}}"#,
            );
            let state = state_with(
                AppType::Claude,
                &[claude("a", "https://a.example", json!({}))],
                "a",
            )
            .await;
            state
                .db
                .save_live_backup(
                    "claude",
                    r#"{"env":{"ANTHROPIC_BASE_URL":"https://stale.example"}}"#,
                )
                .await
                .unwrap();
            state
                .db
                .set_proxy_flags_sync("claude", enabled, false)
                .unwrap();

            startup(&state).await;

            assert!(
                state.db.get_live_backup("claude").await.unwrap().is_none(),
                "the old version would replay a leftover backup row after a downgrade"
            );
            let drained = home.dir.path().join(".cc-switch/backups/proxy-live-backup");
            assert_eq!(
                fs::read_dir(&drained).unwrap().count(),
                1,
                "kept aside as a file"
            );
            let migrated = mode(&AppType::Claude);
            let live = settings();
            assert_eq!(live["hooks"], json!({}));
            if enabled {
                assert!(migrated.is_proxy() && migrated.attached);
                let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
                assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
                exit(&state, &AppType::Claude).await.unwrap();
            } else {
                assert_eq!(migrated.mode, Some(Mode::Direct));
                assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], "https://a.example");
                assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
                assert!(!state.proxy_service.is_running().await);
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn gemini_proxy_contract_keeps_the_other_env_lines() {
        let _home = Home::new();
        let env_path = crate::gemini_config::get_gemini_env_path();
        fs::create_dir_all(env_path.parent().unwrap()).unwrap();
        fs::write(
            &env_path,
            "# my notes\nGEMINI_SANDBOX=true\nGEMINI_API_KEY=real-key\nGOOGLE_GEMINI_BASE_URL=https://g.example\n",
        )
        .unwrap();
        let row = Provider::with_id(
            "g".to_string(),
            "G".to_string(),
            json!({ "env": {
                "GEMINI_API_KEY": "real-key",
                "GOOGLE_GEMINI_BASE_URL": "https://g.example"
            }}),
            None,
        );
        let state = state_with(AppType::Gemini, &[row], "g").await;

        enter(&state, &AppType::Gemini).await.expect("enter");
        let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        assert_eq!(
            fs::read_to_string(&env_path).unwrap(),
            format!(
                "# my notes\nGEMINI_SANDBOX=true\nGEMINI_API_KEY=PROXY_MANAGED\nGOOGLE_GEMINI_BASE_URL={proxy_url}\n"
            )
        );
        exit(&state, &AppType::Gemini).await.expect("exit");
        let text = fs::read_to_string(&env_path).unwrap();
        assert!(text.contains("GEMINI_API_KEY=real-key"), "{text}");
        assert!(!text.contains("PROXY_MANAGED"), "{text}");
    }

    #[tokio::test]
    #[serial]
    async fn codex_routes_between_official_and_third_party_contracts() {
        let _home = Home::new();
        let native_auth = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "native-id",
                "access_token": "native-access",
                "refresh_token": "native-refresh",
                "account_id": "acct-native"
            },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        crate::codex_config::write_codex_live_atomic(&native_auth, Some("model = \"gpt-5.4\"\n"))
            .unwrap();
        let mut official = Provider::with_id(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
            None,
        );
        official.category = Some("official".to_string());
        let relay = Provider::with_id(
            "relay".to_string(),
            "Relay".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-relay" },
                "config": "model_provider = \"custom\"\nmodel = \"gpt-5.4\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        );
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: true,
            ..Default::default()
        })
        .unwrap();
        let state = state_with(AppType::Codex, &[official.clone(), relay], "relay").await;
        // 直连在 relay 上：config.toml 是 relay 的，auth.json 保留原生登录。
        ProviderService::switch(&state, AppType::Codex, "relay").expect("direct relay");
        let config_path = crate::codex_config::get_codex_config_path();
        let auth_path = crate::codex_config::get_codex_auth_path();
        let auth = || -> Value { crate::config::read_json_file(&auth_path).unwrap() };

        enter(&state, &AppType::Codex).await.expect("enter");
        let third_party = fs::read_to_string(&config_path).unwrap();
        assert!(
            third_party.contains(PROXY_TOKEN_PLACEHOLDER),
            "{third_party}"
        );
        assert_eq!(
            auth(),
            native_auth,
            "the third-party contract never writes auth.json"
        );
        let relay_row = state
            .db
            .get_provider_by_id("relay", "codex")
            .unwrap()
            .unwrap();
        assert!(
            !relay_row
                .settings_config
                .to_string()
                .contains("native-access"),
            "backfilling on enter must not copy the ChatGPT login into a third-party row: {}",
            relay_row.settings_config
        );

        ProviderService::switch(&state, AppType::Codex, &official.id).expect("route to official");
        let official_contract = fs::read_to_string(&config_path).unwrap();
        let doc: toml::Table = toml::from_str(&official_contract).unwrap();
        assert_eq!(
            doc["model_provider"].as_str(),
            Some("cc-switch-official"),
            "{official_contract}"
        );
        let route = &doc["model_providers"]["cc-switch-official"];
        assert!(
            route.get("experimental_bearer_token").is_none(),
            "the official contract carries the client's own login: {official_contract}"
        );
        assert_eq!(route["requires_openai_auth"].as_bool(), Some(true));
        // 第三方路由留下的 custom 表改成休眠形态：指向本地代理、只有占位 Key。
        let dormant = &doc["model_providers"]["custom"];
        assert_eq!(
            dormant["experimental_bearer_token"].as_str(),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(dormant.get("requires_openai_auth").is_none());
        assert!(!official_contract.contains("sk-relay"));
        assert_eq!(auth(), native_auth);

        ProviderService::switch(&state, AppType::Codex, "relay").expect("route back");
        assert!(fs::read_to_string(&config_path)
            .unwrap()
            .contains(PROXY_TOKEN_PLACEHOLDER));
        exit(&state, &AppType::Codex).await.expect("exit");
        assert_eq!(auth(), native_auth);
        assert!(!state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
    }

    // ---------- Codex：只替换关键字段 ----------

    fn codex_row(id: &str, url: &str, extra: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({
                "auth": { "OPENAI_API_KEY": format!("sk-{id}") },
                "config": format!(
                    "model_provider = \"{id}\"\nmodel = \"gpt-{id}\"\n{extra}\n[model_providers.{id}]\nname = \"{id}\"\nbase_url = \"{url}\"\nwire_api = \"responses\"\n"
                ),
            }),
            None,
        )
    }

    fn codex_official() -> Provider {
        let mut official = Provider::with_id(
            crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        official
    }

    fn codex_config_path() -> std::path::PathBuf {
        crate::codex_config::get_codex_config_path()
    }

    fn codex_auth_path() -> std::path::PathBuf {
        crate::codex_config::get_codex_auth_path()
    }

    fn seed_codex(config: &str, auth: Option<&Value>) {
        let path = codex_config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, config).unwrap();
        match auth {
            Some(auth) => fs::write(codex_auth_path(), auth.to_string()).unwrap(),
            None => {
                let _ = fs::remove_file(codex_auth_path());
            }
        }
    }

    fn codex_text() -> String {
        fs::read_to_string(codex_config_path()).unwrap()
    }

    fn codex_doc() -> toml::Table {
        toml::from_str(&codex_text()).unwrap()
    }

    fn set_preservation(on: bool) {
        crate::settings::update_settings(crate::settings::AppSettings {
            preserve_codex_official_auth_on_switch: on,
            ..Default::default()
        })
        .unwrap();
    }

    fn chatgpt_login(account: &str) -> Value {
        json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "id",
                "access_token": format!("access-{account}"),
                "refresh_token": format!("refresh-{account}"),
                "account_id": account
            },
            "last_refresh": "2026-09-01T00:00:00Z"
        })
    }

    /// live 里 A 的关键字段之外，都是用户和 Codex 自己的东西。
    const CODEX_USER_LIVE: &str = r#"# 用户的注释
approval_policy = "on-request"
model_provider = "custom"
model = "gpt-a"
model_context_window = 200000

[projects."/work"]
trust_level = "trusted"

[agents]
default_subagent_model = "gpt-a-mini"
max_threads = 4

[model_providers.custom]
name = "a"
base_url = "https://a.example/v1"
wire_api = "responses"
experimental_bearer_token = "sk-a"

[model_providers.ollama_local]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[mcp_servers.fs]
command = "fs-server"
"#;

    fn codex_a_b() -> [Provider; 2] {
        [
            codex_row(
                "a",
                "https://a.example/v1",
                "model_context_window = 200000\n[agents]\ndefault_subagent_model = \"gpt-a-mini\"\n",
            ),
            codex_row("b", "https://b.example/v1", ""),
        ]
    }

    /// 关键字段之外的部分（用户的表、注释、MCP、项目信任）。
    fn codex_user_parts(text: &str) -> Vec<&str> {
        [
            "# 用户的注释",
            "approval_policy = \"on-request\"",
            "[projects.\"/work\"]",
            "max_threads = 4",
            "[model_providers.ollama_local]",
            "[mcp_servers.fs]",
        ]
        .into_iter()
        .filter(|part| text.contains(part))
        .collect()
    }

    #[tokio::test]
    #[serial]
    async fn codex_direct_switch_replaces_only_key_fields_and_round_trips() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

        ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
        let on_b = codex_text();
        let doc = codex_doc();
        assert_eq!(doc["model_provider"].as_str(), Some("custom"));
        assert_eq!(doc["model"].as_str(), Some("gpt-b"));
        let route = &doc["model_providers"]["custom"];
        assert_eq!(route["base_url"].as_str(), Some("https://b.example/v1"));
        assert_eq!(route["experimental_bearer_token"].as_str(), Some("sk-b"));
        assert!(!on_b.contains("sk-a"), "A's key is gone: {on_b}");
        // A 带进来的独有字段（值没被改过）删掉；嵌在 [agents] 里的模型名只删那一个键。
        assert!(doc.get("model_context_window").is_none(), "{on_b}");
        assert!(doc["agents"].get("default_subagent_model").is_none());
        assert_eq!(doc["agents"]["max_threads"].as_integer(), Some(4));
        assert_eq!(codex_user_parts(&on_b).len(), 6, "{on_b}");

        ProviderService::switch(&state, AppType::Codex, "a").expect("back to a");
        let on_a = codex_text();
        let doc = codex_doc();
        assert_eq!(doc["model"].as_str(), Some("gpt-a"));
        assert_eq!(doc["model_context_window"].as_integer(), Some(200000));
        assert_eq!(
            doc["agents"]["default_subagent_model"].as_str(),
            Some("gpt-a-mini")
        );
        assert_eq!(codex_user_parts(&on_a).len(), 6, "{on_a}");

        // 第二轮往返字节稳定。
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b again");
        assert_eq!(codex_text(), on_b);
        ProviderService::switch(&state, AppType::Codex, "a").expect("to a again");
        assert_eq!(codex_text(), on_a);
    }

    #[tokio::test]
    #[serial]
    async fn codex_exclusive_fields_the_user_changed_stay() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        let edited = CODEX_USER_LIVE.replace(
            "model_context_window = 200000",
            "model_context_window = 150000",
        );
        seed_codex(&edited, None);

        ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
        assert_eq!(
            codex_doc()["model_context_window"].as_integer(),
            Some(150000),
            "a value the user changed is not A's to remove"
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_official_switch_leaves_a_dormant_route_table() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
        let [a, b] = codex_a_b();
        let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;

        ProviderService::switch(
            &state,
            AppType::Codex,
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        )
        .expect("switch to official");
        let text = codex_text();
        let doc = codex_doc();
        assert!(doc.get("model_provider").is_none(), "{text}");
        let dormant = &doc["model_providers"]["custom"];
        assert_eq!(
            dormant["base_url"].as_str(),
            Some("http://127.0.0.1:15721/v1"),
            "the dormant table points at the configured local proxy: {text}"
        );
        assert_eq!(
            dormant["experimental_bearer_token"].as_str(),
            Some(PROXY_TOKEN_PLACEHOLDER)
        );
        assert!(dormant.get("name").is_some(), "Codex loads it: {text}");
        assert!(!text.contains("sk-a"), "no real key stays behind: {text}");
        assert_eq!(codex_user_parts(&text).len(), 6, "{text}");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
            chatgpt_login("acct"),
            "the official login is untouched"
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_an_active_profile_overriding_the_route_is_refused_without_side_effects() {
        let _home = Home::new();
        set_preservation(true);
        let live = format!(
            "profile = \"work\"\n{CODEX_USER_LIVE}\n[profiles.work]\nmodel_provider = \"ollama_local\"\n"
        );
        seed_codex(&live, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        let mtime = fs::metadata(codex_config_path())
            .unwrap()
            .modified()
            .unwrap();

        let err = ProviderService::switch(&state, AppType::Codex, "b").expect_err("refused");
        assert!(err.to_string().contains("work"), "{err}");
        assert_eq!(codex_text(), live);
        assert_eq!(
            fs::metadata(codex_config_path())
                .unwrap()
                .modified()
                .unwrap(),
            mtime
        );
        assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("a"));
    }

    #[tokio::test]
    #[serial]
    async fn codex_migration_retires_only_tables_cc_switch_wrote() {
        let _home = Home::new();
        set_preservation(true);
        // 旧版按行的 id 整份写进来的表：a（id 和地址都对得上 a 的行）、b 的地址被用户改过、
        // 被 profile 引用的 c、代理占位残留、用户自己的 ollama_local。
        let live = r#"model_provider = "a"
model = "gpt-a"

[model_providers.a]
name = "a"
base_url = "https://a.example/v1"
experimental_bearer_token = "sk-a"

[model_providers.b]
name = "b"
base_url = "https://my-own-b.example/v1"

[model_providers.c]
name = "c"
base_url = "https://c.example/v1"

[model_providers.deepseek]
name = "deepseek"
base_url = "http://127.0.0.1:15721/v1"
experimental_bearer_token = "PROXY_MANAGED"

[model_providers.ollama_local]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[profiles.side]
model_provider = "c"
"#;
        seed_codex(live, None);
        let [a, b] = codex_a_b();
        let c = codex_row("c", "https://c.example/v1", "");
        let state = state_with(AppType::Codex, &[a, b, c], "a").await;

        ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
        let text = codex_text();
        let providers = codex_doc()["model_providers"].as_table().unwrap().clone();
        assert!(!providers.contains_key("a"), "provably ours: {text}");
        assert!(
            !providers.contains_key("deepseek"),
            "placeholder leftover: {text}"
        );
        assert!(
            providers.contains_key("b"),
            "address differs, not provably ours"
        );
        assert!(providers.contains_key("c"), "a profile still selects it");
        assert!(
            providers.contains_key("ollama_local"),
            "the user's own table"
        );
        assert!(!text.contains("sk-a"), "{text}");
    }

    #[tokio::test]
    #[serial]
    async fn codex_switch_crash_rolls_every_file_forward() {
        let _home = Home::new();
        set_preservation(false);
        seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
        let [a, mut b] = codex_a_b();
        b.settings_config["modelCatalog"] = json!({ "models": [{ "model": "gpt-b" }] });
        let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;
        ProviderService::switch(
            &state,
            AppType::Codex,
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        )
        .expect("official");

        // 官方 → b：删 auth.json（暂存登录）、改 config.toml、写模型目录，一起提交。
        for point in ["published:0", "published:1", "published:2", "target"] {
            ProviderService::switch(
                &state,
                AppType::Codex,
                crate::database::CODEX_OFFICIAL_PROVIDER_ID,
            )
            .expect("reset to official");
            assert!(codex_auth_path().exists(), "{point}: login restored");
            failpoint::crash_at(Some(point));
            let crashed = ProviderService::switch(&state, AppType::Codex, "b");
            failpoint::crash_at(None);
            assert!(crashed.is_err(), "{point}");

            crate::mode::operation::recover_on_startup(&state.db);
            assert!(!codex_auth_path().exists(), "{point}: auth.json deleted");
            assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"), "{point}");
            assert!(
                crate::codex_config::get_codex_model_catalog_path().exists(),
                "{point}: catalog written"
            );
            assert_eq!(
                direct(&state, &AppType::Codex).as_deref(),
                Some("b"),
                "{point}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn codex_preservation_off_gives_the_login_back_on_the_way_to_official() {
        let _home = Home::new();
        set_preservation(false);
        seed_codex("", Some(&chatgpt_login("acct")));
        let [a, b] = codex_a_b();
        let official = codex_official();
        let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
        let login = || -> Option<Value> {
            fs::read(codex_auth_path())
                .ok()
                .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        };

        ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
        assert_eq!(login(), None, "no login next to a third-party route");
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
        assert_eq!(
            login(),
            Some(chatgpt_login("acct")),
            "the same login comes back"
        );
        let row = state
            .db
            .get_provider_by_id(&official.id, "codex")
            .unwrap()
            .unwrap();
        assert_eq!(
            row.settings_config["auth"],
            json!({}),
            "the login never goes into the row (it would sync to the cloud)"
        );

        // 在官方卡上登出后切走再切回：保持登出。
        fs::remove_file(codex_auth_path()).unwrap();
        ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
        assert_eq!(login(), None, "logging out sticks");
    }

    #[tokio::test]
    #[serial]
    async fn codex_keyring_logins_keep_requires_openai_auth_on_the_preservation_setting() {
        let _home = Home::new();
        for preserve in [true, false] {
            set_preservation(preserve);
            seed_codex("cli_auth_credentials_store = \"keyring\"\n", None);
            let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
            ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
            let doc = codex_doc();
            assert_eq!(
                doc["model_providers"]["custom"]["requires_openai_auth"].as_bool(),
                Some(preserve),
                "the login lives in the keyring, auth.json says nothing (preserve={preserve})"
            );
            assert_eq!(doc["cli_auth_credentials_store"].as_str(), Some("keyring"));
        }
    }

    #[tokio::test]
    #[serial]
    async fn codex_route_switch_with_the_same_contract_leaves_the_client_files_alone() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        // b、c 在客户端看来一样（同一个模型名，没有独有字段），只是上游和 Key 不同。
        let [a, b] = codex_a_b();
        let mut c = codex_row("c", "https://c.example/v1", "");
        c.settings_config["config"] = json!(c.settings_config["config"]
            .as_str()
            .unwrap()
            .replace("gpt-c", "gpt-b"));
        // d 和 b 只差模型名。
        let d = codex_row("d", "https://d.example/v1", "");
        let state = state_with(AppType::Codex, &[a, b, c, d], "b").await;
        enter(&state, &AppType::Codex).await.expect("enter");
        let entered = codex_text();
        assert!(entered.contains(PROXY_TOKEN_PLACEHOLDER), "{entered}");
        assert!(!entered.contains("sk-b"), "{entered}");
        let mtime = fs::metadata(codex_config_path())
            .unwrap()
            .modified()
            .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(codex_config_path(), fs::Permissions::from_mode(0o000)).unwrap();
        }
        ProviderService::switch(&state, AppType::Codex, "c").expect("switch route");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(codex_config_path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(codex_text(), entered);
        assert_eq!(
            fs::metadata(codex_config_path())
                .unwrap()
                .modified()
                .unwrap(),
            mtime
        );
        assert_eq!(in_use(&state, &AppType::Codex).as_deref(), Some("c"));

        // 换到只有模型名不同的 d：契约变了，客户端先改写。
        ProviderService::switch(&state, AppType::Codex, "d").expect("route to d");
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-d"));
        // 换到独有字段也不同的 a：同样改写。
        ProviderService::switch(&state, AppType::Codex, "a").expect("route to a");
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"));
        assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("b"));

        exit(&state, &AppType::Codex).await.expect("exit");
        let back = codex_text();
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"));
        assert!(
            back.contains("sk-b") && !back.contains(PROXY_TOKEN_PLACEHOLDER),
            "{back}"
        );
        assert_eq!(codex_user_parts(&back).len(), 6, "{back}");
    }

    #[tokio::test]
    #[serial]
    async fn codex_editor_saves_key_fields_to_the_row_and_global_edits_to_live() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        let save = |id: &str, settings: Value, base: Value| {
            let mut row = state.db.get_provider_by_id(id, "codex").unwrap().unwrap();
            row.settings_config = settings;
            ProviderService::update_from_editor(
                &state,
                AppType::Codex,
                Some(id),
                row,
                Some(crate::services::provider::EditorSave {
                    base,
                    on_conflict: Default::default(),
                }),
            )
        };

        // 编辑非当前的 b：显示的是切到 b 之后的 config.toml（Key 在输入框里，不在 TOML 里）。
        let b_row = state.db.get_provider_by_id("b", "codex").unwrap().unwrap();
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &b_row.settings_config, None)
                .expect("view b");
        let shown = view.settings["config"].as_str().unwrap().to_string();
        assert!(
            shown.contains("gpt-b") && shown.contains("https://b.example/v1"),
            "{shown}"
        );
        assert!(!shown.contains("sk-b"), "{shown}");
        assert_eq!(codex_user_parts(&shown).len(), 6, "{shown}");

        let mut edited = view.settings.clone();
        edited["config"] = json!(
            shown
                .replace("\"on-request\"", "\"never\"")
                .replace("\"gpt-b\"", "\"gpt-b2\"")
                + "\n[mcp_servers.git]\ncommand = \"git\"\n"
        );
        save("b", edited, view.settings.clone()).expect("save b");
        let live = codex_text();
        assert!(
            live.contains("approval_policy = \"never\""),
            "global edit applied: {live}"
        );
        assert!(live.contains("[mcp_servers.git]"), "{live}");
        assert_eq!(
            codex_doc()["model"].as_str(),
            Some("gpt-a"),
            "b is not current: {live}"
        );
        let b_row = state.db.get_provider_by_id("b", "codex").unwrap().unwrap();
        let b_config = b_row.settings_config["config"].as_str().unwrap();
        assert!(
            b_config.contains("gpt-b2") && !b_config.contains("approval_policy"),
            "{b_config}"
        );

        // 编辑当前的 a：关键字段立刻换进 live。
        let a_row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &a_row.settings_config, None)
                .expect("view a");
        let mut edited = view.settings.clone();
        edited["config"] = json!(view.settings["config"]
            .as_str()
            .unwrap()
            .replace("\"gpt-a\"", "\"gpt-a2\""));
        save("a", edited, view.settings.clone()).expect("save a");
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a2"));
        assert!(codex_text().contains("[mcp_servers.git]"));

        // 打开编辑器之后别的程序改了同一个键：保存时报冲突，什么都不写。
        let a_row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &a_row.settings_config, None)
                .expect("view a again");
        let outside = codex_text().replace("\"never\"", "\"untrusted\"");
        fs::write(codex_config_path(), &outside).unwrap();
        let mut edited = view.settings.clone();
        edited["config"] = json!(view.settings["config"]
            .as_str()
            .unwrap()
            .replace("\"never\"", "\"on-failure\""));
        let err = save("a", edited, view.settings.clone()).expect_err("conflict");
        assert!(
            err.to_string()
                .contains(crate::live::patch::EDIT_CONFLICT_CODE),
            "{err}"
        );
        assert_eq!(codex_text(), outside);
    }

    #[tokio::test]
    #[serial]
    async fn codex_a_login_refreshed_during_the_switch_is_never_overwritten() {
        let _home = Home::new();
        set_preservation(false);
        seed_codex("", Some(&chatgpt_login("acct")));
        let [a, _] = codex_a_b();
        let official = codex_official();
        let state = state_with(AppType::Codex, &[a, official.clone()], &official.id).await;
        let config_before = codex_text();

        // 计划删掉 auth.json 之后、发布之前，Codex CLI 刷新了登录。
        let mut refreshed = chatgpt_login("acct");
        refreshed["tokens"]["refresh_token"] = json!("refresh-acct-2");
        let fresh = refreshed.to_string();
        failpoint::on_before_publish(Some(Box::new(move |_, path: &std::path::Path| {
            if path == codex_auth_path() {
                fs::write(path, &fresh).unwrap();
            }
        })));
        let result = ProviderService::switch(&state, AppType::Codex, "a");
        failpoint::on_before_publish(None);

        assert!(
            result.is_err(),
            "the switch stops instead of deleting a newer login"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
            refreshed
        );
        assert_eq!(codex_text(), config_before, "nothing else was published");
        assert_eq!(
            direct(&state, &AppType::Codex).as_deref(),
            Some(official.id.as_str())
        );
        assert!(
            state::pending(&DeviceStore::for_device(), "codex")
                .unwrap()
                .is_none(),
            "an operation that never published leaves no pending"
        );
    }
}
