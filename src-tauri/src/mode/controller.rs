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
    direct_patch, proxy_projection, ClaudeProjection, ProxyAuth, StackRoleModel,
    PROXY_TOKEN_PLACEHOLDER,
};
use crate::live::project::gemini::GeminiProjection;
use crate::live::project::grok::GrokProjection;
use crate::provider::Provider;
use crate::services::provider::codex_client_catalog;
use crate::services::provider::codex_direct::{self, Owner};
use crate::services::provider::codex_official_models;
use crate::services::provider::{claude_direct, gemini_direct, grok_direct};
use crate::store::AppState;

use super::contract;
use super::current::{self, Purpose};
use super::operation;
use super::stack::{self, StackModel, StackView};
use super::state::{op, Contract, Mode, ModeState, PendingTarget, StackState};

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

/// 代理模式且已接上时的路由供应商和读到的模式状态；否则 `None`。
fn attached_route(
    state: &AppState,
    app: &AppType,
) -> Result<Option<(ModeState, Provider)>, String> {
    let mode = current::mode_state(app);
    if !mode.is_proxy() || !mode.attached {
        return Ok(None);
    }
    Ok(route_provider(state, app, &mode)?.map(|route| (mode, route)))
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

/// `stack` 是发布的 Stack 模型，`stack_default` 是四档别名指向的模型（Stack 模式下默认那家
/// 列表里的第一个，见 [`stack::claude_route_default`]；路由模式为 `None`）。
fn claude_contract(
    route: &Provider,
    proxy_url: &str,
    stack: &[StackModel],
    stack_default: Option<&StackModel>,
) -> (ClaudeProjection, Contract) {
    let mut projection = proxy_projection(
        &ClaudeProjection::of(&route.settings_config),
        proxy_url,
        claude_proxy_auth(route),
        stack_default.map(|model| StackRoleModel {
            id: &model.id,
            name: &model.name,
        }),
    );
    with_stack_models(&mut projection, stack);
    let contract = contract::claude(&projection);
    (projection, contract)
}

const CLAUDE_MAX_CONTEXT_ENV: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";

/// `/model` 选择器的配置（顶层关键字段）。`replaceBuiltInOptions` 让选择器只剩 Default 和
/// 这里列的行：内置的 Opus / Sonnet / Haiku 行都指向默认那家的模型，留着就是几行重复，
/// 还会把那个模型自己的行去重吃掉。四档别名仍然要写，Default、`--model opus` 和子代理靠它们。
const CLAUDE_MODEL_PICKER: &str = "modelPicker";

/// 发布了 Stack 模型时：`/model` 换成 Stack 模型列表；`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 改由
/// Stack 模型决定，取非 1M 模型里最小的窗口（等于默认 200K 时不写）。四档别名也写成 Stack id，
/// 同样受 MAX 约束；1M 的 Stack 模型不受 MAX 影响。没有发布 Stack 模型时契约和原来逐字节一致。
fn with_stack_models(projection: &mut ClaudeProjection, stack: &[StackModel]) {
    if stack.is_empty() {
        return;
    }
    let options: Vec<Value> = stack
        .iter()
        .map(|model| {
            json!({
                "model": model.id,
                "label": model.display_name,
                "description": model.description,
            })
        })
        .collect();
    projection.top.insert(
        CLAUDE_MODEL_PICKER.to_string(),
        json!({ "replaceBuiltInOptions": true, "options": options }),
    );
    projection.exclusive.shift_remove(CLAUDE_MAX_CONTEXT_ENV);
    let smallest = stack
        .iter()
        .filter(|model| !model.one_m)
        .map(|model| model.window)
        .min();
    if let Some(window) = smallest.filter(|window| *window != stack::CLAUDE_DEFAULT_WINDOW) {
        projection.exclusive.insert(
            CLAUDE_MAX_CONTEXT_ENV.to_string(),
            Value::String(window.to_string()),
        );
    }
}

/// 这个应用已落定的 Stack 名单。
fn settled_stack(app: &AppType) -> Result<StackState, String> {
    super::state::stack(&DeviceStore::for_device(), app.as_str()).map_err(err)
}

/// 发布 Stack 模型的成员（见 [`stack::published_members`]）。
fn published_members(
    state: &AppState,
    app: &AppType,
    stack: &StackState,
    route: &Provider,
) -> Result<Vec<stack::Member>, String> {
    stack::published_members(&state.db, app, stack, Some(route.id.as_str())).map_err(err)
}

/// 只落定状态，不碰客户端文件（未接上时换路由、故障转移记下新路由等）。
fn commit_state(state: &AppState, app: &AppType, target: &PendingTarget) -> Result<(), String> {
    operation::commit_target(&state.db, &DeviceStore::for_device(), app.as_str(), target)
        .map_err(err)
}

/// 写代理契约。`target` 的 `contract` 由这里填好。契约没变时不碰客户端文件；接上
/// （启动时）一律重写：顺带核对路由供应商还能用（比如托管账号还在），并修正 CC Switch
/// 没运行期间客户端文件里的漂移。
///
/// `next_stack` 是这次操作之后的 Stack 名单：参与契约计算，随同一个操作落定
/// （`PendingTarget::stack`）；`None` 按已落定的名单算。
async fn write_proxy(
    state: &AppState,
    app: &AppType,
    op_name: &str,
    route: &Provider,
    live_now: &LiveNow,
    mut target: ModeState,
    next_stack: Option<StackState>,
) -> Result<(), String> {
    let (proxy_url, codex_base_url) = state.proxy_service.build_proxy_urls().await?;
    // 改用 CC Switch 的目录时契约可能没变（指针只在 live 里），也要重写。
    let force = op_name == op::ATTACH || op_name == op::CATALOG;
    let stack = match &next_stack {
        Some(next) => next.clone(),
        None => settled_stack(app)?,
    };
    // 各应用把 `contract` 填进 `target` 之后，模式状态和新名单一起落定。
    let pending = |target: ModeState| PendingTarget {
        state: Some(target),
        stack: next_stack.clone(),
        ..PendingTarget::default()
    };
    match app {
        AppType::Claude => {
            let members = published_members(state, app, &stack, route)?;
            let published = stack::claude_published(&members);
            let stack_default = stack::claude_route_default(&members);
            let (projection, contract) =
                claude_contract(route, &proxy_url, &published, stack_default.as_ref());
            let unchanged = !force && live_now.has_contract(&contract.key);
            let patch = direct_patch(live_now.claude_exclusive_owner().as_ref(), &projection);
            target.contract = Some(contract);
            claude_direct::run(
                &state.db,
                op_name,
                (!unchanged).then_some(&patch),
                pending(target),
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
                pending(target),
            )
            .map_err(err)?;
        }
        AppType::Codex => {
            let owner = live_now.codex_owner();
            let members = published_members(state, app, &stack, route)?;
            let spec = codex_direct::Target::Proxy {
                route,
                base_url: &codex_base_url,
                stack: &members,
            };
            let mut prepared =
                codex_direct::prepare(&state.codex_oauth_manager, &owner, &spec).map_err(err)?;
            codex_direct::prepare_official_rows(&state.db, &owner, &spec, &mut prepared)
                .await
                .map_err(err)?;
            let planned = codex_direct::plan(&state.db, &owner, &spec, &prepared).map_err(err)?;
            let unchanged = !force && live_now.has_contract(&planned.contract.key);
            target.contract = Some(planned.contract.clone());
            let pending = pending(target);
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
                pending(target),
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

/// 进入代理模式。`stack_mode` 为真是 Stack 模式（界面上和路由模式二选一，见
/// [`StackState::enabled`]）：默认那家（代理路由）随之加入名单，名单里其余各家的模型发布给
/// 客户端。已经在代理模式时按选的模式重写。
pub async fn enter(state: &AppState, app: &AppType, stack_mode: bool) -> Result<(), String> {
    enter_with_route(state, app, stack_mode, None).await
}

/// 同 [`enter`]，`route` 是确认框里选的路由目标（Stack 模式下是默认那家）；`None` 沿用上次
/// 的路由，没有就用直连那家。已经在代理模式时（路由 ↔ Stack）在同一把切换锁里按新模式和
/// 新目标重写，不经过直连。
pub async fn enter_with_route(
    state: &AppState,
    app: &AppType,
    stack_mode: bool,
    route: Option<&str>,
) -> Result<(), String> {
    require_proxy_app(app)?;
    if stack_mode && !stack::supports_stack(app) {
        return Err(format!(
            "{} 不支持聚合模式 ({} does not support the Aggregation mode)",
            app.as_str(),
            app.as_str()
        ));
    }
    let explicit = match route {
        Some(id) => {
            let target = provider(state, app, id)?.ok_or_else(|| format!("供应商不存在: {id}"))?;
            reject_unsupported_official(app, &target)?;
            Some(target)
        }
        None => None,
    };
    let result = match lock_settled(state, app).await {
        Ok(_guard) => enter_locked(state, app, op::ENTER, Some(stack_mode), explicit).await,
        Err(error) => Err(error.to_string()),
    };
    if result.is_err() {
        stop_server_if_unused(state).await;
    }
    result
}

/// `stack_mode` 为 `None` 时沿用已落定的模式（启动时接上）。`explicit_route` 是指定的路由
/// 目标，`None` 沿用上次的路由。
async fn enter_locked(
    state: &AppState,
    app: &AppType,
    op_name: &str,
    stack_mode: Option<bool>,
    explicit_route: Option<Provider>,
) -> Result<(), String> {
    if !state.proxy_service.is_running().await {
        state.proxy_service.start().await?;
    }
    let mode = current::mode_state(app);
    let saved_route = match explicit_route {
        Some(route) => Some(route),
        None => route_provider(state, app, &mode)?,
    };
    let route = match saved_route {
        Some(route) => route,
        None => direct_provider(state, app)?.ok_or_else(|| {
            format!(
                "{} 没有当前供应商，无法进入代理模式 (No current provider for {})",
                app.as_str(),
                app.as_str()
            )
        })?,
    };
    let next_stack = match stack_mode {
        Some(on) => {
            let current = settled_stack(app)?;
            let mut next = current.clone();
            next.enabled = on;
            if on {
                add_default(app, &mut next, &route);
            }
            (next != current).then_some(next)
        }
        None => None,
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
        next_stack,
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

/// 设置里在路由和 Stack 之间换：处于另一种模式（`stack` 为真是 Stack 模式）的 Claude Code、
/// Codex 退回直连，名单留着。返回退回直连的应用。
pub async fn exit_apps_in_mode(state: &AppState, stack: bool) -> Result<Vec<String>, String> {
    let mut exited = Vec::new();
    for app in PROXY_APPS.into_iter().filter(stack::supports_stack) {
        {
            let _guard = lock_settled(state, &app).await.map_err(|e| e.to_string())?;
            if !current::is_proxy(&app) || settled_stack(&app)?.enabled != stack {
                continue;
            }
            exit_locked(state, &app, false)?;
        }
        if let Err(error) = state.db.clear_provider_health_for_app(app.as_str()).await {
            log::warn!("清除 {} 健康状态失败: {error}", app.as_str());
        }
        exited.push(app.as_str().to_string());
    }
    // 没有应用退出时服务不是这次用的：可能是用户手动开的，或者 Claude Desktop 的模型映射
    // 在用，别停。
    if !exited.is_empty() {
        stop_server_if_unused(state).await;
    }
    Ok(exited)
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

/// 没有应用在代理模式、Claude Desktop 也没在用模型映射时，停掉代理服务。
async fn stop_server_if_unused(state: &AppState) {
    if current::proxy_flags(PROXY_APPS).contains(&true)
        || crate::claude_desktop_config::current_provider_uses_proxy(&state.db)
    {
        return;
    }
    if state.proxy_service.is_running().await {
        if let Err(error) = state.proxy_service.stop().await {
            log::warn!("停止代理服务失败: {error}");
        }
    }
}

/// Claude Desktop 的当前供应商是模型映射卡、代理服务却没在跑时把它拉起来：启动、切到映射卡、
/// 应用项目之后各调一次。拉不起来只记日志，Desktop 页的状态横幅会提示服务没在运行。
pub async fn ensure_desktop_mapping_service(state: &AppState) {
    if !crate::claude_desktop_config::current_provider_uses_proxy(&state.db)
        || state.proxy_service.is_running().await
    {
        return;
    }
    if let Err(error) = state.proxy_service.start().await {
        log::error!("Claude Desktop 正在用模型映射，启动代理服务失败: {error}");
    }
}

/// Claude Desktop 换卡（切换、应用项目）之后对齐代理服务：从映射卡换走时，别人也不用就停掉；
/// 换上映射卡时拉起来。`was_mapping` 是换卡前的 `current_provider_uses_proxy`：只在映射卡
/// 被换走时才去停，别的换卡不碰服务，免得停掉用户在设置页手动开的服务。
pub async fn sync_desktop_mapping_service(state: &AppState, was_mapping: bool) {
    if was_mapping && !crate::claude_desktop_config::current_provider_uses_proxy(&state.db) {
        stop_server_if_unused(state).await;
    }
    ensure_desktop_mapping_service(state).await;
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
    // Stack 模式下默认那家也在名单里：设为默认的这家（比如托盘里点的）还没添加就一起加上。
    let current = settled_stack(app)?;
    let mut next = current.clone();
    if next.enabled {
        add_default(app, &mut next, target);
    }
    let next_stack = (next != current).then_some(next);
    if !mode.attached {
        commit_state(
            state,
            app,
            &PendingTarget {
                state: Some(new_state),
                stack: next_stack,
                ..PendingTarget::default()
            },
        )?;
    } else {
        let live_now = LiveNow::of(state, app, &mode)?;
        write_proxy(
            state,
            app,
            op::ROUTE,
            target,
            &live_now,
            new_state,
            next_stack,
        )
        .await?;
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
    switch_route_checked(state, app, provider_id, false).await
}

/// 开启故障转移时切到队列 P1（拿切换锁）。Stack 模式不做故障转移，拒绝：否则会换掉默认
/// 那家。要在锁内、补完上次没做完的操作之后再看模式，锁外读到的可能是旧的。
pub async fn switch_route_for_failover(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
) -> Result<(), String> {
    switch_route_checked(state, app, provider_id, true).await
}

async fn switch_route_checked(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
    reject_stacked: bool,
) -> Result<(), String> {
    require_proxy_app(app)?;
    let target =
        provider(state, app, provider_id)?.ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    reject_unsupported_official(app, &target)?;
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
    if reject_stacked && current::is_proxy(app) && settled_stack(app)?.enabled {
        return Err(
            "聚合模式不做故障转移，请先换回路由模式 (The Aggregation mode has no failover; switch back to the routing mode first)"
                .to_string(),
        );
    }
    switch_route_locked(state, app, &target).await
}

/// 指定代理路由（聚合模式下是默认那家），不管现在是什么模式。直连模式下只记下指针，下次进入
/// 路由 / 聚合模式时用它，客户端文件和模式都不动；已经在代理模式时就是换路由，当场生效。
pub async fn set_route(state: &AppState, app: &AppType, provider_id: &str) -> Result<(), String> {
    require_proxy_app(app)?;
    let target =
        provider(state, app, provider_id)?.ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
    reject_unsupported_official(app, &target)?;
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
    let mode = current::mode_state(app);
    if mode.is_proxy() {
        return switch_route_locked(state, app, &target).await;
    }
    if mode.proxy_route.as_deref() == Some(provider_id) {
        return Ok(());
    }
    commit_state(
        state,
        app,
        &PendingTarget {
            state: Some(ModeState {
                proxy_route: Some(target.id),
                ..mode
            }),
            ..PendingTarget::default()
        },
    )
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

/// 增删 Stack 模型失败。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackWriteError {
    /// 已部分写入：客户端文件可能一半新一半旧，下次操作这个应用或重启 CC Switch 时按
    /// pending 补完，最终是新名单。为假时什么都没改。
    pub partial: bool,
    pub message: String,
}

impl StackWriteError {
    pub(crate) fn unchanged(message: impl Into<String>) -> Self {
        Self {
            partial: false,
            message: message.into(),
        }
    }
}

/// 能不能加进 Stack。不能是官方账号：Claude 官方订阅本来就不进代理；Codex 官方可以做路由，
/// 但它靠客户端自己的登录做账号校验，不能当 Stack 目标。Codex 的行还要能解析：它的模型要
/// 写进目录。
fn check_stack_member(app: &AppType, provider: &Provider) -> Result<(), String> {
    let official = match app {
        AppType::Codex => codex_direct::is_official(provider),
        _ => provider.category.as_deref() == Some("official"),
    };
    if official {
        return Err(
            "官方账号不能加入聚合 (An official account cannot join the aggregation)".to_string(),
        );
    }
    if matches!(app, AppType::Codex) {
        codex_direct::check_stack_member(provider).map_err(err)?;
    }
    Ok(())
}

/// Stack 模式下默认那家也在名单里（界面上是「已添加」）。官方账号只能做默认、不能加进 Stack，
/// 配置解析不了的 Codex 行也不加（它做默认时写入本来就会失败）。
fn add_default(app: &AppType, stack: &mut StackState, route: &Provider) {
    if check_stack_member(app, route).is_err() {
        return;
    }
    stack::allocate_key(stack, route);
    if !stack.is_member(&route.id) {
        stack.members.push(route.id.clone());
    }
}

/// 增删一家 Stack 供应商。`enabled` 是目标值，不是「切换一下」：以为失败又点一次时，先补完
/// 上一次的操作再应用同一个目标值，结果是空操作，不会反过来撤销。
///
/// 代理模式且已接上时，新名单参与契约计算，和客户端文件在同一个操作里提交（契约没变就
/// 只落定名单）；否则只落定名单，下次进入代理模式时生效。移除时 key 留在登记簿里。
///
/// 成功时返回客户端看不到或看不全 Stack 模型的提示（见 [`StackView::notice`]）。失败分两种
/// （见 [`StackWriteError::partial`]）：操作还没开始发布就失败，什么都没改；已经开始发布，
/// pending 留着等前滚。
pub async fn set_stack_member(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
    enabled: bool,
) -> Result<Option<&'static str>, StackWriteError> {
    if !stack::supports_stack(app) {
        return Err(StackWriteError::unchanged(format!(
            "{} 不支持聚合的模型 ({} does not support aggregated models)",
            app.as_str(),
            app.as_str()
        )));
    }
    let _guard = lock_settled(state, app)
        .await
        .map_err(|error| StackWriteError::unchanged(error.to_string()))?;
    if let Err(message) = set_stack_member_locked(state, app, provider_id, enabled).await {
        let partial = operation::pending_published(&DeviceStore::for_device(), app.as_str())
            .unwrap_or_else(|error| {
                log::warn!("读取 {} 的写前意图失败: {error}", app.as_str());
                None
            })
            == Some(true);
        return Err(StackWriteError { partial, message });
    }
    // 名单已经落定：读不出提示也不能报成保存失败（界面会以为什么都没改）。
    Ok(match app {
        AppType::Codex => settled_stack(app)
            .ok()
            .and_then(|stack| codex_stack_notice(state, &stack)),
        _ => None,
    })
}

async fn set_stack_member_locked(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let current = settled_stack(app)?;
    let mut next = current.clone();
    if enabled {
        let target = provider(state, app, provider_id)?
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;
        check_stack_member(app, &target)?;
        stack::allocate_key(&mut next, &target);
        if !next.is_member(provider_id) {
            next.members.push(provider_id.to_string());
        }
    } else {
        let mode = current::mode_state(app);
        if current.enabled && mode.routes_to(provider_id) {
            return Err(
                "默认供应商不能移除，请先把别的供应商设为默认 (The default provider cannot be removed; set another provider as the default first)"
                    .to_string(),
            );
        }
        next.members.retain(|id| id != provider_id);
    }
    if next == current {
        return Ok(());
    }

    match attached_route(state, app)? {
        Some((mode, route)) => {
            let live_now = LiveNow::of(state, app, &mode)?;
            write_proxy(state, app, op::STACK, &route, &live_now, mode, Some(next)).await
        }
        None => commit_state(
            state,
            app,
            &PendingTarget {
                stack: Some(next),
                ..PendingTarget::default()
            },
        ),
    }
}

/// Stack 模式的状态、名单里的每一家和它发布的模型 id（给前端）。
pub fn stack_views(state: &AppState, app: &AppType) -> Result<StackView, String> {
    if !stack::supports_stack(app) {
        return Ok(StackView::default());
    }
    let stack = settled_stack(app)?;
    let mode = current::mode_state(app);
    let route = mode.proxy_route.as_deref().filter(|_| mode.is_proxy());
    let members = stack::members(&state.db, app, &stack, route).map_err(err)?;
    let notice = match app {
        AppType::Codex => codex_stack_notice(state, &stack),
        _ => None,
    };
    Ok(StackView {
        active: mode.is_proxy() && stack.enabled,
        members: stack::member_views(&members),
        notice,
        stale_clients: None,
    })
}

/// [`stack_views`] 再加上 Codex 客户端是不是还在用旧的模型列表（要读进程表，放到阻塞线程池
/// 里）。路由那家自己管理目录时，Stack 模型本来就不发布，重启也看不到，已经有 `notice` 说明，
/// 不再查。
pub async fn stack_view_with_clients(state: &AppState, app: &AppType) -> Result<StackView, String> {
    let mut view = stack_views(state, app)?;
    if matches!(app, AppType::Codex)
        && view.active
        && view.notice != Some("routeOwnsCatalog")
        && codex_publishes_stack_models(state, &settled_stack(app)?)
    {
        view.stale_clients = codex_direct::off_runtime(|| {
            codex_client_catalog::stale_clients(&DeviceStore::for_device())
        })
        .await
        .map_err(err)?;
    }
    Ok(view)
}

/// Codex 接着代理，名单里有要发布的 Stack 模型。
fn codex_publishes_stack_models(state: &AppState, stack: &StackState) -> bool {
    attached_route(state, &AppType::Codex)
        .ok()
        .flatten()
        .is_some_and(|(_, route)| {
            stack::published_members(&state.db, &AppType::Codex, stack, Some(&route.id))
                .is_ok_and(|published| !published.is_empty())
        })
}

/// Codex 在 Stack 模式下有要发布的 Stack 模型，客户端却看不到或看不全：路由那家自己管理模型
/// 目录文件（Stack 模型不发布）；或者官方做默认、最近一次写目录时没拿到官方列表。
fn codex_stack_notice(state: &AppState, stack: &StackState) -> Option<&'static str> {
    let (_, route) = attached_route(state, &AppType::Codex).ok()??;
    let published =
        stack::published_members(&state.db, &AppType::Codex, stack, Some(&route.id)).ok()?;
    if published.is_empty() {
        return None;
    }
    if codex_direct::route_owns_catalog(&route) {
        return Some("routeOwnsCatalog");
    }
    if !codex_direct::is_official(&route) {
        return None;
    }
    match codex_official_models::last_source()? {
        codex_official_models::NativeSource::Fetched => None,
        codex_official_models::NativeSource::Bundled => Some("officialModelsBundled"),
        codex_official_models::NativeSource::Unavailable => Some("officialModelsUnavailable"),
    }
}

/// Codex Stack 模式下聚合的模型被路由那家自己的模型目录挡住（`routeOwnsCatalog`，用户在提示
/// 上点了才调）：去掉它行里指向别的文件的 `model_catalog_json`，按当前路由重写，改用 CC Switch
/// 生成的目录。返回之后还剩的提示。
///
/// 先改行再写客户端：写失败时下一次重写按新的行投影，指针照样清掉。
pub async fn adopt_codex_stack_catalog(state: &AppState) -> Result<Option<&'static str>, String> {
    let app = AppType::Codex;
    let _guard = lock_settled(state, &app).await.map_err(err)?;
    let Some((mode, mut route)) = attached_route(state, &app)? else {
        return Ok(None);
    };
    if let Some(settings) = codex_direct::settings_without_row_catalog(&route) {
        state
            .db
            .update_provider_settings_config(app.as_str(), &route.id, &settings)
            .map_err(err)?;
        route.settings_config = settings;
    }
    let live_now = LiveNow::of(state, &app, &mode)?;
    write_proxy(state, &app, op::CATALOG, &route, &live_now, mode, None).await?;
    Ok(settled_stack(&app)
        .ok()
        .and_then(|stack| codex_stack_notice(state, &stack)))
}

/// 路由供应商的行或代理地址变了：按新契约重写客户端（契约没变就什么都不做）。调用方
/// 持有切换锁。
pub async fn resync_route_locked(state: &AppState, app: &AppType) -> Result<(), String> {
    let Some((_, route)) = attached_route(state, app)? else {
        return Ok(());
    };
    switch_route_locked(state, app, &route).await
}

/// 代理模式下存好了 `provider` 的行：它是代理路由时按新行重写代理契约；在 Stack 名单里时，
/// 它发布的模型、窗口在契约里，按当前路由重算。契约没变就不碰客户端文件，其余情况什么
/// 都不做。调用方持有切换锁。
pub async fn resync_saved_row_locked(
    state: &AppState,
    app: &AppType,
    provider: &Provider,
) -> Result<(), String> {
    let mode = current::mode_state(app);
    if !mode.is_proxy() {
        return Ok(());
    }
    if mode.routes_to(&provider.id) {
        return switch_route_locked(state, app, provider).await;
    }
    let in_stack = stack::is_member(app, &provider.id).unwrap_or_else(|error| {
        log::warn!("读取 {} 的 Stack 模型失败: {error}", app.as_str());
        false
    });
    if in_stack {
        return resync_route_locked(state, app).await;
    }
    Ok(())
}

pub async fn resync_route(state: &AppState, app: &AppType) -> Result<(), String> {
    let _guard = lock_settled(state, app).await.map_err(|e| e.to_string())?;
    resync_route_locked(state, app).await
}

/// 后台检查（CC Switch 启动时和之后每 15 分钟）：Codex 官方做路由、发布了 Stack 模型时，
/// 目标登录的官方模型缓存没有或过期了就刷新（一定联网）；客户端落后于缓存（列表刚变，
/// 或者之前重写失败了）就重写客户端文件。
pub async fn check_codex_official_models(state: &AppState) {
    refresh_codex_official_models(state).await;
    resync_codex_if_behind(state).await;
}

/// 预测登录可能读钥匙串、刷新要跑子进程和联网：整个放到阻塞线程池里。
async fn refresh_codex_official_models(state: &AppState) {
    let state = state.clone();
    let _ = codex_direct::off_runtime(move || {
        let login = match codex_official_login_now(&state, &AppType::Codex) {
            Ok(Some(login)) => login,
            Ok(None) => return,
            Err(error) => {
                log::debug!("检查 Codex 官方模型列表时预测登录失败: {error}");
                return;
            }
        };
        let Some(version) = codex_official_models::needs_refresh(&login) else {
            return;
        };
        if let Err(error) = codex_official_models::refresh(&login, &version) {
            log::warn!("刷新 Codex 官方模型列表失败: {error}");
        }
    })
    .await;
}

/// 缓存里的官方列表比客户端文件新：按当前路由重写（契约没变就什么都不做）。失败时标记
/// 放回去，下一个检查点再试。
pub async fn resync_codex_if_behind(state: &AppState) {
    if !codex_official_models::take_client_behind() {
        return;
    }
    if let Err(error) = resync_route(state, &AppType::Codex).await {
        codex_official_models::mark_client_behind();
        log::warn!("Codex 官方模型列表更新后重写客户端文件失败，下次检查再试: {error}");
    }
}

/// 现在（代理模式、接着、官方做路由、发布了 Stack 模型时）Codex 会用的登录。
fn codex_official_login_now(
    state: &AppState,
    app: &AppType,
) -> Result<Option<codex_official_models::OfficialLogin>, String> {
    let Some((mode, route)) = attached_route(state, app)? else {
        return Ok(None);
    };
    let members = published_members(state, app, &settled_stack(app)?, &route)?;
    if !codex_direct::needs_official_rows(&route, &members) {
        return Ok(None);
    }
    let live_now = LiveNow::of(state, app, &mode)?;
    let owner = live_now.codex_owner();
    let base_url = codex_direct::configured_proxy_base_url(&state.db);
    let spec = codex_direct::Target::Proxy {
        route: &route,
        base_url: &base_url,
        stack: &members,
    };
    let prepared = codex_direct::prepare(&state.codex_oauth_manager, &owner, &spec).map_err(err)?;
    codex_direct::predicted_official_login(&state.db, &owner, &spec, &prepared).map_err(err)
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
///
/// Stack 模式不做故障转移：这时到达的是进入 Stack 模式之前发出的请求的结果，已经过期，丢掉。
/// 记下它会换掉默认那家，却不把它加进名单、也不重写发布给客户端的模型。
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
    if settled_stack(app)?.enabled {
        log::info!(
            "[Failover] {} 在 Stack 模式，忽略进入之前的请求转移到 {provider_id} 的结果",
            app.as_str()
        );
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
    ensure_desktop_mapping_service(state).await;
    // 记一次新启动的 Codex 会读到的目录：兜住启动时补完的操作和 CC Switch 没开时的外部修改。
    codex_client_catalog::observe(&DeviceStore::for_device());
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
        match enter_locked(state, app, op::ATTACH, None, None).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                log::error!("启动时接上 {} 的代理失败，退回直连: {error}", app.as_str());
                let stack = stack::supports_stack(app)
                    && settled_stack(app)
                        .map(|stack| stack.enabled)
                        .unwrap_or(false);
                startup_attach_failures()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(StartupAttachFailure {
                        app_type: app.as_str().to_string(),
                        stack,
                        error: error.clone(),
                    });
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

/// 应用页的模式行：现在生效的是哪种模式、路由到谁（直连模式下是上次路由的那家）、直连那家。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppModeView {
    /// `direct` / `route` / `stack`
    pub mode: &'static str,
    /// 客户端文件指着代理（CC Switch 运行时为真；退出时分离）
    pub attached: bool,
    pub route_provider_id: Option<String>,
    pub direct_provider_id: Option<String>,
}

pub fn app_mode_view(state: &AppState, app: &AppType) -> Result<AppModeView, String> {
    require_proxy_app(app)?;
    let mode = current::mode_state(app);
    let name = if !mode.is_proxy() {
        "direct"
    } else if stack::supports_stack(app) && settled_stack(app)?.enabled {
        "stack"
    } else {
        "route"
    };
    Ok(AppModeView {
        mode: name,
        attached: mode.attached,
        route_provider_id: mode.proxy_route,
        direct_provider_id: direct_provider_id(state, app).map_err(err)?,
    })
}

/// 启动时没能接上代理、退回直连的应用。界面打开时取走一次，在应用页提示并给「重试」。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupAttachFailure {
    pub app_type: String,
    pub stack: bool,
    pub error: String,
}

fn startup_attach_failures() -> &'static std::sync::Mutex<Vec<StartupAttachFailure>> {
    static FAILURES: std::sync::OnceLock<std::sync::Mutex<Vec<StartupAttachFailure>>> =
        std::sync::OnceLock::new();
    FAILURES.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// 看一眼启动时记下的接上失败（不清空）：托盘的问题区自己留一份，界面照样取走。
pub fn startup_attach_failures_snapshot() -> Vec<StartupAttachFailure> {
    startup_attach_failures()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// 取走启动时记下的接上失败（取一次就清空）。
pub fn take_startup_attach_failures() -> Vec<StartupAttachFailure> {
    std::mem::take(
        &mut *startup_attach_failures()
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    )
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
        let (projection, _) = claude_contract(provider, "http://127.0.0.1:15721", &[], None);
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
    use crate::mode::stack::StackMemberView;
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

        enter(&state, &AppType::Claude, false).await.expect("enter");
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
        enter(&state, &AppType::Claude, false)
            .await
            .expect("enter again");
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
        enter(&state, &AppType::Claude, false).await.expect("enter");
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
        enter(&state, &AppType::Claude, false).await.expect("enter");
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
        enter(&state, &AppType::Claude, false).await.expect("enter");
        ProviderService::switch(&state, AppType::Claude, "b").expect("route to b");
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));

        enter(&state, &AppType::Claude, false)
            .await
            .expect("enter again");
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

        enter_locked(state, &AppType::Claude, op::ENTER, Some(false), None)
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
        enter_locked(state, &AppType::Claude, op::ENTER, Some(false), None)
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
        enter(&state, &AppType::Claude, false).await.expect("enter");
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
            let result = enter(&state, &AppType::Claude, false).await;
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
        enter(&state, &AppType::Claude, false).await.expect("enter");
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

        enter(&state, &AppType::Gemini, false).await.expect("enter");
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
        enter(&state, &AppType::Claude, false)
            .await
            .expect("enter claude");
        enter(&state, &AppType::Gemini, false)
            .await
            .expect("enter gemini");
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

        enter(&state, &AppType::Codex, false).await.expect("enter");
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
        // 和官方直连同一个会话桶（内置 openai）：不写选路，顶层改道到代理，客户端带自己的登录。
        assert!(doc.get("model_provider").is_none(), "{official_contract}");
        let base_url = doc["openai_base_url"].as_str().unwrap();
        assert!(
            base_url.starts_with("http://127.0.0.1:") && base_url.ends_with("/v1"),
            "{official_contract}"
        );
        assert!(
            doc["model_providers"].get("cc-switch-official").is_none(),
            "{official_contract}"
        );
        assert!(state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
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
        let third_party = fs::read_to_string(&config_path).unwrap();
        assert!(third_party.contains(PROXY_TOKEN_PLACEHOLDER));
        assert!(!third_party.contains("openai_base_url"), "{third_party}");
        exit(&state, &AppType::Codex).await.expect("exit");
        assert_eq!(auth(), native_auth);
        assert!(!state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
    }

    /// 官方做路由时会话和官方直连落在同一个桶：进出代理不换 Codex 选中的 provider id。
    #[tokio::test]
    #[serial]
    async fn codex_official_route_keeps_the_direct_session_bucket() {
        for unified in [false, true] {
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
            crate::codex_config::write_codex_live_atomic(
                &native_auth,
                Some("model = \"gpt-5.4\"\n"),
            )
            .unwrap();
            let mut official = Provider::with_id(
                crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
                "OpenAI Official".to_string(),
                json!({ "auth": {}, "config": "model = \"gpt-5.4\"\n" }),
                None,
            );
            official.category = Some("official".to_string());
            crate::settings::update_settings(crate::settings::AppSettings {
                preserve_codex_official_auth_on_switch: true,
                unify_codex_session_history: unified,
                ..Default::default()
            })
            .unwrap();
            let state = state_with(AppType::Codex, &[official.clone()], &official.id).await;
            ProviderService::switch(&state, AppType::Codex, &official.id).expect("direct official");
            let config_path = crate::codex_config::get_codex_config_path();
            let selector = || -> Option<String> {
                let text = fs::read_to_string(&config_path).unwrap();
                let doc: toml::Table = toml::from_str(&text).unwrap();
                doc.get("model_provider")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            let bucket = if unified {
                Some("custom".to_string())
            } else {
                None
            };
            assert_eq!(selector(), bucket, "direct, unified={unified}");

            enter(&state, &AppType::Codex, false).await.expect("enter");
            let contract = fs::read_to_string(&config_path).unwrap();
            assert_eq!(selector(), bucket, "proxy, unified={unified}: {contract}");
            let doc: toml::Table = toml::from_str(&contract).unwrap();
            if unified {
                assert!(doc.get("openai_base_url").is_none(), "{contract}");
                let mirror = &doc["model_providers"]["custom"];
                assert_eq!(mirror["name"].as_str(), Some("OpenAI"));
                assert_eq!(mirror["requires_openai_auth"].as_bool(), Some(true));
                assert_eq!(mirror["supports_websockets"].as_bool(), Some(false));
                assert!(mirror.get("experimental_bearer_token").is_none());
                assert!(
                    mirror["base_url"].as_str().unwrap().ends_with("/v1"),
                    "{contract}"
                );
            } else {
                assert!(doc["openai_base_url"].as_str().is_some(), "{contract}");
                assert!(doc.get("model_providers").is_none(), "{contract}");
            }
            assert!(state
                .proxy_service
                .live_has_proxy_placeholder(&AppType::Codex));
            assert_eq!(
                crate::config::read_json_file::<Value>(&crate::codex_config::get_codex_auth_path())
                    .unwrap(),
                native_auth
            );

            exit(&state, &AppType::Codex).await.expect("exit");
            let restored = fs::read_to_string(&config_path).unwrap();
            assert_eq!(selector(), bucket, "exit, unified={unified}: {restored}");
            assert!(!restored.contains("openai_base_url"), "{restored}");
            assert!(!state
                .proxy_service
                .live_has_proxy_placeholder(&AppType::Codex));
        }
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
        enter(&state, &AppType::Codex, false).await.expect("enter");
        ProviderService::switch(&state, AppType::Codex, "a").expect("route to a");
        assert_eq!(pointer().as_deref(), Some("/work/a-catalog.json"));
        exit(&state, &AppType::Codex).await.expect("exit");
        assert_eq!(pointer(), None, "{}", codex_text());

        // 指针是关键字段：手写进 live 的值不保留，有目录时换成 CC Switch 的，没有就删。
        let with_user = format!("model_catalog_json = \"/work/mine.json\"\n{}", codex_text());
        fs::write(codex_config_path(), with_user).unwrap();
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        assert_eq!(pointer().as_deref(), Some(ours), "{}", codex_text());
        let with_user = codex_text().replace(
            &format!("model_catalog_json = \"{ours}\""),
            "model_catalog_json = \"/work/mine.json\"",
        );
        fs::write(codex_config_path(), with_user).unwrap();
        ProviderService::switch(&state, AppType::Codex, "c").expect("to c");
        assert_eq!(pointer(), None, "{}", codex_text());
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

        enter(&state, &AppType::Codex, false).await.expect("enter");
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
        enter(&state, &AppType::Codex, false).await.expect("enter");
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

        enter(&state, &AppType::Gemini, false).await.expect("enter");
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

        enter(&state, &AppType::GrokBuild, false)
            .await
            .expect("enter");
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

    // ---- Stack 模型（`mode::stack`） ----

    fn stack_rows() -> [Provider; 3] {
        [
            claude("a", "https://a.example", json!({})),
            claude(
                "kimi",
                "https://kimi.example",
                json!({
                    "ANTHROPIC_MODEL": "kimi-k3",
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "128000"
                }),
            ),
            claude(
                "zhipu",
                "https://zhipu.example",
                json!({ "ANTHROPIC_MODEL": "glm-5.2[1M]" }),
            ),
        ]
    }

    fn stack_state() -> StackState {
        state::stack(&DeviceStore::for_device(), "claude").unwrap()
    }

    /// 用户自己可能打开的模型发现开关（聚合模式不再写它）。
    const DISCOVERY_ENV: &str = "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY";

    /// `/model` 里列的 Stack 模型 id；没有 `modelPicker` 时为 `None`。
    fn picker() -> Option<Vec<String>> {
        let picker = settings().get(CLAUDE_MODEL_PICKER)?.clone();
        assert_eq!(picker["replaceBuiltInOptions"], true, "{picker}");
        Some(
            picker["options"]
                .as_array()
                .expect("options")
                .iter()
                .map(|row| row["model"].as_str().expect("model").to_string())
                .collect(),
        )
    }

    async fn set_member(state: &AppState, id: &str, enabled: bool) -> Vec<StackMemberView> {
        set_stack_member(state, &AppType::Claude, id, enabled)
            .await
            .unwrap_or_else(|error| panic!("set {id}={enabled}: {error:?}"));
        stack_views(state, &AppType::Claude).unwrap().members
    }

    #[tokio::test]
    #[serial]
    async fn stack_models_join_the_claude_contract_and_leave_with_it() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");
        let only_default_bytes = fs::read(settings_path()).unwrap();
        let only_default_contract = mode(&AppType::Claude).contract.unwrap();

        // 进入 Stack 模式时默认那家（a）已经在名单里，照常发布；四档都指向它的第一个模型。
        assert_eq!(stack_state().members, vec!["a"]);
        assert_eq!(
            picker(),
            Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()])
        );
        let options = &settings()[CLAUDE_MODEL_PICKER]["options"][0];
        assert_eq!(options["label"], "claude-sonnet-4-6（A）");
        assert_eq!(options["description"], "claude-sonnet-4-6 · 200K");
        let env = settings()["env"].clone();
        assert!(env.get(DISCOVERY_ENV).is_none(), "{env}");
        for role in ["HAIKU", "SONNET", "OPUS", "FABLE"] {
            assert_eq!(
                env[format!("ANTHROPIC_DEFAULT_{role}_MODEL")],
                "ccs-claude-a--claude-sonnet-4-6",
                "{role}"
            );
            assert_eq!(
                env[format!("ANTHROPIC_DEFAULT_{role}_MODEL_NAME")],
                "claude-sonnet-4-6",
                "{role}"
            );
        }
        let views = set_member(&state, "kimi", true).await;
        assert_eq!(views.len(), 2);
        assert!(views[0].route);
        assert_eq!(views[0].model_ids, vec!["ccs-claude-a--claude-sonnet-4-6"]);
        assert_eq!(stack_state().key_of("kimi"), Some("kimi"));
        assert_eq!(views[1].model_ids, vec!["ccs-claude-kimi--kimi-k3"]);
        assert_eq!(
            picker(),
            Some(vec![
                "ccs-claude-a--claude-sonnet-4-6".to_string(),
                "ccs-claude-kimi--kimi-k3".to_string(),
            ])
        );
        let env = settings()["env"].clone();
        assert_eq!(env[CLAUDE_MAX_CONTEXT_ENV], "128000");
        assert_eq!(
            env["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "ccs-claude-a--claude-sonnet-4-6"
        );
        assert_eq!(
            mode(&AppType::Claude).contract.unwrap().exclusive[CLAUDE_MAX_CONTEXT_ENV],
            "128000"
        );

        // 1M 的 Stack 模型不影响 MAX。
        set_member(&state, "zhipu", true).await;
        assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "128000");
        set_member(&state, "kimi", false).await;
        assert_eq!(
            picker(),
            Some(vec![
                "ccs-claude-a--claude-sonnet-4-6".to_string(),
                "ccs-claude-zhipu--glm-5.2[1M]".to_string(),
            ])
        );
        let env = settings()["env"].clone();
        assert!(env.get(CLAUDE_MAX_CONTEXT_ENV).is_none(), "{env}");

        // 只剩默认那家：客户端文件和契约回到刚进入时的样子，登记簿保留。
        set_member(&state, "zhipu", false).await;
        assert_eq!(fs::read(settings_path()).unwrap(), only_default_bytes);
        assert_eq!(
            mode(&AppType::Claude).contract.unwrap(),
            only_default_contract
        );
        let stack = stack_state();
        assert_eq!(stack.members, vec!["a"]);
        assert_eq!(stack.key_of("kimi"), Some("kimi"));
        assert_eq!(stack.key_of("zhipu"), Some("zhipu"));

        exit(&state, &AppType::Claude).await.expect("exit");
        assert_back_to_user_settings();
    }

    #[tokio::test]
    #[serial]
    async fn a_users_own_model_discovery_switch_survives_switches_and_proxy_mode() {
        let _home = Home::new();
        let mut user: Value = serde_json::from_str(USER_SETTINGS).unwrap();
        user["env"][DISCOVERY_ENV] = json!("1");
        seed_settings(&serde_json::to_string_pretty(&user).unwrap());
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        let discovery = || settings()["env"].get(DISCOVERY_ENV).cloned();

        ProviderService::switch(&state, AppType::Claude, "kimi").expect("direct kimi");
        assert_eq!(discovery(), Some(json!("1")));
        enter(&state, &AppType::Claude, false).await.expect("enter");
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(discovery(), Some(json!("1")), "no stacked models");
    }

    /// `modelPicker` 是关键字段：聚合模式下换成 Stack 模型列表，用户自己配的不保留；离开
    /// 聚合模式（退出代理、直连切换）都清掉。
    #[tokio::test]
    #[serial]
    async fn the_stack_model_picker_replaces_a_users_own_and_leaves_with_stack_mode() {
        let _home = Home::new();
        let mut user: Value = serde_json::from_str(USER_SETTINGS).unwrap();
        user[CLAUDE_MODEL_PICKER] = json!({ "options": [{ "model": "mine" }] });
        seed_settings(&serde_json::to_string_pretty(&user).unwrap());
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;

        enter(&state, &AppType::Claude, true).await.expect("enter");
        assert_eq!(
            picker(),
            Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()])
        );

        exit(&state, &AppType::Claude).await.expect("exit");
        assert!(settings().get(CLAUDE_MODEL_PICKER).is_none());
        assert_back_to_user_settings();
    }

    #[tokio::test]
    #[serial]
    async fn stack_models_leave_the_client_file_on_exit_and_come_back_on_enter() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;

        // 不在代理模式：只存名单，客户端文件不动。
        set_member(&state, "kimi", true).await;
        assert_eq!(fs::read_to_string(settings_path()).unwrap(), USER_SETTINGS);
        assert!(stack_state().is_member("kimi"));

        enter(&state, &AppType::Claude, true).await.expect("enter");
        assert!(picker().is_some());
        assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "128000");

        exit(&state, &AppType::Claude).await.expect("exit");
        assert_back_to_user_settings();
        assert!(
            stack_state().is_member("kimi"),
            "the list outlives proxy mode"
        );
    }

    #[tokio::test]
    #[serial]
    async fn the_default_leads_the_aliases_and_repeating_a_target_changes_nothing() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");
        let before = fs::read(settings_path()).unwrap();

        // 路由那家已经在名单里：再加一次什么都不变。
        let views = set_member(&state, "a", true).await;
        assert_eq!(views.len(), 1);
        assert_eq!(fs::read(settings_path()).unwrap(), before);

        set_member(&state, "kimi", true).await;
        let with_kimi = fs::read(settings_path()).unwrap();
        set_member(&state, "kimi", true).await;
        assert_eq!(fs::read(settings_path()).unwrap(), with_kimi);
        assert_eq!(stack_state().members, vec!["a", "kimi"]);

        // 换默认到名单里的 kimi：四档跟着指向 kimi 的第一个模型，两家都照常发布。
        ProviderService::switch(&state, AppType::Claude, "kimi").expect("switch route");
        assert!(picker().is_some());
        let env = settings()["env"].clone();
        assert_eq!(env[CLAUDE_MAX_CONTEXT_ENV], "128000");
        assert_eq!(
            env["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "ccs-claude-kimi--kimi-k3"
        );
        assert_eq!(env["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"], "kimi-k3");
        let views = stack_views(&state, &AppType::Claude).unwrap().members;
        let published: Vec<(&str, bool, usize)> = views
            .iter()
            .map(|view| (view.provider_id.as_str(), view.route, view.model_ids.len()))
            .collect();
        assert_eq!(published, vec![("a", false, 1), ("kimi", true, 1)]);
    }

    #[tokio::test]
    #[serial]
    async fn routing_mode_and_stack_mode_share_the_list() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        set_member(&state, "kimi", true).await;

        // 路由模式：名单在也不发布。
        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing");
        assert_eq!(picker(), None);
        assert!(!stack_views(&state, &AppType::Claude).unwrap().active);

        // 已经在代理模式时换成 Stack 模式：默认那家加入名单，其余的发布。
        enter(&state, &AppType::Claude, true)
            .await
            .expect("stack mode");
        assert!(picker().is_some());
        assert!(stack_views(&state, &AppType::Claude).unwrap().active);
        assert_eq!(stack_state().members, vec!["kimi", "a"]);

        // 换回路由模式：名单留着，客户端里的 Stack 模型去掉。
        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing again");
        assert_eq!(picker(), None);
        assert_eq!(stack_state().members, vec!["kimi", "a"]);
        assert!(!stack_state().enabled);

        // 退出再以 Stack 模式进入：名单原样恢复。
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_back_to_user_settings();
        enter(&state, &AppType::Claude, true)
            .await
            .expect("stack mode again");
        assert!(picker().is_some());
        assert_eq!(stack_state().members, vec!["kimi", "a"]);
    }

    #[tokio::test]
    #[serial]
    async fn entering_with_a_picked_route_changes_target_and_mode_in_one_step() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        let view = || app_mode_view(&state, &AppType::Claude).expect("mode view");

        assert_eq!(view().mode, "direct");
        assert_eq!(view().direct_provider_id.as_deref(), Some("a"));

        // 直连 → 路由，路由到确认框里选的那家；直连指针不动。
        enter_with_route(&state, &AppType::Claude, false, Some("kimi"))
            .await
            .expect("route to kimi");
        assert_eq!(view().mode, "route");
        assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
        assert_eq!(view().direct_provider_id.as_deref(), Some("a"));

        // 路由 → Stack，默认换成另一家：同一把锁里一次写完，不经过直连。
        enter_with_route(&state, &AppType::Claude, true, Some("zhipu"))
            .await
            .expect("stack with zhipu");
        assert_eq!(view().mode, "stack");
        assert_eq!(view().route_provider_id.as_deref(), Some("zhipu"));
        assert!(stack_state().members.contains(&"zhipu".to_string()));

        // 回到直连：路由目标留着，下次进入沿用。
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(view().mode, "direct");
        assert_eq!(view().route_provider_id.as_deref(), Some("zhipu"));
        assert_back_to_user_settings();

        // 选了不存在的供应商：什么都不改。
        enter_with_route(&state, &AppType::Claude, false, Some("missing"))
            .await
            .expect_err("unknown provider");
        assert_eq!(view().mode, "direct");
    }

    #[tokio::test]
    #[serial]
    async fn setting_the_route_in_direct_mode_only_records_it() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let mut official = claude("official", "https://api.anthropic.com", json!({}));
        official.category = Some("official".to_string());
        let [a, kimi, zhipu] = stack_rows();
        let state = state_with(AppType::Claude, &[a, kimi, zhipu, official], "a").await;
        let view = || app_mode_view(&state, &AppType::Claude).expect("mode view");

        // 直连模式：只记下指针。模式、直连指针、名单、客户端文件都不动。
        set_route(&state, &AppType::Claude, "kimi")
            .await
            .expect("remember kimi");
        assert_eq!(view().mode, "direct");
        assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
        assert_eq!(view().direct_provider_id.as_deref(), Some("a"));
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));
        assert!(stack_state().is_empty());
        assert_back_to_user_settings();

        // 不存在的、不能走代理的官方订阅：拒绝，记下的那家不变。
        set_route(&state, &AppType::Claude, "missing")
            .await
            .expect_err("unknown provider");
        set_route(&state, &AppType::Claude, "official")
            .await
            .expect_err("official subscription");
        assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));

        // 进入聚合模式沿用记下的那家：它是默认，随之加入名单。
        enter(&state, &AppType::Claude, true).await.expect("enter");
        assert_eq!(view().mode, "stack");
        assert_eq!(view().route_provider_id.as_deref(), Some("kimi"));
        assert_eq!(stack_state().members, vec!["kimi"]);

        // 已经在聚合模式：当场换默认，新默认也加入名单。
        set_route(&state, &AppType::Claude, "zhipu")
            .await
            .expect("set default");
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("zhipu"));
        assert_eq!(stack_state().members, vec!["kimi", "zhipu"]);
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_back_to_user_settings();
    }

    #[tokio::test]
    #[serial]
    async fn the_default_cannot_be_removed_and_a_new_default_joins_the_list() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");

        let error = set_stack_member(&state, &AppType::Claude, "a", false)
            .await
            .expect_err("the default");
        assert!(!error.partial);
        assert_eq!(stack_state().members, vec!["a"]);

        // 设为默认（比如托盘里点）一家还没添加的：一起加入名单，原来的默认留在名单里，
        // 之后可以移除。四档指向新默认的第一个模型：1M 模型三档带标记，haiku 不带。
        ProviderService::switch(&state, AppType::Claude, "zhipu").expect("set default");
        assert_eq!(stack_state().members, vec!["a", "zhipu"]);
        assert!(picker().is_some());
        let env = settings()["env"].clone();
        assert_eq!(
            env["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "ccs-claude-zhipu--glm-5.2[1M]"
        );
        assert_eq!(
            env["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "ccs-claude-zhipu--glm-5.2"
        );
        let with_a = settings();
        set_member(&state, "a", false).await;
        assert_eq!(stack_state().members, vec!["zhipu"]);
        assert_eq!(
            picker(),
            Some(vec!["ccs-claude-zhipu--glm-5.2[1M]".to_string()])
        );
        let env = settings()["env"].clone();
        assert_eq!(env, with_a["env"], "a is not the default any more");
    }

    #[tokio::test]
    #[serial]
    async fn changing_the_main_page_switch_sends_apps_in_the_other_mode_back_to_direct() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        set_member(&state, "kimi", true).await;
        enter(&state, &AppType::Claude, true).await.expect("enter");

        // 换成显示路由开关之前，Stack 模式的应用退回直连；路由模式的不动。
        assert!(exit_apps_in_mode(&state, false).await.unwrap().is_empty());
        assert!(current::is_proxy(&AppType::Claude));
        assert_eq!(
            exit_apps_in_mode(&state, true).await.unwrap(),
            vec!["claude"]
        );
        assert!(!current::is_proxy(&AppType::Claude));
        assert_back_to_user_settings();
        assert_eq!(stack_state().members, vec!["kimi", "a"], "the list stays");
    }

    #[tokio::test]
    #[serial]
    async fn claude_desktop_model_mapping_keeps_the_server_running() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(
            AppType::Claude,
            &[claude("a", "https://a.example", json!({}))],
            "a",
        )
        .await;
        let mut mapping = Provider::with_id(
            "map".to_string(),
            "Map".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://map.example" } }),
            None,
        );
        mapping.meta = Some(crate::provider::ProviderMeta {
            claude_desktop_mode: Some(crate::provider::ClaudeDesktopMode::Proxy),
            ..Default::default()
        });
        let desktop = AppType::ClaudeDesktop;
        state
            .db
            .save_provider(desktop.as_str(), &mapping)
            .expect("save mapping provider");
        crate::settings::set_current_provider(&desktop, Some("map")).expect("desktop current");

        // Claude Code 退出路由时，Desktop 还在用模型映射，服务不能跟着停。
        enter(&state, &AppType::Claude, false).await.expect("enter");
        exit(&state, &AppType::Claude).await.expect("exit");
        assert!(state.proxy_service.is_running().await);

        // 服务被手动停掉后，下一次检查（启动、切换、应用项目）会把它拉起来。
        state.proxy_service.stop().await.expect("stop");
        ensure_desktop_mapping_service(&state).await;
        assert!(state.proxy_service.is_running().await);

        // 从映射卡换走时 Claude Code 还在路由：服务留着，等它退出路由时再停。
        enter(&state, &AppType::Claude, false).await.expect("enter");
        crate::settings::set_current_provider(&desktop, None).expect("clear desktop current");
        state
            .db
            .delete_provider(desktop.as_str(), "map")
            .expect("drop mapping");
        sync_desktop_mapping_service(&state, true).await;
        assert!(state.proxy_service.is_running().await);
        exit(&state, &AppType::Claude).await.expect("exit");
        assert!(!state.proxy_service.is_running().await);

        // 换卡前就不是映射卡：用户在设置页手动开的服务不碰。
        state.proxy_service.start().await.expect("manual start");
        sync_desktop_mapping_service(&state, false).await;
        assert!(state.proxy_service.is_running().await);

        // 从映射卡换走、没人在用：顺手停掉。
        sync_desktop_mapping_service(&state, true).await;
        assert!(!state.proxy_service.is_running().await);
    }

    #[tokio::test]
    #[serial]
    async fn changing_the_main_page_switch_with_nothing_to_exit_keeps_the_server() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        // 各应用都直连，服务是用户手动开的（或者 Claude Desktop 的模型映射在用）。
        state.proxy_service.start().await.expect("start");

        assert!(exit_apps_in_mode(&state, true).await.unwrap().is_empty());
        assert!(state.proxy_service.is_running().await);
        state.proxy_service.stop().await.expect("stop");
    }

    #[tokio::test]
    #[serial]
    async fn a_failover_finishing_after_entering_stack_mode_is_dropped() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");
        let before = fs::read(settings_path()).unwrap();

        // 进入 Stack 模式之前发出的路由请求，这时才转移到 kimi 成功。
        assert!(!record_failover_route(&state, &AppType::Claude, "kimi")
            .await
            .expect("record failover"));
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));
        assert_eq!(stack_state().members, vec!["a"]);
        assert_eq!(fs::read(settings_path()).unwrap(), before);
    }

    #[tokio::test]
    #[serial]
    async fn enabling_failover_cannot_move_the_default_in_stack_mode() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing");
        set_member(&state, "kimi", true).await;

        // 进入 Stack 模式做到一半失败：锁外读到的还是路由模式，拿切换锁补完之后才是 Stack 模式。
        failpoint::crash_at(Some("published:0"));
        enter(&state, &AppType::Claude, true)
            .await
            .expect_err("crash");
        failpoint::crash_at(None);
        assert!(!crate::mode::stack::stack_mode_now(&AppType::Claude));

        switch_route_for_failover(&state, &AppType::Claude, "kimi")
            .await
            .expect_err("stack mode");
        assert!(crate::mode::stack::stack_mode_now(&AppType::Claude));
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("a"));

        // 路由模式照常切到 P1。
        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing");
        switch_route_for_failover(&state, &AppType::Claude, "kimi")
            .await
            .expect("switch");
        assert_eq!(in_use(&state, &AppType::Claude).as_deref(), Some("kimi"));
        state.proxy_service.stop().await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn stack_mode_sends_default_requests_to_the_default_only() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        // 故障转移开着，队列里还有别家。
        let mut config = state.db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        state.db.update_proxy_config_for_app(config).await.unwrap();
        for id in ["a", "kimi"] {
            state.db.add_to_failover_queue("claude", id).unwrap();
        }
        let proxy = crate::proxy::server::ProxyState::for_test(state.db.clone());
        let body = json!({ "model": "claude-sonnet-5", "messages": [] });
        let headers = axum::http::HeaderMap::new();
        let context = || {
            crate::proxy::handler_context::RequestContext::new(
                &proxy,
                &body,
                &headers,
                AppType::Claude,
                "Claude",
                "claude",
                None,
            )
        };
        let chain = |ctx: &crate::proxy::handler_context::RequestContext| {
            ctx.get_providers()
                .into_iter()
                .map(|provider| provider.id)
                .collect::<Vec<_>>()
        };

        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing");
        let ctx = context().await.expect("routing context");
        assert_eq!(chain(&ctx), vec!["a", "kimi"]);
        assert!(ctx.app_config.auto_failover_enabled);

        enter(&state, &AppType::Claude, true)
            .await
            .expect("stack mode");
        let ctx = context().await.expect("stack mode context");
        assert_eq!(chain(&ctx), vec!["a"]);
        assert!(!ctx.app_config.auto_failover_enabled);
        // 队列留着，回到路由模式恢复。
        assert_eq!(state.db.get_failover_queue("claude").unwrap().len(), 2);
    }

    #[tokio::test]
    #[serial]
    async fn official_accounts_cannot_be_stacked() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let mut official = claude("official", "https://api.anthropic.com", json!({}));
        official.category = Some("official".to_string());
        let rows = [claude("a", "https://a.example", json!({})), official];
        let state = state_with(AppType::Claude, &rows, "a").await;
        let error = set_stack_member(&state, &AppType::Claude, "official", true)
            .await
            .expect_err("official");
        assert!(!error.partial);
        assert!(stack_state().is_empty());
        let error = set_stack_member(&state, &AppType::Claude, "missing", true)
            .await
            .expect_err("missing");
        assert!(!error.partial);
    }

    #[tokio::test]
    #[serial]
    async fn a_failed_stack_change_says_whether_it_will_be_finished() {
        for (point, partial) in [
            ("pending", false),
            ("marked", true),
            ("published:0", true),
            ("target", true),
        ] {
            let _home = Home::new();
            seed_settings(USER_SETTINGS);
            let state = state_with(AppType::Claude, &stack_rows(), "a").await;
            enter(&state, &AppType::Claude, true).await.expect("enter");

            failpoint::crash_at(Some(point));
            let error = set_stack_member(&state, &AppType::Claude, "kimi", true)
                .await
                .expect_err(point);
            failpoint::crash_at(None);
            assert_eq!(error.partial, partial, "{point}: {error:?}");
            // 状态落定之前失败，已落定的名单还是旧的。
            assert_eq!(
                stack_state().is_member("kimi"),
                point == "target",
                "{point}"
            );

            // 再发一次同样的目标值：先补完（或丢弃）上一次，再应用，结果都是新名单。
            set_member(&state, "kimi", true).await;
            assert!(stack_state().is_member("kimi"), "{point}");
            assert!(picker().is_some(), "{point}");
            assert!(state::pending(&DeviceStore::for_device(), "claude")
                .unwrap()
                .is_none());
            state.proxy_service.stop().await.unwrap();
        }
    }

    #[tokio::test]
    #[serial]
    async fn operations_that_do_not_touch_the_list_keep_it() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        set_member(&state, "kimi", true).await;
        let stack = stack_state();

        // 一个不带 `stack` 的 pending（旧版本写的、或者换路由这类操作）前滚时不清名单。
        commit_state(
            &state,
            &AppType::Claude,
            &PendingTarget::mode(mode(&AppType::Claude)),
        )
        .unwrap();
        enter(&state, &AppType::Claude, false).await.expect("enter");
        exit(&state, &AppType::Claude).await.expect("exit");
        assert_eq!(stack_state(), stack);
    }

    #[tokio::test]
    #[serial]
    async fn editing_or_deleting_an_stacked_provider_rewrites_the_contract() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");
        set_member(&state, "kimi", true).await;

        let mut kimi = state
            .db
            .get_provider_by_id("kimi", "claude")
            .unwrap()
            .unwrap();
        kimi.settings_config["env"][CLAUDE_MAX_CONTEXT_ENV] = json!("64000");
        ProviderService::update(&state, AppType::Claude, None, kimi).expect("update kimi");
        assert_eq!(settings()["env"][CLAUDE_MAX_CONTEXT_ENV], "64000");

        ProviderService::delete(&state, AppType::Claude, "kimi").expect("delete kimi");
        let env = settings()["env"].clone();
        assert_eq!(
            picker(),
            Some(vec!["ccs-claude-a--claude-sonnet-4-6".to_string()]),
            "a still publishes"
        );
        assert!(env.get(CLAUDE_MAX_CONTEXT_ENV).is_none(), "{env}");
        assert!(state
            .db
            .get_provider_by_id("kimi", "claude")
            .unwrap()
            .is_none());
        let stack = stack_state();
        assert_eq!(stack.members, vec!["a"]);
        assert_eq!(stack.key_of("kimi"), Some("kimi"), "the key stays taken");
    }

    #[tokio::test]
    #[serial]
    async fn the_first_model_of_the_defaults_list_leads_the_aliases() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        enter(&state, &AppType::Claude, true).await.expect("enter");
        let mapped_contract = mode(&AppType::Claude).contract.unwrap();
        let sonnet = || settings()["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"].clone();
        assert_eq!(sonnet(), "ccs-claude-a--claude-sonnet-4-6");

        // 配了列表：整张照常发布，第一个占四档。
        let mut a = state.db.get_provider_by_id("a", "claude").unwrap().unwrap();
        a.meta.get_or_insert_with(Default::default).stack_models = serde_json::from_value(
            json!([{ "model": "a-vision", "displayName": "A Vision" }, { "model": "claude-sonnet-4-6" }]),
        )
        .unwrap();
        ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("update a");
        let views = stack_views(&state, &AppType::Claude).unwrap().members;
        assert!(views[0].route);
        assert_eq!(
            views[0].model_ids,
            vec!["ccs-claude-a--a-vision", "ccs-claude-a--claude-sonnet-4-6"]
        );
        assert_eq!(sonnet(), "ccs-claude-a--a-vision");
        assert_eq!(
            settings()["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"],
            "A Vision"
        );

        // 调整顺序就换了默认模型。
        a.meta
            .as_mut()
            .unwrap()
            .stack_models
            .as_mut()
            .unwrap()
            .reverse();
        ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("reorder a");
        assert_eq!(sonnet(), "ccs-claude-a--claude-sonnet-4-6");

        // 用户清空了列表：什么都不发布，四档回到路由契约的写法。
        a.meta.as_mut().unwrap().stack_models = Some(Vec::new());
        ProviderService::update(&state, AppType::Claude, None, a.clone()).expect("clear a");
        let views = stack_views(&state, &AppType::Claude).unwrap().members;
        assert!(views[0].model_ids.is_empty());
        assert!(!sonnet().as_str().unwrap().starts_with("ccs-claude-"));

        // 没配列表：回到按映射发布，第一个是 `ANTHROPIC_MODEL`。
        a.meta.as_mut().unwrap().stack_models = None;
        ProviderService::update(&state, AppType::Claude, None, a).expect("unset a");
        assert_eq!(mode(&AppType::Claude).contract.unwrap(), mapped_contract);
    }

    #[tokio::test]
    #[serial]
    async fn routing_mode_keeps_the_claude_aliases() {
        let _home = Home::new();
        seed_settings(USER_SETTINGS);
        let state = state_with(AppType::Claude, &stack_rows(), "a").await;
        set_member(&state, "kimi", true).await;
        enter(&state, &AppType::Claude, false)
            .await
            .expect("routing");
        let env = settings()["env"].clone();
        assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "claude-sonnet-5");
        assert_eq!(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-haiku-4-5");
        assert_eq!(picker(), None);
    }

    fn codex_native(id: &str, url: &str, extra: &str, catalog: Option<Value>) -> Provider {
        let mut provider = codex_row(id, url, extra);
        if let Some(catalog) = catalog {
            provider.settings_config["modelCatalog"] = catalog;
        }
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        provider
    }

    fn codex_stack_rows() -> [Provider; 3] {
        [
            codex_native(
                "a",
                "https://a.example/v1",
                "model_context_window = 200000\nmodel_auto_compact_token_limit = 150000\n",
                None,
            ),
            codex_native(
                "deepseek",
                "https://api.deepseek.com/v1",
                "",
                Some(json!({ "models": [
                    { "model": "deepseek-v4-pro", "displayName": "DeepSeek V4 Pro" }
                ]})),
            ),
            codex_native("zhipu", "https://open.bigmodel.cn/api/v1", "", None),
        ]
    }

    async fn set_codex_member(state: &AppState, id: &str, enabled: bool) -> Vec<StackMemberView> {
        set_stack_member(state, &AppType::Codex, id, enabled)
            .await
            .unwrap_or_else(|error| panic!("set {id}={enabled}: {error:?}"));
        stack_views(state, &AppType::Codex).unwrap().members
    }

    fn codex_catalog() -> Value {
        crate::config::read_json_file(&crate::codex_config::get_codex_model_catalog_path()).unwrap()
    }

    #[tokio::test]
    #[serial]
    async fn codex_stack_models_join_the_catalog_and_leave_with_it() {
        let _home = Home::new();
        seed_codex("approval_policy = \"on-request\"\n", None);
        let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        let plain_text = codex_text();
        let plain_contract = mode(&AppType::Codex).contract.unwrap();
        assert_eq!(
            codex_doc()["model_context_window"].as_integer(),
            Some(200000)
        );

        // 默认那家（a）在名单最前面，不发布。
        let views = set_codex_member(&state, "deepseek", true).await;
        assert!(views[0].route);
        assert_eq!(views[1].model_ids, vec!["ccs-deepseek/deepseek-v4-pro"]);
        let doc = codex_doc();
        assert_eq!(
            doc["model"].as_str(),
            Some("gpt-a"),
            "the route keeps the default model"
        );
        assert_eq!(
            doc["model_catalog_json"].as_str(),
            Some(crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
        // 窗口类全局键会覆盖每一行，改写进路由那家自己的行。
        assert!(doc.get("model_context_window").is_none(), "{doc:?}");
        assert!(
            doc.get("model_auto_compact_token_limit").is_none(),
            "{doc:?}"
        );

        let catalog = codex_catalog();
        let models = catalog["models"].as_array().unwrap();
        let slugs: Vec<&str> = models.iter().map(|m| m["slug"].as_str().unwrap()).collect();
        assert_eq!(slugs, vec!["gpt-a", "ccs-deepseek/deepseek-v4-pro"]);
        let (route, stacked) = (&models[0], &models[1]);
        assert_eq!(route["priority"], 1);
        assert_eq!(route["context_window"], 200000);
        assert_eq!(route["auto_compact_token_limit"], 150000);
        // 路由那家的行保持模板的值（这里模板没有），加进第一家不会让路由上的会话被压缩。
        assert_eq!(route["comp_hash"], Value::Null);
        assert_eq!(stacked["priority"], 2);
        assert_eq!(stacked["display_name"], "DeepSeek V4 Pro（DEEPSEEK）");
        // DeepSeek 官方目录的 "3000" 不带过来，窗口按它自己的行算。
        assert_eq!(stacked["comp_hash"], "cc-switch");
        let window = stacked["context_window"].as_u64().unwrap();
        assert_eq!(
            stacked["auto_compact_token_limit"].as_u64(),
            Some(window * 9 / 10)
        );

        // 没有配置模型目录的行只发布它的 `model`。
        let views = set_codex_member(&state, "zhipu", true).await;
        assert_eq!(views[2].model_ids, vec!["ccs-zhipu/gpt-zhipu"]);
        let slugs: Vec<String> = codex_catalog()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["slug"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            slugs,
            vec![
                "gpt-a",
                "ccs-deepseek/deepseek-v4-pro",
                "ccs-zhipu/gpt-zhipu"
            ]
        );

        // 只剩默认那家：config.toml 和契约回到没有 Stack 模型时的样子。
        set_codex_member(&state, "deepseek", false).await;
        set_codex_member(&state, "zhipu", false).await;
        assert_eq!(codex_text(), plain_text);
        assert_eq!(mode(&AppType::Codex).contract.unwrap(), plain_contract);

        exit(&state, &AppType::Codex).await.expect("exit");
        assert!(!state
            .proxy_service
            .live_has_proxy_placeholder(&AppType::Codex));
    }

    /// 路由那家自己管理模型目录文件：Stack 模型发布不了，名单照存，结果带提示而不是静默成功。
    #[tokio::test]
    #[serial]
    async fn codex_route_with_its_own_catalog_reports_stack_models_unpublished() {
        let _home = Home::new();
        seed_codex("", None);
        let [_, deepseek, zhipu] = codex_stack_rows();
        let route = codex_native(
            "a",
            "https://a.example/v1",
            "model_catalog_json = \"/opt/team/models.json\"\n",
            None,
        );
        let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        let plain_text = codex_text();

        let notice = set_stack_member(&state, &AppType::Codex, "deepseek", true)
            .await
            .expect("stack");
        assert_eq!(notice, Some("routeOwnsCatalog"));
        assert!(stack_state_of(&AppType::Codex).is_member("deepseek"));
        assert_eq!(codex_text(), plain_text, "nothing to publish");
        assert_eq!(
            stack_views(&state, &AppType::Codex).unwrap().notice,
            Some("routeOwnsCatalog")
        );

        // 名单清空后不再提示。
        let notice = set_stack_member(&state, &AppType::Codex, "deepseek", false)
            .await
            .expect("detach");
        assert_eq!(notice, None);
    }

    /// 假的进程表和时钟（见 `codex_client_catalog::Env`），结束时换回真的。
    struct FakeClients {
        now_ms: Arc<std::sync::atomic::AtomicU64>,
        table: Arc<std::sync::Mutex<String>>,
    }

    impl FakeClients {
        const START_MS: u64 = 1_800_000_000_000;

        fn install() -> Self {
            let now_ms = Arc::new(std::sync::atomic::AtomicU64::new(Self::START_MS));
            let table = Arc::new(std::sync::Mutex::new(String::new()));
            let (clock, rows) = (now_ms.clone(), table.clone());
            codex_client_catalog::set_test_env(codex_client_catalog::Env {
                process_table: Box::new(move || Some(rows.lock().unwrap().clone())),
                now_ms: Box::new(move || clock.load(std::sync::atomic::Ordering::SeqCst)),
                restart: Box::new(|_| Err("not in tests".to_string())),
            });
            Self { now_ms, table }
        }

        /// 桌面版的 app-server，已经跑了 `etime`（`ps` 的格式）。
        fn desktop_running_for(&self, etime: &str) {
            *self.table.lock().unwrap() = format!(
                "62347 {etime} /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex -c features.code_mode_host=true app-server"
            );
        }

        fn advance(&self, ms: u64) {
            self.now_ms
                .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Drop for FakeClients {
        fn drop(&mut self) {
            codex_client_catalog::reset_test_env();
        }
    }

    async fn stale_clients_of(state: &AppState) -> Option<codex_client_catalog::StaleClients> {
        stack_view_with_clients(state, &AppType::Codex)
            .await
            .expect("stack view")
            .stale_clients
    }

    /// 桌面版在目录变化之前启动：Stack 视图带上 `staleClients`；之后启动的不算旧。路由模式、
    /// 路由那家自己管理目录（Stack 模型本来就不发布）时不查。
    #[tokio::test]
    #[serial]
    async fn codex_stack_view_reports_clients_on_an_old_catalog() {
        let _home = Home::new();
        let clients = FakeClients::install();
        seed_codex("", None);
        clients.desktop_running_for("10:00");
        let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(
            stale_clients_of(&state).await,
            Some(codex_client_catalog::StaleClients {
                daemon: false,
                others: true
            })
        );
        // 普通的 Stack 视图不读进程表。
        assert_eq!(
            stack_views(&state, &AppType::Codex).unwrap().stale_clients,
            None
        );

        // 桌面版重开之后启动：读到的正是现在这份目录。
        clients.advance(10_000);
        clients.desktop_running_for("00:05");
        assert_eq!(stale_clients_of(&state).await, None);

        // 目录又变了（再加一家）：刚才那个桌面版又旧了。
        clients.advance(10_000);
        set_codex_member(&state, "zhipu", true).await;
        clients.desktop_running_for("00:15");
        assert!(stale_clients_of(&state).await.is_some());

        // 换成路由模式：不是 Stack 模式，不查。
        enter(&state, &AppType::Codex, false)
            .await
            .expect("routing");
        clients.desktop_running_for("10:00");
        assert_eq!(stale_clients_of(&state).await, None);
    }

    #[tokio::test]
    #[serial]
    async fn codex_route_with_its_own_catalog_skips_the_client_check() {
        let _home = Home::new();
        let clients = FakeClients::install();
        seed_codex("", None);
        clients.desktop_running_for("10:00");
        let [_, deepseek, zhipu] = codex_stack_rows();
        let route = codex_native(
            "a",
            "https://a.example/v1",
            "model_catalog_json = \"/opt/team/models.json\"\n",
            None,
        );
        let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        let view = stack_view_with_clients(&state, &AppType::Codex)
            .await
            .unwrap();
        assert_eq!(view.notice, Some("routeOwnsCatalog"));
        assert_eq!(view.stale_clients, None);
    }

    /// 用户直接在 config.toml 里指定的模型目录：写入时照留，生成的目录不生效，同样要提示。
    /// 手写进 config.toml 的指针挡不住聚合：进代理时按关键字段清掉，加进 Stack 后换上合并目录。
    #[tokio::test]
    #[serial]
    async fn codex_a_hand_written_catalog_pointer_gives_way_to_the_stack_catalog() {
        let _home = Home::new();
        seed_codex("model_catalog_json = \"/work/global-models.json\"\n", None);
        let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        assert!(
            codex_doc().get("model_catalog_json").is_none(),
            "{}",
            codex_text()
        );

        let notice = set_stack_member(&state, &AppType::Codex, "deepseek", true)
            .await
            .expect("stack");
        assert_eq!(notice, None);
        assert_eq!(
            codex_doc()["model_catalog_json"].as_str(),
            Some(crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
    }

    /// 提示上的「改用 CC Switch 的模型目录」：去掉路由那家行里的指针，契约带进 live 的那份
    /// 跟着删掉，换上合并目录，不再提示。行里其余内容原样。
    #[tokio::test]
    #[serial]
    async fn codex_adopting_the_catalog_drops_the_route_rows_own_pointer() {
        let _home = Home::new();
        seed_codex("", None);
        let [_, deepseek, zhipu] = codex_stack_rows();
        let route = codex_native(
            "a",
            "https://a.example/v1",
            "model_catalog_json = \"/opt/team/models.json\"\nmodel_verbosity = \"high\"\n",
            None,
        );
        let state = state_with(AppType::Codex, &[route, deepseek, zhipu], "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(
            codex_doc()["model_catalog_json"].as_str(),
            Some("/opt/team/models.json")
        );

        let notice = adopt_codex_stack_catalog(&state).await.expect("adopt");
        assert_eq!(notice, None);
        let row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let row_config = row.settings_config["config"].as_str().unwrap();
        assert!(
            !row_config.contains("model_catalog_json") && row_config.contains("model_verbosity"),
            "{row_config}"
        );
        assert_eq!(
            codex_doc()["model_catalog_json"].as_str(),
            Some(crate::codex_config::CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
        let slugs: Vec<String> = codex_catalog()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["slug"].as_str().unwrap().to_string())
            .collect();
        assert!(
            slugs.contains(&"ccs-deepseek/deepseek-v4-pro".to_string()),
            "{slugs:?}"
        );
        assert_eq!(stack_views(&state, &AppType::Codex).unwrap().notice, None);
    }

    /// 编辑器里的模型目录指针归这张卡：live 里手写的不显示、不收进行，保存当前卡时清掉；用户在
    /// 编辑器里写的外来指针存进行、随卡写入；指向 CC Switch 目录的指针不存（由写入方决定）。
    #[tokio::test]
    #[serial]
    async fn codex_editor_catalog_pointer_belongs_to_the_card() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(
            &format!("model_catalog_json = \"/work/mine.json\"\n{CODEX_USER_LIVE}"),
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
        let row_config = |id: &str| {
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
        let with_pointer = |base: &Value, pointer: &str| {
            let mut edited = base.clone();
            edited["config"] = json!(format!(
                "model_catalog_json = {pointer:?}\n{}",
                base["config"].as_str().unwrap()
            ));
            edited
        };
        let pointer = || {
            codex_doc()
                .get("model_catalog_json")
                .and_then(|value| value.as_str().map(str::to_string))
        };

        let (row, base) = open("a");
        assert!(
            !base["config"]
                .as_str()
                .unwrap()
                .contains("model_catalog_json"),
            "{base}"
        );
        save(row, base.clone(), base).expect("save as is");
        assert!(!row_config("a").contains("model_catalog_json"));
        assert_eq!(pointer(), None, "{}", codex_text());

        // 手写 CC Switch 自己的目录：a 没有模型映射，不生成目录，行里不存、live 里也不写。
        let ours = crate::codex_config::get_codex_model_catalog_path()
            .display()
            .to_string();
        let (row, base) = open("a");
        save(row, with_pointer(&base, &ours), base).expect("save ours");
        assert!(!row_config("a").contains("model_catalog_json"));
        assert_eq!(pointer(), None, "{}", codex_text());

        let (row, base) = open("b");
        save(row, with_pointer(&base, "/work/b.json"), base).expect("save b pointer");
        assert!(
            row_config("b").contains("model_catalog_json = \"/work/b.json\""),
            "{}",
            row_config("b")
        );
        ProviderService::switch(&state, AppType::Codex, "b").expect("to b");
        assert_eq!(pointer().as_deref(), Some("/work/b.json"));
        ProviderService::switch(&state, AppType::Codex, "a").expect("to a");
        assert_eq!(pointer(), None, "{}", codex_text());
    }

    /// 普通保存入口（不带编辑器底）改了 Stack 里那家的模型目录：合并目录跟着重算。
    #[tokio::test]
    #[serial]
    async fn editing_an_stacked_codex_provider_rewrites_the_catalog() {
        let _home = Home::new();
        seed_codex("", None);
        let state = state_with(AppType::Codex, &codex_stack_rows(), "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;

        let mut deepseek = state
            .db
            .get_provider_by_id("deepseek", "codex")
            .unwrap()
            .unwrap();
        deepseek.settings_config["modelCatalog"] = json!({ "models": [
            { "model": "deepseek-v5" }
        ]});
        ProviderService::update(&state, AppType::Codex, None, deepseek).expect("update deepseek");
        let slugs: Vec<String> = codex_catalog()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["slug"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(slugs, vec!["gpt-a", "ccs-deepseek/deepseek-v5"]);
    }

    #[tokio::test]
    #[serial]
    async fn codex_a_broken_stacked_provider_is_skipped_and_cannot_be_stacked() {
        let _home = Home::new();
        seed_codex("", None);
        let mut rows = codex_stack_rows().to_vec();
        let mut broken = codex_native("broken", "https://b.example/v1", "", None);
        broken.settings_config["config"] = json!("model = [\n");
        rows.push(broken);
        let state = state_with(AppType::Codex, &rows, "a").await;
        enter(&state, &AppType::Codex, true).await.expect("enter");

        // 配置解析不了的行不能加进 Stack，什么都不改。
        let error = set_stack_member(&state, &AppType::Codex, "broken", true)
            .await
            .unwrap_err();
        assert!(!error.partial);
        assert_eq!(stack_state_of(&AppType::Codex).members, vec!["a"]);

        // 加进 Stack 之后才坏掉（比如云同步直接换了行）：只跳过这一家，其余照常发布，
        // 重写和换路由都不受影响。
        set_codex_member(&state, "deepseek", true).await;
        set_codex_member(&state, "zhipu", true).await;
        assert_eq!(
            catalog_slugs(),
            vec![
                "gpt-a",
                "ccs-deepseek/deepseek-v4-pro",
                "ccs-zhipu/gpt-zhipu"
            ]
        );
        let mut zhipu = state
            .db
            .get_provider_by_id("zhipu", "codex")
            .unwrap()
            .unwrap();
        zhipu.settings_config["config"] = json!("model = [\n");
        state.db.save_provider("codex", &zhipu).unwrap();
        resync_route(&state, &AppType::Codex).await.expect("resync");
        assert_eq!(
            catalog_slugs(),
            vec!["gpt-a", "ccs-deepseek/deepseek-v4-pro"]
        );
        switch_route(&state, &AppType::Codex, "deepseek")
            .await
            .expect("switch route");
        assert_eq!(
            mode(&AppType::Codex).proxy_route.as_deref(),
            Some("deepseek")
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_official_accounts_cannot_be_stacked() {
        let _home = Home::new();
        seed_codex("", None);
        let mut rows = codex_stack_rows().to_vec();
        rows.push(codex_official());
        let state = state_with(AppType::Codex, &rows, "a").await;
        let error = set_stack_member(
            &state,
            &AppType::Codex,
            crate::database::CODEX_OFFICIAL_PROVIDER_ID,
            true,
        )
        .await
        .expect_err("official");
        assert!(!error.partial);
        assert!(stack_state_of(&AppType::Codex).members.is_empty());
    }

    fn stack_state_of(app: &AppType) -> StackState {
        state::stack(&DeviceStore::for_device(), app.as_str()).unwrap()
    }

    // ---- Stack 模型：Codex 官方做路由 ----

    use crate::services::provider::codex_official_models::testing::{
        login as chatgpt, native_models, Calls, Fake,
    };
    use crate::services::provider::codex_official_models::{self as official_models, Fetch};
    use crate::services::subscription::CodexKeychainLogin;

    /// 换上假的官方接口；结束时换回来。
    struct FakeModels {
        calls: Calls,
        now: Arc<std::sync::Mutex<u64>>,
        responses: Arc<std::sync::Mutex<Vec<Fetch>>>,
        keychain_reads: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Drop for FakeModels {
        fn drop(&mut self) {
            official_models::reset_test_env();
        }
    }

    const NOW: u64 = 1_800_000_000;

    fn fake_models(
        responses: Vec<Fetch>,
        keychain: CodexKeychainLogin,
        on_fetch: Option<Box<dyn Fn() + Send + Sync>>,
    ) -> FakeModels {
        fake_models_sharing(
            responses,
            Arc::new(std::sync::Mutex::new(keychain)),
            on_fetch,
        )
    }

    /// 同 [`fake_models`]，钥匙串由调用方持有，可以在拉取时换掉。
    fn fake_models_sharing(
        responses: Vec<Fetch>,
        keychain: Arc<std::sync::Mutex<CodexKeychainLogin>>,
        on_fetch: Option<Box<dyn Fn() + Send + Sync>>,
    ) -> FakeModels {
        let calls: Calls = Arc::default();
        let now = Arc::new(std::sync::Mutex::new(NOW));
        let responses = Arc::new(std::sync::Mutex::new(responses));
        let keychain_reads: Arc<std::sync::atomic::AtomicUsize> = Arc::default();
        Fake {
            version: Some("0.158.0".to_string()),
            responses: responses.clone(),
            bundled: None,
            keychain,
            keychain_reads: keychain_reads.clone(),
            now: now.clone(),
            calls: calls.clone(),
            on_fetch,
        }
        .install();
        FakeModels {
            calls,
            now,
            responses,
            keychain_reads,
        }
    }

    fn official_list(extra: &[(&str, i64)]) -> Fetch {
        let mut slugs = vec![("gpt-6-sol", 4), ("gpt-5.5", 12)];
        slugs.extend_from_slice(extra);
        Fetch::Models {
            models: native_models(&slugs),
            etag: Some("\"e1\"".to_string()),
        }
    }

    fn catalog_slugs() -> Vec<String> {
        codex_catalog()["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["slug"].as_str().unwrap().to_string())
            .collect()
    }

    fn codex_official_stack_rows() -> Vec<Provider> {
        let mut rows = vec![codex_official()];
        rows.extend(codex_stack_rows());
        rows
    }

    #[tokio::test]
    #[serial]
    async fn codex_official_route_lists_every_official_model_before_the_stacked_ones() {
        let _home = Home::new();
        let alice = chatgpt("ws", "alice");
        seed_codex("", Some(&alice));
        let auth_bytes = fs::read(codex_auth_path()).unwrap();
        let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        assert!(
            fake.calls.lock().unwrap().is_empty(),
            "no stacked models, no fetch"
        );

        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(
            catalog_slugs(),
            vec!["gpt-6-sol", "gpt-5.5", "ccs-deepseek/deepseek-v4-pro"]
        );
        let catalog = codex_catalog();
        let sol = &catalog["models"][0];
        // 原生字段保留，旧的指令字段补上。
        assert_eq!(sol["comp_hash"], "3000");
        assert_eq!(sol["base_instructions"], "T");
        // 取列表用的是 Codex 实际会用的登录；整个过程不写 auth.json。
        assert_eq!(
            fake.calls.lock().unwrap().clone(),
            vec![("ws|sub:alice".to_string(), "0.158.0".to_string(), None)]
        );
        assert_eq!(fs::read(codex_auth_path()).unwrap(), auth_bytes);
        assert!(stack_views(&state, &AppType::Codex)
            .unwrap()
            .notice
            .is_none());

        // 后台检查：一小时后是新的，不联网；七小时后刷新，列表变了就重写目录。
        *fake.now.lock().unwrap() = NOW + 3600;
        check_codex_official_models(&state).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        *fake.now.lock().unwrap() = NOW + 7 * 3600;
        fake.responses
            .lock()
            .unwrap()
            .push(official_list(&[("gpt-6-luna", 5)]));
        check_codex_official_models(&state).await;
        assert_eq!(
            fake.calls.lock().unwrap().last().unwrap().2.as_deref(),
            Some("\"e1\""),
            "the refresh revalidates with the etag"
        );
        assert!(catalog_slugs().contains(&"gpt-6-luna".to_string()));

        // 名单清空：不再写目录。
        set_codex_member(&state, "deepseek", false).await;
        assert!(codex_doc().get("model_catalog_json").is_none());
        exit(&state, &AppType::Codex).await.expect("exit");
        assert_eq!(fs::read(codex_auth_path()).unwrap(), auth_bytes);
    }

    /// 列表变了而重写失败（这里是 config.toml 恰好解析不了）：缓存已经是新的，下一次检查
    /// 不再联网、拿到的也不会是「列表变了」，但仍按缓存补写客户端文件。
    #[tokio::test]
    #[serial]
    async fn codex_a_failed_rewrite_after_a_refresh_is_retried_at_the_next_check() {
        let _home = Home::new();
        seed_codex("", Some(&chatgpt("ws", "alice")));
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let break_config: Box<dyn Fn() + Send + Sync> = {
            let fetches = fetches.clone();
            Box::new(move || {
                // 第二次拉取（后台刷新）时 config.toml 恰好坏了。
                if fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                    fs::write(codex_config_path(), "model = [\n").unwrap();
                }
            })
        };
        let fake = fake_models(
            vec![official_list(&[]), official_list(&[("gpt-6-luna", 5)])],
            CodexKeychainLogin::Missing,
            Some(break_config),
        );
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        let good = codex_text();

        *fake.now.lock().unwrap() = NOW + 7 * 3600;
        check_codex_official_models(&state).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        assert!(!catalog_slugs().contains(&"gpt-6-luna".to_string()));

        fs::write(codex_config_path(), &good).unwrap();
        check_codex_official_models(&state).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 2, "the cache is fresh");
        assert!(catalog_slugs().contains(&"gpt-6-luna".to_string()));

        // 补写成功后标记清掉：再检查什么都不做。
        let written = codex_text();
        check_codex_official_models(&state).await;
        assert_eq!(codex_text(), written);
    }

    #[tokio::test]
    #[serial]
    async fn codex_a_login_that_changes_before_the_write_stops_it() {
        let _home = Home::new();
        seed_codex("", Some(&chatgpt("ws", "alice")));
        // 拿锁前按 alice 取了列表，之后 Codex 恰好换成同一工作区的 bob 登录。
        let _fake = fake_models(
            vec![official_list(&[])],
            CodexKeychainLogin::Missing,
            Some(Box::new(|| {
                fs::write(codex_auth_path(), chatgpt("ws", "bob").to_string()).unwrap();
            })),
        );
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        let before = codex_text();

        let error = set_stack_member(&state, &AppType::Codex, "deepseek", true)
            .await
            .expect_err("login changed");
        assert!(!error.partial, "{}", error.message);
        assert_eq!(codex_text(), before);
        assert!(!crate::codex_config::get_codex_model_catalog_path().exists());
        assert!(stack_state_of(&AppType::Codex).members.is_empty());
    }

    /// 钥匙串不归写锁管：按 alice 取列表的时候 Codex 在钥匙串里换成了 bob，不能把 alice
    /// 的列表写给 bob。
    #[tokio::test]
    #[serial]
    async fn codex_a_keychain_login_that_changes_during_the_fetch_stops_the_write() {
        let _home = Home::new();
        seed_codex("cli_auth_credentials_store = \"keyring\"\n", None);
        let keychain = Arc::new(std::sync::Mutex::new(CodexKeychainLogin::Found(chatgpt(
            "ws", "alice",
        ))));
        let switched = keychain.clone();
        let fake = fake_models_sharing(
            vec![official_list(&[])],
            keychain,
            Some(Box::new(move || {
                *switched.lock().unwrap() = CodexKeychainLogin::Found(chatgpt("ws", "bob"));
            })),
        );
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        let before = codex_text();

        let error = set_stack_member(&state, &AppType::Codex, "deepseek", true)
            .await
            .expect_err("login changed");
        assert!(!error.partial, "{}", error.message);
        assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
        assert_eq!(codex_text(), before);
        assert!(!crate::codex_config::get_codex_model_catalog_path().exists());
        assert!(stack_state_of(&AppType::Codex).members.is_empty());

        // 重试按 bob 重新取。
        *fake.responses.lock().unwrap() = vec![official_list(&[])];
        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(fake.calls.lock().unwrap()[1].0, "ws|sub:bob");
        assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));
    }

    #[tokio::test]
    #[serial]
    async fn codex_the_login_comes_from_where_codex_keeps_it() {
        let _home = Home::new();
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;

        // keyring：auth.json 里残留 alice，Codex 用的是钥匙串里的 bob。
        seed_codex(
            "cli_auth_credentials_store = \"keyring\"\n",
            Some(&chatgpt("ws", "alice")),
        );
        let fake = fake_models(
            vec![official_list(&[])],
            CodexKeychainLogin::Found(chatgpt("ws", "bob")),
            None,
        );
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), official).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:bob");
        // 预测登录读一次，取完列表再读一次给拿锁后的核对用；写锁里不读。
        assert_eq!(
            fake.keychain_reads
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        exit(&state, &AppType::Codex).await.expect("exit");
        set_codex_member(&state, "deepseek", false).await;
        drop(fake);

        // auto：钥匙串读不出来（Windows、Linux，或访问被拒）时不知道 Codex 用的是谁，
        // 不拿 auth.json 里的 alice 顶替。
        seed_codex(
            "cli_auth_credentials_store = \"auto\"\n",
            Some(&chatgpt("ws", "alice")),
        );
        let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Unknown, None);
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert!(fake.calls.lock().unwrap().is_empty());
        assert_eq!(
            stack_views(&state, &AppType::Codex).unwrap().notice,
            Some("officialModelsUnavailable")
        );
        exit(&state, &AppType::Codex).await.expect("exit");
        set_codex_member(&state, "deepseek", false).await;
        drop(fake);

        // auto：钥匙串里确定没有，Codex 退回 auth.json。
        seed_codex(
            "cli_auth_credentials_store = \"auto\"\n",
            Some(&chatgpt("ws", "alice")),
        );
        let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
        exit(&state, &AppType::Codex).await.expect("exit");
        set_codex_member(&state, "deepseek", false).await;
        drop(fake);

        // ephemeral：登录从不落盘，不拉取；没有自带列表时 Stack 模型暂不可用，也不写目录。
        seed_codex(
            "cli_auth_credentials_store = \"ephemeral\"\n",
            Some(&chatgpt("ws", "alice")),
        );
        let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
        enter(&state, &AppType::Codex, true).await.expect("enter");
        set_codex_member(&state, "deepseek", true).await;
        assert!(fake.calls.lock().unwrap().is_empty());
        assert!(codex_doc().get("model_catalog_json").is_none());
        assert_eq!(
            stack_views(&state, &AppType::Codex).unwrap().notice,
            Some("officialModelsUnavailable")
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_switching_back_to_official_uses_the_login_it_will_restore() {
        let _home = Home::new();
        set_preservation(false);
        seed_codex("", Some(&chatgpt("ws", "alice")));
        let fake = fake_models(vec![official_list(&[])], CodexKeychainLogin::Missing, None);
        let state = state_with(AppType::Codex, &codex_official_stack_rows(), "a").await;
        // 直连切到第三方：登录存进暂存，auth.json 删掉。
        ProviderService::switch(&state, AppType::Codex, "a").expect("direct a");
        assert!(!codex_auth_path().exists());
        set_codex_member(&state, "deepseek", true).await;
        enter(&state, &AppType::Codex, true).await.expect("enter");
        assert!(fake.calls.lock().unwrap().is_empty(), "third-party route");

        // 换路由到官方卡：auth.json 会从暂存还回 alice，列表按 alice 取。
        let official = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        ProviderService::switch(&state, AppType::Codex, official).expect("route to official");
        assert_eq!(fake.calls.lock().unwrap()[0].0, "ws|sub:alice");
        assert!(catalog_slugs().contains(&"gpt-6-sol".to_string()));
        let restored: Value = crate::config::read_json_file(&codex_auth_path()).unwrap();
        assert_eq!(restored["tokens"]["account_id"], "ws");
    }

    /// `src/config/codexTemplates.ts` 的 `getCodexCustomTemplate()`：Key 为空、
    /// `requires_openai_auth = true`。新增对话框一打开就投影它。
    fn codex_keyless_template() -> Value {
        json!({
            "auth": { "OPENAI_API_KEY": "" },
            "config": "model_provider = \"custom\"\nmodel = \"gpt-5.6-sol\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
        })
    }

    /// 没填 Key 的草稿：显示「切过去之后的样子」和只存行都不拿切换的安全闸拒绝，要进 live
    /// 时（路由到它、直连切到它）照样拒绝，否则会把 ChatGPT 登录发给第三方。
    #[tokio::test]
    #[serial]
    async fn codex_keyless_draft_is_shown_and_saved_but_never_written_to_live() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex("model = \"gpt-5.4\"\n", Some(&chatgpt_login("acct")));
        let [a, b] = codex_a_b();
        let official = codex_official();
        let state = state_with(AppType::Codex, &[a, b, official.clone()], &official.id).await;
        ProviderService::switch(&state, AppType::Codex, &official.id).expect("official");
        let template = codex_keyless_template();
        let view = |state: &AppState, label: &str| -> Value {
            let view = ProviderService::editor_view(state, AppType::Codex, &template, None)
                .unwrap_or_else(|err| panic!("{label}: cannot show the template: {err}"));
            let shown = view.settings["config"].as_str().unwrap().to_string();
            assert!(!shown.contains("pending-key"), "{label}: {shown}");
            let doc: toml::Table = toml::from_str(&shown).unwrap();
            assert_eq!(
                doc["model_provider"].as_str(),
                Some("custom"),
                "{label}: {shown}"
            );
            assert!(doc.get("openai_base_url").is_none(), "{label}: {shown}");
            let route = &doc["model_providers"]["custom"];
            assert_eq!(
                route["requires_openai_auth"].as_bool(),
                Some(true),
                "{label}: {shown}"
            );
            assert!(
                route.get("experimental_bearer_token").is_none(),
                "{label}: {shown}"
            );
            view.settings
        };

        // 直连（当前是官方卡）。
        view(&state, "direct");
        // 代理的官方路由：live 顶层是 openai_base_url。
        enter(&state, &AppType::Codex, false).await.expect("enter");
        let routed = codex_text();
        assert!(routed.contains("openai_base_url"), "{routed}");
        let shown = view(&state, "official proxy route");

        // 表单确认过「不填 Key 也保存」：只存行，live 不动，占位 Key 不进行。
        let draft = Provider::with_id("c".into(), "C".into(), template.clone(), None);
        add_from_editor(&state, AppType::Codex, draft, shown.clone(), shown)
            .expect("a keyless row can be saved");
        assert_eq!(codex_text(), routed);
        let c = state.db.get_provider_by_id("c", "codex").unwrap().unwrap();
        let c_config = c.settings_config["config"].as_str().unwrap();
        assert!(!c_config.contains("pending-key"), "{c_config}");
        assert!(
            !c_config.contains("experimental_bearer_token"),
            "{c_config}"
        );
        assert!(
            c_config.contains("requires_openai_auth = true"),
            "{c_config}"
        );

        // 路由到它、直连切到它都拒绝，live 不动。
        let err = ProviderService::switch(&state, AppType::Codex, "c").expect_err("route to c");
        assert!(err.to_string().contains("requires_openai_auth"), "{err}");
        assert_eq!(codex_text(), routed);
        exit(&state, &AppType::Codex).await.expect("exit");
        let direct_live = codex_text();
        let err = ProviderService::switch(&state, AppType::Codex, "c").expect_err("switch to c");
        assert!(err.to_string().contains("requires_openai_auth"), "{err}");
        assert_eq!(codex_text(), direct_live);
        assert_eq!(
            direct(&state, &AppType::Codex).as_deref(),
            Some(official.id.as_str())
        );
    }

    /// 直连模式下编辑当前供应商：保存会把关键字段一起换进 live，这时去掉 Key 照样拒绝，行
    /// 撤回、live 不动。
    #[tokio::test]
    #[serial]
    async fn codex_editing_the_current_provider_into_a_keyless_row_is_refused() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex(CODEX_USER_LIVE, Some(&chatgpt_login("acct")));
        let state = state_with(AppType::Codex, &codex_a_b(), "a").await;
        ProviderService::switch(&state, AppType::Codex, "a").expect("a");
        let before_live = codex_text();
        let mut row = state.db.get_provider_by_id("a", "codex").unwrap().unwrap();
        let before_row = row.settings_config.clone();

        let base = ProviderService::editor_view(&state, AppType::Codex, &row.settings_config, None)
            .expect("view")
            .settings;
        let mut doc: toml_edit::DocumentMut = base["config"].as_str().unwrap().parse().unwrap();
        doc["model_providers"]["custom"]["requires_openai_auth"] = toml_edit::value(true);
        let mut edited = base.clone();
        edited["config"] = json!(doc.to_string());
        edited["auth"]["OPENAI_API_KEY"] = json!("");
        row.settings_config = edited;
        let err = ProviderService::update_from_editor(
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
        .expect_err("the current provider cannot lose its key");
        assert!(err.to_string().contains("requires_openai_auth"), "{err}");
        assert_eq!(codex_text(), before_live);
        assert_eq!(
            state
                .db
                .get_provider_by_id("a", "codex")
                .unwrap()
                .unwrap()
                .settings_config,
            before_row
        );
    }

    /// 新增第一个供应商会同时写进 live：没填 Key 照样拒绝，行不留。
    #[tokio::test]
    #[serial]
    async fn codex_adding_a_keyless_first_provider_is_refused() {
        let _home = Home::new();
        set_preservation(true);
        seed_codex("model = \"gpt-5.4\"\n", Some(&chatgpt_login("acct")));
        let state = state_without_providers().await;
        let before = codex_text();
        let template = codex_keyless_template();
        let base = ProviderService::editor_view(&state, AppType::Codex, &template, None)
            .expect("view")
            .settings;
        let draft = Provider::with_id("c".into(), "C".into(), template, None);
        let err = add_from_editor(&state, AppType::Codex, draft, base.clone(), base)
            .expect_err("a keyless first provider would go live");
        assert!(err.to_string().contains("requires_openai_auth"), "{err}");
        assert!(state.db.get_provider_by_id("c", "codex").unwrap().is_none());
        assert_eq!(codex_text(), before);
    }
}
