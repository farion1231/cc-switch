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
//! 四个应用的客户端文件都经写入引擎写，只改关键字段（和独有字段），文件和模式状态在
//! 同一个操作里提交，崩溃后按 pending 前滚。进入代理前不回填：直连供应商的行不会因为
//! 进出代理而改变。
//!
//! 调用方持有这个应用的代理切换锁（`ProxyService::lock_switch_for_app`）；写引擎的应用
//! 写锁在更里面拿，两把锁不反向嵌套。

use serde_json::{json, Value};
use tokio::sync::OwnedMutexGuard;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::live::project::claude::{
    direct_patch, proxy_projection, ClaudeProjection, ProxyAuth, PROXY_TOKEN_PLACEHOLDER,
};
use crate::live::project::gemini::GeminiProjection;
use crate::live::project::grok::GrokProjection;
use crate::provider::Provider;
use crate::services::provider::codex_direct::{self, Owner};
use crate::services::provider::{claude_direct, gemini_direct, grok_direct};
use crate::store::AppState;

use super::contract;
use super::current::{self, Purpose};
use super::operation;
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
    current::direct_provider(&state.db, app).map_err(err)
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

    /// 客户端文件现在就是这份契约（`key` 相同）。
    fn has_contract(&self, key: &str) -> bool {
        matches!(self, Self::Proxy { contract: Some(contract), .. } if contract.key == key)
    }

    /// live 现在是哪个直连供应商写进去的（Grok 在没有写入记录时据此推断要删的表）。
    fn direct_owner(&self) -> Option<&Provider> {
        match self {
            Self::Direct(provider) => provider.as_ref(),
            Self::Proxy { .. } => None,
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

/// 只落定状态，不碰客户端文件（未接上时换路由、故障转移记下新路由等）。
fn commit_state(state: &AppState, app: &AppType, target: &PendingTarget) -> Result<(), String> {
    operation::commit_target(&state.db, &DeviceStore::for_device(), app.as_str(), target)
        .map_err(err)
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
    let (proxy_url, codex_base_url) = state.proxy_service.build_proxy_urls().await?;
    let force = op_name == op::ATTACH;
    match app {
        AppType::Claude => {
            let (projection, contract) = claude_contract(route, &proxy_url);
            let unchanged = !force && live_now.has_contract(&contract.key);
            let patch = direct_patch(live_now.claude_exclusive_owner().as_ref(), &projection);
            target.contract = Some(contract);
            claude_direct::run(
                &state.db,
                op_name,
                (!unchanged).then_some(&patch),
                PendingTarget::mode(target),
            )
            .map_err(err)?;
        }
        AppType::Gemini => {
            let projection = GeminiProjection::proxy_contract(
                &GeminiProjection::of(&route.settings_config, false),
                &proxy_url,
                PROXY_TOKEN_PLACEHOLDER,
            );
            let contract = contract::gemini(&projection);
            let unchanged = !force && live_now.has_contract(&contract.key);
            target.contract = Some(contract);
            gemini_direct::run(
                &state.db,
                op_name,
                (!unchanged).then_some(&projection),
                PendingTarget::mode(target),
            )
            .map_err(err)?;
        }
        AppType::Codex => {
            let owner = live_now.codex_owner();
            let spec = codex_direct::Target::Proxy {
                route,
                base_url: &codex_base_url,
            };
            let prepared =
                codex_direct::prepare(&state.codex_oauth_manager, &owner, &spec).map_err(err)?;
            let planned = codex_direct::plan(&state.db, &owner, &spec, &prepared).map_err(err)?;
            let unchanged = !force && live_now.has_contract(&planned.contract.key);
            target.contract = Some(planned.contract.clone());
            let pending = PendingTarget::mode(target);
            if unchanged {
                commit_state(state, app, &pending)?;
            } else {
                codex_direct::run(&state.db, op_name, planned, &prepared, pending).map_err(err)?;
            }
        }
        AppType::GrokBuild => {
            let base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));
            let projection = GrokProjection::proxy_contract(
                &grok_direct::projection(route).map_err(err)?,
                &base_url,
                PROXY_TOKEN_PLACEHOLDER,
            )
            .map_err(err)?;
            let contract = contract::grok(&projection);
            let unchanged = !force && live_now.has_contract(&contract.key);
            target.contract = Some(contract);
            grok_direct::run(
                &state.db,
                op_name,
                live_now.direct_owner(),
                (!unchanged).then_some(&projection),
                PendingTarget::mode(target),
            )
            .map_err(err)?;
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
    let pending_target = PendingTarget::mode(target);
    let attached = matches!(live_now, LiveNow::Proxy { .. });
    match app {
        AppType::Claude => {
            let empty = ClaudeProjection::default();
            let projection = usable_direct(app, direct.as_ref())
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
            let target = usable_direct(app, direct.as_ref());
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
        AppType::Gemini => {
            let projection = attached.then(|| {
                direct_or_empty(
                    app,
                    direct.as_ref(),
                    gemini_direct::projection,
                    GeminiProjection::empty,
                )
            });
            gemini_direct::run(&state.db, op_name, projection.as_ref(), pending_target)
                .map_err(err)?;
        }
        AppType::GrokBuild => {
            let projection = attached.then(|| {
                direct_or_empty(app, direct.as_ref(), grok_direct::projection, || {
                    GrokProjection { table: None }
                })
            });
            grok_direct::run(
                &state.db,
                op_name,
                None,
                projection.as_ref(),
                pending_target,
            )
            .map_err(err)?;
        }
        _ => return Err(format!("{} 不支持本地路由", app.as_str())),
    }
    Ok(())
}

/// 直连供应商的投影。写不出来（没有、行里带着占位符、缺 Key）也不能让客户端一直指着
/// 代理：退一步只清空关键字段（`empty`）。
fn direct_or_empty<P>(
    app: &AppType,
    direct: Option<&Provider>,
    project: impl FnOnce(&Provider) -> Result<P, AppError>,
    empty: impl FnOnce() -> P,
) -> P {
    usable_direct(app, direct)
        .map(project)
        .transpose()
        .unwrap_or_else(|error| {
            log::warn!(
                "写回直连的 {} 配置失败，只清空关键字段: {error}",
                app.as_str()
            );
            None
        })
        .unwrap_or_else(empty)
}

/// 能照写回 live 的直连供应商：行里本身带着占位符（旧版接管期间被导入的残留）的不行，
/// 否则客户端会一直指着已经不在的本地代理。
fn usable_direct<'a>(app: &AppType, direct: Option<&'a Provider>) -> Option<&'a Provider> {
    direct.filter(|provider| {
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
}

fn require_proxy_app(app: &AppType) -> Result<(), String> {
    if app.supports_local_proxy() {
        Ok(())
    } else {
        Err(format!("{} 不支持本地路由", app.as_str()))
    }
}

/// 拿这个应用的代理切换锁，再补完它上一次没做完的写入。之后读到的模式、路由和直连
/// 指针都是落定过的。写入函数在写锁里发现还有没补完的操作会补完后拒绝这次写入（见
/// `operation::recover_before_write`），入口先补完，用户就不用重试一次。
///
/// 补不完（比如本机设置文件写不进去、改不了指针）就拒绝这次操作：这时读到的还是补完前的
/// 指针和模式，照着做下去（比如只存了一行、以为它不是当前供应商），等那次操作补完就和刚
/// 做的对不上了。
pub(crate) async fn lock_settled(
    state: &AppState,
    app: &AppType,
) -> Result<OwnedMutexGuard<()>, AppError> {
    let guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
    operation::settle(&state.db, app.as_str()).map_err(|error| {
        AppError::localized(
            "mode.unsettled",
            format!(
                "{} 上一次写配置文件的操作没做完，现在也补不完：{error}。本次什么都没做，请排查后重试",
                app.as_str()
            ),
            format!(
                "The previous write to {}'s config files is unfinished and cannot be completed now: {error}. Nothing was done; fix the cause and retry",
                app.as_str()
            ),
        )
    })?;
    Ok(guard)
}

/// 同步代码里用的 [`lock_settled`]。不支持代理的应用没有模式可以被并发改掉，不拿锁。
pub(crate) fn lock_settled_blocking(
    state: &AppState,
    app: &AppType,
) -> Result<Option<OwnedMutexGuard<()>>, AppError> {
    if !app.supports_local_proxy() {
        return Ok(None);
    }
    futures::executor::block_on(lock_settled(state, app)).map(Some)
}

/// 进入代理模式。
pub async fn enter(state: &AppState, app: &AppType) -> Result<(), String> {
    require_proxy_app(app)?;
    let result = match lock_settled(state, app).await {
        Ok(_guard) => enter_locked(state, app, op::ENTER).await,
        Err(error) => Err(error.to_string()),
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
        let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
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
    if current::proxy_flags(PROXY_APPS).contains(&true) {
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
        let result = match lock_settled(state, &app).await {
            Ok(_guard) => exit_locked(state, &app, false),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
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
        let result = match lock_settled(state, &app).await {
            Ok(_guard) => exit_locked(state, &app, true),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
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
        commit_state(state, app, &PendingTarget::mode(new_state))?;
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
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
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
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
    resync_route_locked(state, app).await
}

/// 代理换了地址之后，按新地址重写每个接上代理的应用。一个应用失败（比如配置文件解析
/// 不了）不影响其余应用：旧地址已经没人监听，跳过的应用会一直连不上。失败的汇总报错。
pub async fn resync_routes(state: &AppState) -> Result<(), String> {
    let mut failures = Vec::new();
    for app in PROXY_APPS {
        if let Err(error) = resync_route(state, &app).await {
            log::warn!("按新的代理地址重写 {} 失败: {error}", app.as_str());
            failures.push(format!("{}: {error}", app.as_str()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// 故障转移成功后记下新路由：只换代理的上游，不写客户端文件（契约兼容性本轮不检查）。
/// 返回路由是否真的变了。
pub async fn record_failover_route(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
) -> Result<bool, String> {
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
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
        let result = match lock_settled(state, &app).await {
            Ok(_guard) => startup_app(state, &app).await,
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
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
        commit_state(state, app, &PendingTarget::mode(mode))?;
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
    use crate::services::provider::ProviderService;
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

    /// 保存当前供应商要等进入代理写完契约，之后按代理模式处理，不能拿进入前读到的直连
    /// 模式把关键字段写回 live。
    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn saving_the_current_provider_waits_for_entering_proxy_mode() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state: &'static AppState = Box::leak(Box::new(
            state_with(
                AppType::Claude,
                &[claude("a", "https://a.example", json!({}))],
                "a",
            )
            .await,
        ));

        let guard = lock_settled(state, &AppType::Claude).await.unwrap();
        let updater = std::thread::spawn(move || {
            ProviderService::update(
                state,
                AppType::Claude,
                None,
                claude("a", "https://a2.example", json!({})),
            )
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!updater.is_finished(), "the save waits for the switch lock");
        assert_eq!(settings()["env"]["ANTHROPIC_BASE_URL"], "https://a.example");

        enter_locked(state, &AppType::Claude, op::ENTER)
            .await
            .expect("enter");
        drop(guard);
        tokio::task::spawn_blocking(move || updater.join())
            .await
            .unwrap()
            .unwrap()
            .expect("save");

        let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        let live = settings();
        assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], proxy_url.as_str());
        assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], PROXY_TOKEN_PLACEHOLDER);

        exit(state, &AppType::Claude).await.expect("exit");
        assert_eq!(
            settings()["env"]["ANTHROPIC_BASE_URL"],
            "https://a2.example",
            "the saved row is what leaving proxy mode writes back"
        );

        // 同步当前供应商（导入、云同步、统一供应商）同样等进入代理写完。
        let guard = lock_settled(state, &AppType::Claude).await.unwrap();
        let syncer = std::thread::spawn(move || {
            ProviderService::sync_current_provider_for_app(state, AppType::Claude)
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!syncer.is_finished(), "the sync waits for the switch lock");
        enter_locked(state, &AppType::Claude, op::ENTER)
            .await
            .expect("enter");
        drop(guard);
        tokio::task::spawn_blocking(move || syncer.join())
            .await
            .unwrap()
            .unwrap()
            .expect("sync");
        assert_eq!(
            settings()["env"]["ANTHROPIC_AUTH_TOKEN"],
            PROXY_TOKEN_PLACEHOLDER
        );
        exit(state, &AppType::Claude).await.expect("exit");
    }

    /// 上一次的操作补不完（这里是落定状态失败）时，保存编辑器直接拒绝、行不动。否则按补完
    /// 之前的指针判断「b 不是当前供应商」只存了行，等那次操作补完 b 成了当前供应商，live
    /// 里却是它的旧 Key。
    #[tokio::test]
    #[serial]
    async fn nothing_is_saved_while_the_previous_write_cannot_be_finished() {
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
        failpoint::crash_at(Some("published:0"));
        let interrupted = ProviderService::switch(&state, AppType::Claude, "b");
        failpoint::crash_at(None);
        assert!(interrupted.is_err());

        let mut row = state.db.get_provider_by_id("b", "claude").unwrap().unwrap();
        let base =
            ProviderService::editor_view(&state, AppType::Claude, &row.settings_config, None)
                .expect("view")
                .settings;
        let mut edited = base.clone();
        edited["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("sk-b-new");
        row.settings_config = edited;
        failpoint::crash_at(Some("recover:target"));
        let refused = ProviderService::update_from_editor(
            &state,
            AppType::Claude,
            None,
            row,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        );
        failpoint::crash_at(None);
        let error = refused.expect_err("refused while unsettled");
        assert!(error.to_string().contains("补不完"), "{error}");
        let b_token = |state: &AppState| {
            state
                .db
                .get_provider_by_id("b", "claude")
                .unwrap()
                .unwrap()
                .settings_config["env"]["ANTHROPIC_AUTH_TOKEN"]
                .clone()
        };
        assert_eq!(b_token(&state), "sk-b", "the row is untouched");

        crate::mode::operation::recover_on_startup(&state.db);
        assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("b"));
        assert_eq!(settings()["env"]["ANTHROPIC_AUTH_TOKEN"], b_token(&state));
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

    /// 代理换了端口：一个应用重写失败（配置文件解析不了），其余接上代理的应用照样按新
    /// 地址重写，失败的应用在报错里。
    #[tokio::test]
    #[serial]
    async fn a_new_proxy_address_reaches_every_app_even_if_one_fails() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        seed_gemini(
            "GEMINI_API_KEY=real-key\nGOOGLE_GEMINI_BASE_URL=https://g.example\n",
            "{}",
        );
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;
        let row = gemini(
            "g",
            json!({ "GEMINI_API_KEY": "real-key", "GOOGLE_GEMINI_BASE_URL": "https://g.example" }),
            json!({}),
        );
        state.db.save_provider("gemini", &row).unwrap();
        state.db.set_current_provider("gemini", "g").unwrap();
        crate::settings::set_current_provider(&AppType::Gemini, Some("g")).unwrap();
        enter(&state, &AppType::Claude).await.expect("enter claude");
        enter(&state, &AppType::Gemini).await.expect("enter gemini");
        let old_url = state.proxy_service.build_proxy_urls().await.unwrap().0;

        fs::write(settings_path(), "not json").unwrap();
        let mut config = state.db.get_proxy_config().await.unwrap();
        config.listen_port = 0;
        assert!(state.proxy_service.update_config(&config).await.unwrap());
        let new_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        assert_ne!(new_url, old_url);

        let error = resync_routes(&state).await.expect_err("claude fails");
        assert!(error.starts_with("claude:"), "{error}");
        assert!(
            gemini_env().contains(&format!("GOOGLE_GEMINI_BASE_URL={new_url}\n")),
            "{}",
            gemini_env()
        );
        state.proxy_service.stop().await.unwrap();
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

    /// 行里自己指定的模型目录指针跟着这一家走：切走时删掉，切到生成了目录的那家就换成
    /// CC Switch 自己的指针；代理契约带进来的，退出代理时同样删掉。用户直接写进 live 的
    /// 指针一直留着。
    #[tokio::test]
    #[serial]
    async fn codex_a_row_catalog_pointer_leaves_with_its_provider() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let a = codex_row(
            "a",
            "https://a.example/v1",
            "model_catalog_json = \"/work/a-catalog.json\"",
        );
        let mut b = codex_row("b", "https://b.example/v1", "");
        b.settings_config["modelCatalog"] = json!({ "models": [{ "model": "gpt-b" }] });
        let c = codex_row("c", "https://c.example/v1", "");
        let state = state_with(AppType::Codex, &[a, b, c], "c").await;
        let pointer = || {
            codex_doc()
                .get("model_catalog_json")
                .and_then(|value| value.as_str().map(str::to_string))
        };
        let ours = crate::live::project::codex::CATALOG_FILENAME;

        ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
        assert_eq!(pointer().as_deref(), Some("/work/a-catalog.json"));
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        assert_eq!(pointer().as_deref(), Some(ours), "{}", codex_text());
        ProviderService::switch(&state, AppType::Codex, "a").expect("back to a");
        ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
        assert_eq!(pointer(), None, "{}", codex_text());

        // 代理模式：路由从 c 换到 a，契约带进 a 的指针；退出代理写回直连 c 时删掉。
        enter(&state, &AppType::Codex).await.expect("enter");
        ProviderService::switch(&state, AppType::Codex, "a").expect("route to a");
        assert_eq!(pointer().as_deref(), Some("/work/a-catalog.json"));
        exit(&state, &AppType::Codex).await.expect("exit");
        assert_eq!(pointer(), None, "{}", codex_text());

        // 用户自己写进 live 的指针不认领、不删，也不被 CC Switch 的指针替换。
        let with_user = format!("model_catalog_json = \"/work/mine.json\"\n{}", codex_text());
        fs::write(codex_config_path(), with_user).unwrap();
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        assert_eq!(pointer().as_deref(), Some("/work/mine.json"));
        ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
        assert_eq!(pointer().as_deref(), Some("/work/mine.json"));
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

    /// 生效的 profile 显式选了内置的 `openai`：和官方卡不写 model_provider 去的是同一个
    /// 地方，切到官方卡不拒绝；切到第三方仍然拒绝。
    #[tokio::test]
    #[serial]
    async fn codex_a_profile_selecting_the_built_in_openai_allows_the_official_card() {
        let _home = Home::new();
        set_preservation(true);
        let live = format!(
            "profile = \"work\"\n{CODEX_USER_LIVE}\n[profiles.work]\nmodel_provider = \"openai\"\n"
        );
        seed_codex(&live, Some(&chatgpt_login("acct")));
        let [a, b] = codex_a_b();
        let state = state_with(AppType::Codex, &[a, b, codex_official()], "a").await;

        ProviderService::switch(
            &state,
            AppType::Codex,
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
        )
        .expect("switch to official");
        let doc = codex_doc();
        assert!(doc.get("model_provider").is_none());
        assert_eq!(
            doc["profiles"]["work"]["model_provider"].as_str(),
            Some("openai")
        );

        let err = ProviderService::switch(&state, AppType::Codex, "b").expect_err("refused");
        assert!(err.to_string().contains("work"), "{err}");
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

    fn codex_login_on_disk() -> Value {
        crate::config::read_json_file(&codex_auth_path()).unwrap()
    }

    /// 官方 → a：删掉 auth.json 之后失败。登录只在暂存的临时文件里，指针没动。
    async fn codex_switch_interrupted_after_auth_json() -> (AppState, Provider) {
        set_preservation(false);
        seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
        let [a, b] = codex_a_b();
        let official = codex_official();
        let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("official");
        failpoint::crash_at(Some("published:0"));
        let failed = ProviderService::switch(&state, AppType::Codex, "a");
        failpoint::crash_at(None);
        assert!(failed.is_err());
        assert!(!codex_auth_path().exists());
        assert_eq!(
            direct(&state, &AppType::Codex).as_deref(),
            Some(official.id.as_str())
        );
        (state, official)
    }

    #[tokio::test]
    #[serial]
    async fn codex_a_retry_after_a_failed_switch_finishes_it_first() {
        let _home = Home::new();
        let (state, official) = codex_switch_interrupted_after_auth_json().await;

        // 重试切到 b：先补完到 a，再按补完后的 auth.json 和暂存从 a 切到 b。
        ProviderService::switch(&state, AppType::Codex, "b").expect("retry");
        assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("b"));
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-b"));
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
        assert_eq!(codex_login_on_disk(), chatgpt_login("acct"));
    }

    #[tokio::test]
    #[serial]
    async fn codex_an_interrupted_switch_keeps_the_login_when_codex_changed_config_toml() {
        let _home = Home::new();
        let (state, official) = codex_switch_interrupted_after_auth_json().await;
        // 补完之前 Codex 自己改了 config.toml（信任了一个新项目）。
        let mut text = codex_text();
        text.push_str("\n[projects.\"/new\"]\ntrust_level = \"trusted\"\n");
        fs::write(codex_config_path(), &text).unwrap();

        crate::mode::operation::recover_on_startup(&state.db);
        assert_eq!(
            direct(&state, &AppType::Codex).as_deref(),
            Some("a"),
            "the pointer follows the auth.json already deleted"
        );
        assert_eq!(codex_text(), text, "Codex's own change is left alone");
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("to official");
        assert_eq!(
            codex_login_on_disk(),
            chatgpt_login("acct"),
            "the login made it into the stash"
        );
    }

    /// 发布过的文件在补完之前又被客户端改掉（这里是用户在 Codex 里重新登录、写了新的
    /// auth.json）：单看文件分不出发布开始过没有，按 pending 里的「已开始发布」照样前滚，
    /// 还没写出去的登录暂存不能丢，客户端写的 auth.json 不动。
    #[tokio::test]
    #[serial]
    async fn codex_an_interrupted_switch_keeps_the_stash_when_the_published_file_changed_again() {
        let _home = Home::new();
        let (state, _official) = codex_switch_interrupted_after_auth_json().await;
        fs::write(codex_auth_path(), chatgpt_login("other").to_string()).unwrap();

        crate::mode::operation::recover_on_startup(&state.db);
        assert_eq!(direct(&state, &AppType::Codex).as_deref(), Some("a"));
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"));
        let stash = fs::read_to_string(DeviceStore::for_device().file("codex-login-stash.json"))
            .expect("the stash was published");
        assert!(stash.contains("refresh-acct"), "{stash}");
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(codex_auth_path()).unwrap()).unwrap(),
            chatgpt_login("other")
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_an_unreadable_login_stash_is_never_overwritten() {
        let _home = Home::new();
        set_preservation(false);
        seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
        let stash = DeviceStore::for_device().file("codex-login-stash.json");
        fs::create_dir_all(stash.parent().unwrap()).unwrap();
        let broken = br#"{"logins":{"account:old":{"tokens":{"refresh_token":"salvageable"}}},"#;
        fs::write(&stash, broken).unwrap();
        let [a, b] = codex_a_b();
        let official = codex_official();
        let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;

        // 切到第三方要把 auth.json 里的登录存进暂存：停下，什么都不写。
        let err = ProviderService::switch(&state, AppType::Codex, "a").expect_err("refused");
        assert!(err.to_string().contains("codex-login-stash.json"), "{err}");
        assert_eq!(fs::read(&stash).unwrap(), broken);
        assert_eq!(codex_login_on_disk(), chatgpt_login("acct"));
        assert_eq!(codex_text(), CODEX_USER_LIVE);

        // 用不着暂存的切换照常。
        fs::remove_file(codex_auth_path()).unwrap();
        ProviderService::switch(&state, AppType::Codex, "b").expect("nothing to stash");
        assert_eq!(fs::read(&stash).unwrap(), broken);
    }

    #[tokio::test]
    #[serial]
    async fn codex_official_routes_for_different_accounts_swap_the_login() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex("", Some(&chatgpt_login("acct-a")));
        // 两张没绑托管账号的官方卡，行里各存着一个账号（旧版回填的，暂存第一次建立时
        // 收进去）。
        let official = |id: &str| {
            let mut row = Provider::with_id(
                id.to_string(),
                id.to_uppercase(),
                json!({ "auth": chatgpt_login(id), "config": "" }),
                None,
            );
            row.category = Some("official".to_string());
            row
        };
        let state = state_with(
            AppType::Codex,
            &[official("acct-a"), official("acct-b")],
            "acct-a",
        )
        .await;
        let account = || codex_login_on_disk()["tokens"]["account_id"].clone();
        ProviderService::switch(&state, AppType::Codex, "acct-a").expect("direct a");
        assert_eq!(account(), json!("acct-a"));
        ProviderService::switch(&state, AppType::Codex, "acct-b").expect("direct b");
        assert_eq!(
            account(),
            json!("acct-b"),
            "the direct switch swaps accounts"
        );
        ProviderService::switch(&state, AppType::Codex, "acct-a").expect("direct a again");

        enter(&state, &AppType::Codex).await.expect("enter");
        switch_route(&state, &AppType::Codex, "acct-b")
            .await
            .expect("route to b");
        assert_eq!(
            account(),
            json!("acct-b"),
            "Codex signs in as the route's account"
        );
        switch_route(&state, &AppType::Codex, "acct-a")
            .await
            .expect("route back to a");
        assert_eq!(account(), json!("acct-a"));
        exit(&state, &AppType::Codex).await.expect("exit");
    }

    #[tokio::test]
    #[serial]
    async fn claude_a_retry_after_a_failed_switch_removes_the_failed_targets_exclusive_fields() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let a = claude("a", "https://a.example", json!({}));
        let b = claude(
            "b",
            "https://b.example",
            json!({ "CLAUDE_CODE_DISABLE_ARTIFACT": "1" }),
        );
        let c = claude("c", "https://c.example", json!({}));
        let state = state_with(AppType::Claude, &[a, b, c], "a").await;

        // 切到 b：settings.json 已经写好，指针落定前失败。
        failpoint::crash_at(Some("published:0"));
        let failed = ProviderService::switch(&state, AppType::Claude, "b");
        failpoint::crash_at(None);
        assert!(failed.is_err());
        assert_eq!(settings()["env"]["CLAUDE_CODE_DISABLE_ARTIFACT"], "1");
        assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("a"));

        // 重试切到 c：先补完到 b，再按 b 删它带进来的独有字段。
        ProviderService::switch(&state, AppType::Claude, "c").expect("retry");
        assert_eq!(direct(&state, &AppType::Claude).as_deref(), Some("c"));
        let env = &settings()["env"];
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://c.example");
        assert!(env.get("CLAUDE_CODE_DISABLE_ARTIFACT").is_none(), "{env}");
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
                    draft: None,
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

    /// live 里用户自己写的独有字段（`model_verbosity` 这类）不归当前供应商：原样保存不会
    /// 把它收进行，切走时也就不会删掉；在编辑器里删掉它就从 live 删；新加的独有字段归
    /// 供应商。
    #[tokio::test]
    #[serial]
    async fn codex_editor_leaves_exclusive_fields_from_live_to_the_user() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(
            &CODEX_USER_LIVE.replace(
                "model = \"gpt-a\"\n",
                "model = \"gpt-a\"\nmodel_verbosity = \"high\"\nmodel_supports_reasoning_summaries = true\n",
            ),
            None,
        );
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        let open = |id: &str| {
            let row = state.db.get_provider_by_id(id, "codex").unwrap().unwrap();
            let view =
                ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
                    .expect("view");
            (row, view.settings)
        };
        let save = |mut row: Provider, edited: Value, base: Value| {
            row.settings_config = edited;
            ProviderService::update_from_editor(
                &state,
                AppType::Codex,
                None,
                row,
                Some(crate::services::provider::EditorSave {
                    base,
                    draft: None,
                    on_conflict: Default::default(),
                }),
            )
        };
        let a_config = |state: &AppState| {
            state
                .db
                .get_provider_by_id("a", "codex")
                .unwrap()
                .unwrap()
                .settings_config["config"]
                .as_str()
                .unwrap()
                .to_string()
        };

        // 只改名、配置原样保存。
        let (mut row, base) = open("a");
        row.name = "renamed".into();
        save(row, base.clone(), base).expect("save as is");
        let config = a_config(&state);
        assert!(
            config.contains("model_context_window = 200000")
                && !config.contains("model_verbosity")
                && !config.contains("model_supports_reasoning_summaries"),
            "{config}"
        );

        // 删掉一个从 live 带进来的，再加一个供应商自己的。
        let (row, base) = open("a");
        let mut edited = base.clone();
        edited["config"] = json!(base["config"]
            .as_str()
            .unwrap()
            .replace("model_supports_reasoning_summaries = true\n", "")
            .replace(
                "model_verbosity",
                "model_auto_compact_token_limit = 100000\nmodel_verbosity"
            ));
        save(row, edited, base).expect("save edits");
        let live = codex_text();
        assert!(
            !live.contains("model_supports_reasoning_summaries")
                && live.contains("model_auto_compact_token_limit = 100000"),
            "{live}"
        );
        let config = a_config(&state);
        assert!(
            config.contains("model_auto_compact_token_limit = 100000")
                && !config.contains("model_verbosity"),
            "{config}"
        );

        ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
        let live = codex_doc();
        assert_eq!(live["model_verbosity"].as_str(), Some("high"));
        assert!(live.get("model_auto_compact_token_limit").is_none());
        assert!(live.get("model_context_window").is_none());
    }

    /// 打开编辑器之后客户端改了一个从 live 带进来的独有字段：用户没动它，保存时不收进行，
    /// 也不把打开时的值写回去。live 里生效的 profile 选着路由表时，编辑和新增都照样能存。
    #[tokio::test]
    #[serial]
    async fn codex_editor_leaves_what_the_user_did_not_touch_to_the_client() {
        let _home = Home::new();
        set_preservation(true);
        let live = CODEX_USER_LIVE.replace(
            "model = \"gpt-a\"\n",
            "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
        );
        seed_codex(
            &format!("profile = \"work\"\n{live}\n[profiles.work]\nmodel_provider = \"custom\"\n"),
            None,
        );
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

        let mut row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let base = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
            .expect("view")
            .settings;
        let changed =
            codex_text().replace("model_verbosity = \"high\"", "model_verbosity = \"low\"");
        fs::write(codex_config_path(), &changed).unwrap();
        row.name = "renamed".into();
        row.settings_config = base.clone();
        ProviderService::update_from_editor(
            &state,
            AppType::Codex,
            None,
            row,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("save with the profile active");
        let doc = codex_doc();
        assert_eq!(
            doc["model_verbosity"].as_str(),
            Some("low"),
            "{}",
            codex_text()
        );
        assert_eq!(
            doc["profiles"]["work"]["model_provider"].as_str(),
            Some("custom")
        );
        let stored = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        assert!(!stored.settings_config["config"]
            .as_str()
            .unwrap()
            .contains("model_verbosity"));

        let draft = codex_row("c", "https://c.example/v1", "");
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
                .expect("draft view");
        add_from_editor(
            &state,
            AppType::Codex,
            draft,
            view.settings.clone(),
            view.settings,
        )
        .expect("add with the profile active");
    }

    /// 新增对话框打开之后，客户端改了一个从 live 带进来的独有字段：草稿里没有它，保存时不
    /// 收进新供应商，切到新供应商再切走，客户端改的值还在。预设自己带的独有字段照样归新
    /// 供应商。
    #[tokio::test]
    #[serial]
    async fn codex_add_dialog_leaves_a_live_field_changed_after_opening_to_the_client() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(
            &CODEX_USER_LIVE.replace(
                "model = \"gpt-a\"\n",
                "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
            ),
            None,
        );
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        let config_of = |id: &str| {
            state
                .db
                .get_provider_by_id(id, "codex")
                .unwrap()
                .unwrap()
                .settings_config["config"]
                .as_str()
                .unwrap()
                .to_string()
        };

        let draft = codex_row(
            "c",
            "https://c.example/v1",
            "model_auto_compact_token_limit = 90000",
        );
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
                .expect("draft view");
        let changed =
            codex_text().replace("model_verbosity = \"high\"", "model_verbosity = \"low\"");
        fs::write(codex_config_path(), changed).unwrap();
        add_from_editor(
            &state,
            AppType::Codex,
            draft,
            view.settings.clone(),
            view.settings,
        )
        .expect("add c");
        let stored = config_of("c");
        assert!(
            stored.contains("model_auto_compact_token_limit = 90000")
                && !stored.contains("model_verbosity"),
            "{stored}"
        );
        ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        assert_eq!(codex_doc()["model_verbosity"].as_str(), Some("low"));

        // 旧的调用方不带草稿：退回和 live 里用户自己的值比（live 没被改过时分得清）。
        let mut legacy = codex_row(
            "d",
            "https://d.example/v1",
            "model_auto_compact_token_limit = 80000",
        );
        let base =
            ProviderService::editor_view(&state, AppType::Codex, &legacy.settings_config, None)
                .expect("legacy view")
                .settings;
        legacy.settings_config = base.clone();
        ProviderService::add_from_editor(
            &state,
            AppType::Codex,
            legacy,
            true,
            Some(crate::services::provider::EditorSave {
                base,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("add d");
        let stored = config_of("d");
        assert!(
            stored.contains("model_auto_compact_token_limit = 80000")
                && !stored.contains("model_verbosity"),
            "{stored}"
        );
    }

    /// 编辑器里把路由表从 custom 改名成别的表：那张表归供应商（按内容收成 custom 表），
    /// 不当成全局设置写进 live，切走后表和里面的 Key 都不会留下。
    #[tokio::test]
    #[serial]
    async fn codex_editor_route_table_renamed_in_the_editor_stays_with_the_provider() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

        let mut row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let view = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
            .expect("view a");
        let shown = view.settings["config"].as_str().unwrap();
        assert!(shown.contains("[model_providers.custom]\n"), "{shown}");
        let mut edited = view.settings.clone();
        edited["config"] = json!(shown
            .replace(
                "model_provider = \"custom\"",
                "model_provider = \"deepseek\""
            )
            .replace(
                "[model_providers.custom]\n",
                "[model_providers.deepseek]\nexperimental_bearer_token = \"sk-secret\"\n",
            ));
        row.settings_config = edited;
        ProviderService::update_from_editor(
            &state,
            AppType::Codex,
            None,
            row,
            Some(crate::services::provider::EditorSave {
                base: view.settings,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("save");
        let live = codex_text();
        assert!(!live.contains("[model_providers.deepseek]"), "{live}");
        assert!(live.contains("[model_providers.ollama_local]"), "{live}");

        ProviderService::switch(&state, AppType::Codex, "b").expect("switch to b");
        let live = codex_text();
        assert!(
            !live.contains("sk-secret") && !live.contains("deepseek"),
            "{live}"
        );
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

    // ===== Gemini CLI =====

    const GEMINI_USER_ENV: &str = "# my notes\nGEMINI_SANDBOX=docker\nGEMINI_API_KEY=key-a\nDEBUG=1\nGOOGLE_GEMINI_BASE_URL=https://a.example\nGEMINI_MODEL=m-a\n";
    const GEMINI_USER_SETTINGS: &str = r#"{
  "model": {
    "name": "m-a",
    "compressionThreshold": 0.5
  },
  "security": {
    "auth": {
      "selectedType": "gemini-api-key"
    }
  },
  "mcpServers": {
    "fs": {
      "command": "fs"
    }
  }
}
"#;

    fn gemini_env_path() -> std::path::PathBuf {
        crate::gemini_config::get_gemini_env_path()
    }

    fn gemini_settings_path() -> std::path::PathBuf {
        crate::gemini_config::get_gemini_settings_path()
    }

    fn seed_gemini(env: &str, settings: &str) {
        fs::create_dir_all(gemini_env_path().parent().unwrap()).unwrap();
        fs::write(gemini_env_path(), env).unwrap();
        fs::write(gemini_settings_path(), settings).unwrap();
    }

    fn gemini_env() -> String {
        fs::read_to_string(gemini_env_path()).unwrap()
    }

    fn gemini_settings() -> Value {
        serde_json::from_slice(&fs::read(gemini_settings_path()).unwrap()).unwrap()
    }

    /// `.env` 里用户自己的行（注释、非关键字段），按原顺序。
    fn gemini_user_lines(text: &str) -> Vec<String> {
        text.lines()
            .filter(|line| {
                line.split_once('=')
                    .is_none_or(|(key, _)| !crate::live::floor::gemini_floor_env(key.trim()))
            })
            .map(str::to_string)
            .collect()
    }

    fn gemini(id: &str, env: Value, config: Value) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({ "env": env, "config": config }),
            None,
        )
    }

    fn gemini_a_vertex() -> [Provider; 2] {
        [
            gemini(
                "a",
                json!({
                    "GEMINI_API_KEY": "key-a",
                    "GOOGLE_GEMINI_BASE_URL": "https://a.example",
                    "GEMINI_MODEL": "m-a"
                }),
                json!({ "model": { "name": "m-a" } }),
            ),
            gemini(
                "vertex",
                json!({
                    "GOOGLE_GENAI_USE_VERTEXAI": "true",
                    "GOOGLE_CLOUD_PROJECT": "p"
                }),
                json!({}),
            ),
        ]
    }

    #[tokio::test]
    #[serial]
    async fn gemini_switch_replaces_only_key_fields_and_round_trips() {
        let _home = Home::new();
        seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
        let state = state_with(AppType::Gemini, &gemini_a_vertex(), "a").await;

        ProviderService::switch(&state, AppType::Gemini, "vertex").expect("to vertex");
        assert_eq!(
            gemini_env(),
            "# my notes\nGEMINI_SANDBOX=docker\nDEBUG=1\nGOOGLE_GENAI_USE_VERTEXAI=true\nGOOGLE_CLOUD_PROJECT=p\n"
        );
        assert_eq!(
            gemini_settings(),
            json!({
                "model": { "compressionThreshold": 0.5 },
                "security": { "auth": { "selectedType": "gemini-api-key" } },
                "mcpServers": { "fs": { "command": "fs" } }
            })
        );

        ProviderService::switch(&state, AppType::Gemini, "a").expect("back to a");
        let env = gemini_env();
        assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));
        for line in [
            "GEMINI_API_KEY=key-a",
            "GOOGLE_GEMINI_BASE_URL=https://a.example",
            "GEMINI_MODEL=m-a",
        ] {
            assert!(env.contains(line), "{env}");
        }
        assert!(!env.contains("VERTEX"), "{env}");
        assert_eq!(
            gemini_settings(),
            serde_json::from_str::<Value>(GEMINI_USER_SETTINGS).unwrap()
        );
        assert_eq!(direct(&state, &AppType::Gemini).as_deref(), Some("a"));
    }

    #[tokio::test]
    #[serial]
    async fn gemini_official_switch_selects_the_google_login() {
        let _home = Home::new();
        seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
        let [a, _] = gemini_a_vertex();
        let mut official = gemini("google", json!({}), json!({}));
        official.category = Some("official".to_string());
        let state = state_with(AppType::Gemini, &[a, official], "a").await;

        ProviderService::switch(&state, AppType::Gemini, "google").expect("to official");
        assert_eq!(gemini_env(), "# my notes\nGEMINI_SANDBOX=docker\nDEBUG=1\n");
        assert_eq!(
            gemini_settings()["security"]["auth"]["selectedType"],
            json!("oauth-personal")
        );
        assert!(gemini_settings()["model"].get("name").is_none());
    }

    #[tokio::test]
    #[serial]
    async fn gemini_proxy_contract_follows_the_route_model_and_skips_same_contract_routes() {
        let _home = Home::new();
        seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
        let [a, _] = gemini_a_vertex();
        let mut b = a.clone();
        b.id = "b".to_string();
        b.settings_config["env"]["GEMINI_API_KEY"] = json!("key-b");
        b.settings_config["env"]["GOOGLE_GEMINI_BASE_URL"] = json!("https://b.example");
        let state = state_with(AppType::Gemini, &[a, b], "a").await;

        enter(&state, &AppType::Gemini).await.expect("enter");
        let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        let env = gemini_env();
        assert!(env.contains("GEMINI_API_KEY=PROXY_MANAGED"), "{env}");
        assert!(
            env.contains(&format!("GOOGLE_GEMINI_BASE_URL={proxy_url}")),
            "{env}"
        );
        assert!(env.contains("GEMINI_MODEL=m-a"), "{env}");
        assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));

        // b 的模型名和 a 一样：契约相同，客户端文件不读也不写。
        let before = fs::read(gemini_env_path()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(gemini_env_path(), fs::Permissions::from_mode(0o000)).unwrap();
        }
        ProviderService::switch(&state, AppType::Gemini, "b").expect("route to b");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(gemini_env_path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(fs::read(gemini_env_path()).unwrap(), before);
        assert_eq!(in_use(&state, &AppType::Gemini).as_deref(), Some("b"));

        exit(&state, &AppType::Gemini).await.expect("exit");
        let env = gemini_env();
        assert!(env.contains("GEMINI_API_KEY=key-a"), "{env}");
        assert!(!env.contains(PROXY_TOKEN_PLACEHOLDER), "{env}");
        assert_eq!(gemini_user_lines(&env), gemini_user_lines(GEMINI_USER_ENV));
    }

    #[tokio::test]
    #[serial]
    async fn gemini_editor_saves_key_fields_to_the_row_and_global_edits_to_live() {
        let _home = Home::new();
        seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
        let state = state_with(AppType::Gemini, &gemini_a_vertex(), "a").await;

        let a = state.db.get_provider_by_id("a", "gemini").unwrap().unwrap();
        let view = ProviderService::editor_view(&state, AppType::Gemini, &a.settings_config, None)
            .expect("view");
        assert_eq!(view.settings["env"]["GEMINI_SANDBOX"], json!("docker"));
        assert_eq!(
            view.settings["config"]["mcpServers"]["fs"]["command"],
            json!("fs")
        );

        let mut edited = view.settings.clone();
        edited["env"]["GEMINI_API_KEY"] = json!("key-a2");
        edited["env"]["DEBUG"] = json!("2");
        edited["config"]["ui"] = json!({ "theme": "dark" });
        let mut row = a.clone();
        row.settings_config = edited;
        ProviderService::update_from_editor(
            &state,
            AppType::Gemini,
            Some("a"),
            row,
            Some(crate::services::provider::EditorSave {
                base: view.settings.clone(),
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("save");

        let env = gemini_env();
        assert!(
            env.contains("GEMINI_API_KEY=key-a2") && env.contains("DEBUG=2"),
            "{env}"
        );
        assert_eq!(gemini_settings()["ui"], json!({ "theme": "dark" }));
        let saved = state.db.get_provider_by_id("a", "gemini").unwrap().unwrap();
        assert_eq!(
            saved.settings_config["env"]["GEMINI_API_KEY"],
            json!("key-a2")
        );
        assert!(saved.settings_config["env"].get("DEBUG").is_none());
        assert!(saved.settings_config["config"].get("ui").is_none());

        // 编辑非当前的 vertex：live 的关键字段不动，全局改动照写。
        let vertex = state
            .db
            .get_provider_by_id("vertex", "gemini")
            .unwrap()
            .unwrap();
        let view =
            ProviderService::editor_view(&state, AppType::Gemini, &vertex.settings_config, None)
                .expect("view vertex");
        assert!(view.settings["env"].get("GEMINI_API_KEY").is_none());
        let mut edited = view.settings.clone();
        edited["env"]["GOOGLE_CLOUD_PROJECT"] = json!("p2");
        edited["env"]["DEBUG"] = json!("3");
        let mut row = vertex.clone();
        row.settings_config = edited;
        ProviderService::update_from_editor(
            &state,
            AppType::Gemini,
            Some("vertex"),
            row,
            Some(crate::services::provider::EditorSave {
                base: view.settings,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("save vertex");
        let env = gemini_env();
        assert!(
            env.contains("GEMINI_API_KEY=key-a2") && env.contains("DEBUG=3"),
            "{env}"
        );
        assert!(!env.contains("GOOGLE_CLOUD_PROJECT"), "{env}");
        let saved = state
            .db
            .get_provider_by_id("vertex", "gemini")
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.settings_config["env"]["GOOGLE_CLOUD_PROJECT"],
            json!("p2")
        );
    }

    // ===== Grok Build =====

    const GROK_USER_LIVE: &str = "# mine\n[ui]\ntheme = \"dark\"\n\n[model.mine]\nmodel = \"m\"\nname = \"Mine\"\n\n[mcp_servers.fs]\ncommand = \"fs\"\n";

    fn grok_path() -> std::path::PathBuf {
        crate::grok_config::get_grok_config_path()
    }

    fn seed_grok(text: &str) {
        fs::create_dir_all(grok_path().parent().unwrap()).unwrap();
        fs::write(grok_path(), text).unwrap();
    }

    fn grok_text() -> String {
        fs::read_to_string(grok_path()).unwrap()
    }

    fn grok_doc() -> toml::Table {
        toml::from_str(&grok_text()).unwrap()
    }

    /// live 里的 `[model.*]` 表名（排好序）。
    fn grok_tables() -> Vec<String> {
        grok_doc()
            .get("model")
            .and_then(|model| model.as_table())
            .map(|tables| tables.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn grok_row(id: &str, table: &str, extra: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_uppercase(),
            json!({ "config": format!(
                "[models]\ndefault = \"{table}\"\n\n[model.\"{table}\"]\nmodel = \"{id}-model\"\nname = \"{id}\"\nbase_url = \"https://{id}.example/v1\"\napi_key = \"key-{id}\"\napi_backend = \"responses\"\ncontext_window = 500000\n{extra}"
            ) }),
            None,
        )
    }

    fn grok_official() -> Provider {
        let mut official = Provider::with_id(
            "grok-official".to_string(),
            "Grok Official".to_string(),
            json!({ "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        official
    }

    #[tokio::test]
    #[serial]
    async fn grok_switch_deletes_the_written_table_even_after_the_client_changed_the_default() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_with(
            AppType::GrokBuild,
            &[grok_row("a", "grok-4.5", ""), grok_official()],
            "grok-official",
        )
        .await;

        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("to a");
        assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.5"));
        assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);

        // Grok 的 /settings 把默认模型改成了内置的 grok-4.6。
        let changed = grok_text().replace("default = \"grok-4.5\"", "default = \"grok-4.6\"");
        fs::write(grok_path(), changed).unwrap();

        ProviderService::switch(&state, AppType::GrokBuild, "grok-official").expect("to official");
        assert_eq!(grok_tables(), vec!["mine"], "{}", grok_text());
        assert!(grok_doc().get("models").is_none(), "{}", grok_text());
        assert!(grok_text().starts_with("# mine\n[ui]\ntheme = \"dark\"\n"));

        // 切回去照样能用。
        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("back to a");
        assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
    }

    #[tokio::test]
    #[serial]
    async fn grok_table_keys_follow_the_provider_and_renames_replace_the_old_table() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_with(
            AppType::GrokBuild,
            &[
                grok_row("a", "grok-4.5", "reasoning_summary = \"none\"\n"),
                grok_row("b", "b", ""),
            ],
            "b",
        )
        .await;

        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("to a");
        assert_eq!(
            grok_doc()["model"]["grok-4.5"]["reasoning_summary"].as_str(),
            Some("none")
        );
        ProviderService::switch(&state, AppType::GrokBuild, "b").expect("to b");
        assert_eq!(grok_tables(), vec!["b", "mine"]);
        assert!(
            !grok_text().contains("reasoning_summary"),
            "{}",
            grok_text()
        );

        // 编辑当前供应商、把表名从 b 改成 grok-4.6：live 里只剩新表。
        let mut b = state
            .db
            .get_provider_by_id("b", "grokbuild")
            .unwrap()
            .unwrap();
        b.settings_config = grok_row("b", "grok-4.6", "").settings_config;
        ProviderService::update(&state, AppType::GrokBuild, Some("b"), b).expect("rename");
        assert_eq!(grok_tables(), vec!["grok-4.6", "mine"]);
        assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.6"));
    }

    #[tokio::test]
    #[serial]
    async fn grok_without_a_write_record_infers_the_table_an_older_version_wrote() {
        let _home = Home::new();
        // 旧版把 a 的行整份写进了 live：新版没有写入记录。
        let a = grok_row("a", "grok-4.5", "");
        let old = format!(
            "{}\n{}",
            a.settings_config["config"].as_str().unwrap(),
            GROK_USER_LIVE
        );
        seed_grok(&old);
        let state = state_with(AppType::GrokBuild, &[a, grok_row("b", "b", "")], "a").await;

        ProviderService::switch(&state, AppType::GrokBuild, "b").expect("to b");
        assert_eq!(grok_tables(), vec!["b", "mine"], "{}", grok_text());
        assert_eq!(
            state::written(&DeviceStore::for_device(), "grokbuild")
                .unwrap()
                .unwrap()
                .tables,
            vec!["b".to_string()]
        );
    }

    #[tokio::test]
    #[serial]
    async fn grok_proxy_writes_the_route_table_through_the_engine() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_with(
            AppType::GrokBuild,
            &[
                grok_row("a", "grok-4.5", ""),
                grok_row("b", "b", ""),
                grok_official(),
            ],
            "a",
        )
        .await;
        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

        enter(&state, &AppType::GrokBuild).await.expect("enter");
        let proxy_url = state.proxy_service.build_proxy_urls().await.unwrap().0;
        let doc = grok_doc();
        let table = &doc["model"]["grok-4.5"];
        assert_eq!(table["api_key"].as_str(), Some(PROXY_TOKEN_PLACEHOLDER));
        assert_eq!(
            table["base_url"].as_str(),
            Some(format!("{proxy_url}/grokbuild/v1").as_str())
        );

        // 换一家表名不同的路由：旧的代理表按写入记录删掉。
        ProviderService::switch(&state, AppType::GrokBuild, "b").expect("route to b");
        assert_eq!(grok_tables(), vec!["b", "mine"]);
        assert!(
            ProviderService::switch(&state, AppType::GrokBuild, "grok-official").is_err(),
            "the official account cannot be routed"
        );

        exit(&state, &AppType::GrokBuild).await.expect("exit");
        assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
        assert_eq!(
            grok_doc()["model"]["grok-4.5"]["api_key"].as_str(),
            Some("key-a")
        );
        assert!(grok_text().contains("[mcp_servers.fs]"));
    }

    #[tokio::test]
    #[serial]
    async fn grok_switch_crash_rolls_forward_with_the_write_record() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_with(
            AppType::GrokBuild,
            &[grok_row("a", "grok-4.5", ""), grok_row("b", "b", "")],
            "a",
        )
        .await;
        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

        for point in ["published:0", "target"] {
            ProviderService::switch(&state, AppType::GrokBuild, "a").expect("reset");
            failpoint::crash_at(Some(point));
            let crashed = ProviderService::switch(&state, AppType::GrokBuild, "b");
            failpoint::crash_at(None);
            assert!(crashed.is_err(), "{point}");

            crate::mode::operation::recover_on_startup(&state.db);
            assert_eq!(grok_tables(), vec!["b", "mine"], "{point}");
            assert_eq!(direct(&state, &AppType::GrokBuild).as_deref(), Some("b"));
            assert_eq!(
                state::written(&DeviceStore::for_device(), "grokbuild")
                    .unwrap()
                    .unwrap()
                    .tables,
                vec!["b".to_string()],
                "{point}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn grok_editor_saves_the_table_to_the_row_and_global_edits_to_live() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_with(
            AppType::GrokBuild,
            &[grok_row("a", "grok-4.5", ""), grok_row("b", "b", "")],
            "a",
        )
        .await;
        ProviderService::switch(&state, AppType::GrokBuild, "a").expect("direct a");

        let a = state
            .db
            .get_provider_by_id("a", "grokbuild")
            .unwrap()
            .unwrap();
        let view =
            ProviderService::editor_view(&state, AppType::GrokBuild, &a.settings_config, None)
                .expect("view");
        let shown = view.settings["config"].as_str().unwrap().to_string();
        assert!(
            shown.contains("[model.mine]") && shown.contains("key-a"),
            "{shown}"
        );

        let edited = shown
            .replace("theme = \"dark\"", "theme = \"light\"")
            .replace("a-model", "a-model-2");
        let mut row = a.clone();
        row.settings_config = json!({ "config": edited });
        ProviderService::update_from_editor(
            &state,
            AppType::GrokBuild,
            Some("a"),
            row,
            Some(crate::services::provider::EditorSave {
                base: view.settings,
                draft: None,
                on_conflict: Default::default(),
            }),
        )
        .expect("save");

        let doc = grok_doc();
        assert_eq!(doc["ui"]["theme"].as_str(), Some("light"));
        assert_eq!(
            doc["model"]["grok-4.5"]["model"].as_str(),
            Some("a-model-2")
        );
        let saved = state
            .db
            .get_provider_by_id("a", "grokbuild")
            .unwrap()
            .unwrap();
        let row_text = saved.settings_config["config"].as_str().unwrap();
        assert!(row_text.contains("a-model-2"), "{row_text}");
        assert!(
            !row_text.contains("[ui]") && !row_text.contains("[model.mine]"),
            "{row_text}"
        );
    }

    // ---------- 新增对话框：和编辑器同一套规则 ----------

    async fn state_without_providers() -> AppState {
        let db = Arc::new(Database::memory().expect("memory db"));
        db.update_proxy_config(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .expect("ephemeral port");
        AppState::new(db)
    }

    fn add_from_editor(
        state: &AppState,
        app: AppType,
        mut row: Provider,
        edited: Value,
        base: Value,
    ) -> Result<bool, AppError> {
        // 和新增对话框一样：`row` 是投影成 `base` 的草稿。
        let draft = std::mem::replace(&mut row.settings_config, edited);
        ProviderService::add_from_editor(
            state,
            app,
            row,
            true,
            Some(crate::services::provider::EditorSave {
                base,
                draft: Some(draft),
                on_conflict: Default::default(),
            }),
        )
    }

    #[tokio::test]
    #[serial]
    async fn codex_add_dialog_saves_key_fields_to_the_row_and_global_edits_to_live() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, None);
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

        // 新增 c：显示的是切到 c 之后的 config.toml，全局部分来自 live。
        let draft = codex_row("c", "https://c.example/v1", "");
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
                .expect("view c");
        let shown = view.settings["config"].as_str().unwrap().to_string();
        assert!(
            shown.contains("gpt-c") && shown.contains("approval_policy"),
            "{shown}"
        );
        let mut edited = view.settings.clone();
        edited["config"] = json!(shown.replace("\"on-request\"", "\"never\""));
        add_from_editor(&state, AppType::Codex, draft, edited, view.settings).expect("add c");

        let live = codex_text();
        assert!(live.contains("approval_policy = \"never\""), "{live}");
        assert_eq!(codex_doc()["model"].as_str(), Some("gpt-a"), "{live}");
        let c = state.db.get_provider_by_id("c", "codex").unwrap().unwrap();
        let c_config = c.settings_config["config"].as_str().unwrap();
        assert!(
            c_config.contains("gpt-c") && !c_config.contains("approval_policy"),
            "{c_config}"
        );
        assert_eq!(
            c.meta.as_ref().and_then(|meta| meta.common_config_enabled),
            Some(true)
        );
    }

    /// 新增对话框的底已经套了预设：预设带的独有字段归新供应商，live 里用户自己写的不归它。
    #[tokio::test]
    #[serial]
    async fn codex_add_dialog_keeps_the_users_exclusive_fields_out_of_the_new_row() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(
            &CODEX_USER_LIVE.replace(
                "model = \"gpt-a\"\n",
                "model = \"gpt-a\"\nmodel_verbosity = \"high\"\n",
            ),
            None,
        );
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;

        let draft = codex_row(
            "c",
            "https://c.example/v1",
            "model_auto_compact_token_limit = 90000\n",
        );
        let view =
            ProviderService::editor_view(&state, AppType::Codex, &draft.settings_config, None)
                .expect("view c");
        let shown = view.settings["config"].as_str().unwrap();
        assert!(
            shown.contains("model_verbosity") && shown.contains("model_auto_compact_token_limit"),
            "{shown}"
        );
        add_from_editor(
            &state,
            AppType::Codex,
            draft,
            view.settings.clone(),
            view.settings,
        )
        .expect("add c");

        let c = state.db.get_provider_by_id("c", "codex").unwrap().unwrap();
        let c_config = c.settings_config["config"].as_str().unwrap();
        assert!(
            c_config.contains("model_auto_compact_token_limit = 90000")
                && !c_config.contains("model_verbosity"),
            "{c_config}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn gemini_add_dialog_first_provider_writes_key_fields_and_sets_the_pointer() {
        let _home = Home::new();
        seed_gemini(GEMINI_USER_ENV, GEMINI_USER_SETTINGS);
        let state = state_without_providers().await;

        let draft = Provider::with_id(
            "c".to_string(),
            "C".to_string(),
            json!({ "env": {
                "GEMINI_API_KEY": "key-c",
                "GOOGLE_GEMINI_BASE_URL": "https://c.example",
                "GEMINI_MODEL": "m-c",
            }, "config": {} }),
            None,
        );
        let view =
            ProviderService::editor_view(&state, AppType::Gemini, &draft.settings_config, None)
                .expect("view c");
        assert_eq!(view.settings["env"]["GEMINI_SANDBOX"], json!("docker"));
        let mut edited = view.settings.clone();
        edited["env"]["DEBUG"] = json!("5");
        edited["config"]["ui"] = json!({ "theme": "dark" });
        add_from_editor(&state, AppType::Gemini, draft, edited, view.settings).expect("add c");

        let env = gemini_env();
        assert!(
            env.contains("GEMINI_API_KEY=key-c")
                && env.contains("GEMINI_MODEL=m-c")
                && env.contains("DEBUG=5")
                && env.contains("# my notes"),
            "{env}"
        );
        assert_eq!(gemini_settings()["ui"], json!({ "theme": "dark" }));
        assert_eq!(
            crate::mode::current::provider_for(
                &state.db,
                &AppType::Gemini,
                crate::mode::current::Purpose::Direct
            )
            .unwrap()
            .as_deref(),
            Some("c")
        );
        let c = state.db.get_provider_by_id("c", "gemini").unwrap().unwrap();
        assert!(c.settings_config["env"].get("DEBUG").is_none());
        assert!(c.settings_config["config"].get("ui").is_none());
    }

    #[tokio::test]
    #[serial]
    async fn grok_add_dialog_first_provider_records_the_written_table() {
        let _home = Home::new();
        seed_grok(GROK_USER_LIVE);
        let state = state_without_providers().await;

        let draft = grok_row("a", "grok-4.5", "");
        let view =
            ProviderService::editor_view(&state, AppType::GrokBuild, &draft.settings_config, None)
                .expect("view a");
        let shown = view.settings["config"].as_str().unwrap().to_string();
        assert!(
            shown.contains("[model.mine]") && shown.contains("key-a"),
            "{shown}"
        );
        let edited = json!({ "config": shown.replace("theme = \"dark\"", "theme = \"light\"") });
        add_from_editor(&state, AppType::GrokBuild, draft, edited, view.settings).expect("add a");

        let doc = grok_doc();
        assert_eq!(doc["ui"]["theme"].as_str(), Some("light"));
        assert_eq!(doc["models"]["default"].as_str(), Some("grok-4.5"));
        assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
        let written = crate::mode::state::written(&DeviceStore::for_device(), "grokbuild")
            .unwrap()
            .expect("write record");
        assert_eq!(written.tables, vec!["grok-4.5".to_string()]);
        let a = state
            .db
            .get_provider_by_id("a", "grokbuild")
            .unwrap()
            .unwrap();
        let row_text = a.settings_config["config"].as_str().unwrap();
        assert!(
            !row_text.contains("[ui]") && !row_text.contains("[model.mine]"),
            "{row_text}"
        );

        // 第二个新增的供应商不动 live 的关键字段。
        let draft = grok_row("b", "b", "");
        let view =
            ProviderService::editor_view(&state, AppType::GrokBuild, &draft.settings_config, None)
                .expect("view b");
        let edited = view.settings.clone();
        add_from_editor(&state, AppType::GrokBuild, draft, edited, view.settings).expect("add b");
        assert_eq!(grok_doc()["models"]["default"].as_str(), Some("grok-4.5"));
        assert_eq!(grok_tables(), vec!["grok-4.5", "mine"]);
    }
}
