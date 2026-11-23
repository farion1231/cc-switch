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

use crate::sync_utils::MutexExt;
use serde_json::{json, Value};
use tokio::sync::OwnedMutexGuard;

use crate::app_config::AppType;
use crate::claude_desktop_config::{self, DesktopTarget};
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::live::project::claude::{
    direct_patch, proxy_projection, ClaudeProjection, ProxyAuth, StackRoleModel,
    PROXY_TOKEN_PLACEHOLDER,
};
use crate::live::project::gemini::GeminiProjection;
use crate::live::project::grok::GrokProjection;
use crate::provider::Provider;
use crate::proxy::loopback_probe;
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

/// 有路由模式的应用（[`AppSpec::route_mode`](crate::app_config::AppSpec::route_mode)）：本地路由的
/// 在前，Claude Desktop 在最后（它经同一个路由服务转发，只是没有故障转移）。
fn route_mode_apps() -> impl Iterator<Item = AppType> {
    let apps = || AppType::all().filter(|app| app.spec().route_mode);
    apps()
        .filter(|app| app.spec().local_proxy)
        .chain(apps().filter(|app| !app.spec().local_proxy))
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn provider(state: &AppState, app: &AppType, id: &str) -> Result<Option<Provider>, String> {
    state.db.get_provider_by_id(id, app).map_err(err)
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
        ProxyAuth::AuthToken
    }
}

/// `stack` 是发布的 Stack 模型，`stack_default` 是四档别名指向的模型（Stack 模式下默认那家
/// 列表里的第一个，见 [`stack::claude_route_default`]；路由模式为 `None`）。`others` 是其余
/// 发布模型的成员，它们的兼容开关按 [`with_member_switches`] 并进契约。
fn claude_contract(
    route: &Provider,
    proxy_url: &str,
    stack: &[StackModel],
    stack_default: Option<&StackModel>,
    others: &[&Provider],
) -> (ClaudeProjection, Contract) {
    let mut projection = proxy_projection(
        &ClaudeProjection::of(&route.settings_config),
        &route.name,
        proxy_url,
        claude_proxy_auth(route),
        stack_default.map(|model| StackRoleModel {
            id: &model.id,
            name: &model.name,
        }),
    );
    with_stack_models(&mut projection, stack);
    if !stack.is_empty() {
        let others: Vec<_> = others
            .iter()
            .map(|provider| ClaudeProjection::of(&provider.settings_config).exclusive)
            .collect();
        with_member_switches(&mut projection.exclusive, &others);
    }
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

/// 关掉某个功能的兼容开关：任何一家要关，契约就关（值取第一个要关的那家的）。上游不支持的
/// 功能开着会让那家每个请求都报错，关掉对别家只是少一个功能。
const CLAUDE_STACK_ANY_MEMBER_SWITCHES: &[&str] = &[
    "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS",
    "CLAUDE_CODE_DISABLE_ARTIFACT",
    "CLAUDE_CODE_AUTO_MODE_SERVER",
];

/// 打开某个功能的兼容开关：每一家都开了才开，有一家没开就去掉。
const CLAUDE_STACK_ALL_MEMBER_SWITCHES: &[&str] = &[
    "ENABLE_TOOL_SEARCH",
    "CLAUDE_CODE_ALWAYS_ENABLE_EFFORT",
    "CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING",
];

/// 环境变量当开关读：`1` / `true` / `yes` / `on` 为开，`0` / `false` / `no` / `off` / 空为关。
fn env_switch_on(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(on) => Some(*on),
        Value::Number(number) => number.as_f64().map(|n| n != 0.0),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" | "" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// 这一家要的是关掉功能的那一边：`CLAUDE_CODE_AUTO_MODE_SERVER` 设成关，其余开关设成开。
fn asks_to_turn_off(key: &str, value: &Value) -> bool {
    let on = env_switch_on(value);
    if key == "CLAUDE_CODE_AUTO_MODE_SERVER" {
        on == Some(false)
    } else {
        on == Some(true)
    }
}

/// Stack 模式下其余成员的兼容开关。Claude Code 只有一份 `settings.json`，这些开关在客户端发
/// 请求时生效、代理补不上，只写默认那家的话，选中别家的模型就带着那家不支持的功能出去。按
/// 开关的性质合并（[`CLAUDE_STACK_ANY_MEMBER_SWITCHES`]、[`CLAUDE_STACK_ALL_MEMBER_SWITCHES`]）；
/// `CLAUDE_CODE_EXTRA_BODY`、关思考这类和窗口类的仍只跟默认那家（窗口由 Stack 模型决定）。
fn with_member_switches(
    exclusive: &mut serde_json::Map<String, Value>,
    others: &[serde_json::Map<String, Value>],
) {
    if others.is_empty() {
        return;
    }
    for key in CLAUDE_STACK_ANY_MEMBER_SWITCHES {
        if exclusive
            .get(*key)
            .is_some_and(|value| asks_to_turn_off(key, value))
        {
            continue;
        }
        if let Some(value) = others
            .iter()
            .filter_map(|member| member.get(*key))
            .find(|value| asks_to_turn_off(key, value))
        {
            exclusive.insert(key.to_string(), value.clone());
        }
    }
    for key in CLAUDE_STACK_ALL_MEMBER_SWITCHES {
        let everyone_on = others
            .iter()
            .all(|member| member.get(*key).and_then(env_switch_on).unwrap_or(false));
        if !everyone_on {
            exclusive.shift_remove(*key);
        }
    }
}

/// 这个应用已落定的 Stack 名单。
fn settled_stack(app: &AppType) -> Result<StackState, String> {
    super::state::stack(&DeviceStore::for_device(), app.as_str()).map_err(err)
}

/// 日志里的模式名。
fn mode_label(proxy: bool, stack: bool) -> &'static str {
    match (proxy, stack) {
        (false, _) => "直连",
        (true, false) => "路由",
        (true, true) => "聚合",
    }
}

/// 日志里写没写客户端文件。
fn rewrite_label(rewritten: bool) -> &'static str {
    if rewritten {
        "已重写"
    } else {
        "没变，未重写"
    }
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

/// 写代理契约。`target` 的 `contract` 由这里填好。契约没变时不碰客户端文件（返回假）；接上
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
) -> Result<bool, String> {
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
    let unchanged = match app {
        AppType::Claude => {
            let members = published_members(state, app, &stack, route)?;
            let published = stack::claude_published(&members);
            let stack_default = stack::claude_route_default(&members);
            let others: Vec<&Provider> = members
                .iter()
                .filter(|member| !member.route)
                .map(|member| &member.provider)
                .collect();
            let (projection, contract) = claude_contract(
                route,
                &proxy_url,
                &published,
                stack_default.as_ref(),
                &others,
            );
            let unchanged = !force && live_now.has_contract(&contract.key);
            let mut patch = direct_patch(live_now.claude_exclusive_owner().as_ref(), &projection);
            crate::services::provider::claude_official::strip_from_patch(&mut patch);
            target.contract = Some(contract);
            claude_direct::run(
                &state.db,
                op_name,
                (!unchanged).then_some(&patch),
                pending(target),
            )
            .map_err(err)?;
            unchanged
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
            unchanged
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
            unchanged
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
                None,
                None,
            )
            .map_err(err)?;
            unchanged
        }
        AppType::ClaudeDesktop => {
            let members = published_members(state, app, &stack, route)?;
            let profile = claude_desktop_config::route_profile(
                &state.db,
                route,
                &proxy_url,
                &stack::desktop_published(&members),
            )
            .map_err(err)?;
            let contract = contract::claude_desktop(&profile);
            let unchanged = !force && live_now.has_contract(&contract.key);
            target.contract = Some(contract);
            claude_desktop_config::run(
                &state.db,
                op_name,
                (!unchanged).then_some(DesktopTarget::Profile(&profile)),
                pending(target),
            )
            .map_err(err)?;
            unchanged
        }
        _ => return Err(format!("{} 不支持本地路由", app.as_str())),
    };
    Ok(!unchanged)
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
            let mut patch = direct_patch(
                live_now.claude_exclusive_owner().as_ref(),
                projection.as_ref().unwrap_or(&empty),
            );
            crate::services::provider::claude_official::strip_from_patch(&mut patch);
            // 退出路由（不是退出 CC Switch 时的分离）回到直连的官方卡：和切换一样把它绑的
            // 账号换进 Claude Code 的登录（路由期间可能改过这张卡绑的号）
            let login_target = usable_direct(app, direct.as_ref()).filter(|_| op_name == op::EXIT);
            crate::services::provider::claude_official::with_login_switch(login_target, || {
                claude_direct::run(
                    &state.db,
                    op_name,
                    attached.then_some(&patch),
                    pending_target,
                )
            })
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
            // 退出路由（不是退出 CC Switch 时的分离）回到直连的官方卡：和切换一样换好 Grok CLI
            // 的登录（路由期间可能改过直连卡或它绑的号）。登录换不了不能让客户端一直指着
            // 代理：只写配置，记日志。
            let login = usable_direct(app, direct.as_ref())
                .filter(|_| attached && op_name == op::EXIT)
                .and_then(|provider| {
                    grok_direct::prepare_login(&state.xai_oauth_manager, provider).unwrap_or_else(
                        |error| {
                            log::warn!("退出路由时换 Grok CLI 登录失败，只写回配置: {error}");
                            None
                        },
                    )
                });
            grok_direct::run(
                &state.db,
                op_name,
                None,
                projection.as_ref(),
                pending_target,
                None,
                login.as_ref(),
            )
            .map_err(err)?;
        }
        AppType::ClaudeDesktop => {
            // 直连那家写不进去（模型映射卡、配置不全）也不能让 Desktop 一直指着路由：退回官方。
            let profile = if attached {
                claude_desktop_config::direct_target(direct.as_ref()).unwrap_or_else(|error| {
                    log::warn!("写回直连的 Claude Desktop 配置失败，改为官方: {error}");
                    None
                })
            } else {
                None
            };
            let write = attached.then_some(match &profile {
                Some(profile) => DesktopTarget::Profile(profile),
                None => DesktopTarget::Official,
            });
            claude_desktop_config::run(&state.db, op_name, write, pending_target).map_err(err)?;
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
    if app.spec().route_mode {
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

/// 同步代码里用的 [`lock_settled`]。没有路由模式的应用没有模式可以被并发改掉，不拿锁。
pub(crate) fn lock_settled_blocking(
    state: &AppState,
    app: &AppType,
) -> Result<Option<OwnedMutexGuard<()>>, AppError> {
    if !app.spec().route_mode {
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
    let result = try_enter_with_route(state, app, stack_mode, route).await;
    if let Err(error) = &result {
        log::error!(
            "[MODE] {} 进入{}模式失败（路由目标 {}）: {}",
            app.as_str(),
            mode_label(true, stack_mode),
            route.unwrap_or("沿用上次"),
            crate::error_for_log(error)
        );
    }
    result
}

async fn try_enter_with_route(
    state: &AppState,
    app: &AppType,
    stack_mode: bool,
    route: Option<&str>,
) -> Result<(), String> {
    require_proxy_app(app)?;
    if stack_mode && !app.spec().stack {
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
    // 只给日志用，读不到不拦着进入。
    let was_stack = settled_stack(app).is_ok_and(|stack| stack.enabled);
    let stack_on = stack_mode.unwrap_or(was_stack);
    let live_now = LiveNow::of(state, app, &mode)?;
    let rewritten = write_proxy(
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
    log::info!(
        "[MODE] {} {op_name}：{} → {}，路由 {}，客户端配置{}",
        app.as_str(),
        mode_label(mode.is_proxy(), was_stack),
        mode_label(true, stack_on),
        route.id,
        rewrite_label(rewritten)
    );
    state.proxy_service.set_active_target(app, &route).await;
    warn_if_official_route(state, app, &route).await;
    spawn_loopback_check(state, app, op_name == op::ENTER);
    Ok(())
}

/// Codex、Grok Build 跟随系统代理发请求，发往本地路由的请求可能被系统代理接走、客户端只看到
/// 空正文 502（见 [`loopback_probe`]）。后台自检一次，不拦进入：日志里留一笔；`notify` 时
/// （用户手动进入）再提示。启动时接上不提示，那时界面多半还没在听事件。
fn spawn_loopback_check(state: &AppState, app: &AppType, notify: bool) {
    if !matches!(app, AppType::Codex | AppType::GrokBuild) {
        return;
    }
    let service = state.proxy_service.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok((origin, _)) = service.build_proxy_urls().await else {
            return;
        };
        // 客户端地址改成别的机器时，CC Switch 这边走一遍代表不了客户端。
        if !crate::proxy::http_client::is_loopback_url(&origin) {
            return;
        }
        let Some(reason) = loopback_probe::hijack_reason(&origin).await else {
            return;
        };
        log::warn!(
            "[MODE] {} 的本地路由 {origin} 按系统代理访问回不到 CC Switch（{reason}），\
             客户端的请求可能被系统代理接走",
            app.as_str()
        );
        if notify {
            service
                .emit(
                    "proxy-loopback-hijacked",
                    json!({ "appType": app.as_str(), "origin": origin }),
                )
                .await;
        }
    });
}

async fn warn_if_official_route(state: &AppState, app: &AppType, route: &Provider) {
    if route.has_official_category()
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
    if let Err(error) = state.proxy_service.clear_provider_health_for_app(app).await {
        log::warn!("清除 {} 健康状态失败: {error}", app.as_str());
    }
    stop_server_if_unused(state).await;
    Ok(())
}

/// 设置里在路由和 Stack 之间换：处于另一种模式（`stack` 为真是 Stack 模式）的 Claude Code、
/// Codex 退回直连，名单留着。返回退回直连的应用。
pub async fn exit_apps_in_mode(state: &AppState, stack: bool) -> Result<Vec<String>, String> {
    let mut exited = Vec::new();
    for app in route_mode_apps().filter(|app| app.spec().stack) {
        {
            let _guard = lock_settled(state, &app).await.map_err(|e| e.to_string())?;
            if !current::is_proxy(&app) || settled_stack(&app)?.enabled != stack {
                continue;
            }
            exit_locked(state, &app, false)?;
        }
        if let Err(error) = state
            .proxy_service
            .clear_provider_health_for_app(&app)
            .await
        {
            log::warn!("清除 {} 健康状态失败: {error}", app.as_str());
        }
        exited.push(app.as_str().to_string());
    }
    // 没有应用退出时服务不是这次用的：可能是用户手动开的，或者别的应用在用，别停。
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
        log::debug!("[MODE] {} 已经是直连，不用退出", app.as_str());
        return Ok(());
    }
    let op_name = if keep_mode { op::DETACH } else { op::EXIT };
    let was_stack = settled_stack(app).is_ok_and(|stack| stack.enabled);
    let result = write_direct_for_exit(state, app, op_name, keep_mode, &mode);
    match &result {
        Ok(()) => log::info!(
            "[MODE] {} {op_name}：{} → {}，客户端配置写回直连供应商 {}",
            app.as_str(),
            mode_label(mode.is_proxy(), was_stack),
            if keep_mode {
                "客户端指回直连，模式保留"
            } else {
                "直连"
            },
            direct_provider(state, app)
                .ok()
                .flatten()
                .map_or_else(|| "（无）".to_string(), |provider| provider.id)
        ),
        Err(error) => log::error!(
            "[MODE] {} {op_name} 失败: {}",
            app.as_str(),
            crate::error_for_log(error)
        ),
    }
    result
}

fn write_direct_for_exit(
    state: &AppState,
    app: &AppType,
    op_name: &str,
    keep_mode: bool,
    mode: &ModeState,
) -> Result<(), String> {
    let live_now = LiveNow::of(state, app, mode)?;
    write_direct(
        state,
        app,
        op_name,
        &live_now,
        ModeState {
            mode: Some(if keep_mode && mode.is_proxy() {
                Mode::Proxy
            } else {
                Mode::Direct
            }),
            attached: false,
            proxy_route: mode.proxy_route.clone(),
            contract: None,
        },
    )
}

/// 没有应用在代理模式（含 Claude Desktop 的路由模式）时，停掉代理服务。
async fn stop_server_if_unused(state: &AppState) {
    if current::any_proxy() {
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
    for app in route_mode_apps() {
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
///
/// Claude Desktop 不分离：它只在启动时读配置，运行中的 Desktop 改了文件也照样连着路由；
/// 分离反而会在 CC Switch 关着时把 Desktop 换成直连那家（映射卡写不进去时是官方登录），
/// 下次打开 CC Switch 又换回来，每次都要重启 Desktop。
pub async fn detach_all(state: &AppState) {
    for app in route_mode_apps().filter(|app| *app != AppType::ClaudeDesktop) {
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

/// What this operation actually applied, computed while holding the per-app switch lock.
/// A client-config rewrite is separate from the proxy's immediately effective upstream target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSwitchResult {
    pub kind: RouteSwitchKind,
    pub client_config_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteSwitchKind {
    Remembered,
    Route,
    Stack,
}

/// 代理模式下手动换路由。调用方持有切换锁。直连指针不变。
pub async fn switch_route_locked(
    state: &AppState,
    app: &AppType,
    target: &Provider,
) -> Result<RouteSwitchResult, String> {
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
    let rewritten = if !mode.attached {
        commit_state(
            state,
            app,
            &PendingTarget {
                state: Some(new_state),
                stack: next_stack,
                ..PendingTarget::default()
            },
        )?;
        false
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
        .await?
    };
    // 编辑路由那家之后也走这里重算契约：路由没换、文件也没动的不记。
    if rewritten || !mode.routes_to(&target.id) {
        log::info!(
            "[MODE] {} {}：{}模式路由 {} → {}，客户端配置{}",
            app.as_str(),
            op::ROUTE,
            mode_label(true, current.enabled),
            mode.proxy_route.as_deref().unwrap_or("（无）"),
            target.id,
            rewrite_label(rewritten)
        );
    }
    state.proxy_service.set_active_target(app, target).await;
    Ok(RouteSwitchResult {
        kind: if !mode.attached {
            RouteSwitchKind::Remembered
        } else if current.enabled {
            RouteSwitchKind::Stack
        } else {
            RouteSwitchKind::Route
        },
        client_config_changed: rewritten,
    })
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
    switch_route_locked(state, app, &target).await.map(|_| ())
}

/// 指定代理路由（聚合模式下是默认那家），不管现在是什么模式。直连模式下只记下指针，下次进入
/// 路由 / 聚合模式时用它，客户端文件和模式都不动；已经在代理模式时就是换路由，当场生效。
pub async fn set_route(
    state: &AppState,
    app: &AppType,
    provider_id: &str,
) -> Result<RouteSwitchResult, String> {
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
        return Ok(RouteSwitchResult {
            kind: RouteSwitchKind::Remembered,
            client_config_changed: false,
        });
    }
    let previous = mode.proxy_route.clone();
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
    )?;
    log::info!(
        "[MODE] {} 直连模式下记下路由 {} → {}，下次进入路由 / 聚合模式时用",
        app.as_str(),
        previous.as_deref().unwrap_or("（无）"),
        provider_id
    );
    Ok(RouteSwitchResult {
        kind: RouteSwitchKind::Remembered,
        client_config_changed: false,
    })
}

/// 代理模式下不能切到不支持代理的官方供应商（Codex 官方账号走客户端自己的登录，除外）。
pub fn reject_unsupported_official(app: &AppType, provider: &Provider) -> Result<(), String> {
    if provider.has_official_category()
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
        _ => provider.has_official_category(),
    };
    if official {
        return Err(
            "官方账号不能加入聚合 (An official account cannot join the aggregation)".to_string(),
        );
    }
    match app {
        AppType::Codex => codex_direct::check_stack_member(provider).map_err(err)?,
        // 映射表不完整的卡发布不出模型，做默认时写入也会失败
        AppType::ClaudeDesktop => {
            claude_desktop_config::validate_proxy_provider(provider).map_err(err)?;
            claude_desktop_config::configured_proxy_model_routes(provider).map_err(err)?;
        }
        _ => {}
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
    if !app.spec().stack {
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
    publish_stack(state, app, next).await
}

/// 落定新的 Stack 名单：代理模式且已接上时和契约一起提交，否则只落定名单。
async fn publish_stack(state: &AppState, app: &AppType, next: StackState) -> Result<(), String> {
    match attached_route(state, app)? {
        Some((mode, route)) => {
            let live_now = LiveNow::of(state, app, &mode)?;
            write_proxy(state, app, op::STACK, &route, &live_now, mode, Some(next))
                .await
                .map(|_| ())
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

/// 拖动排序后让 Stack 名单跟界面顺序一致：名单顺序就是客户端看到的聚合模型顺序（Codex 的
/// 模型目录等），界面「已添加」却按 `sort_index` 排。只重排已有成员，不增删；顺序没变就什么
/// 都不写。不在库里的成员（理论上不会有）排到最后，保持原相对顺序。
pub async fn sync_stack_order(state: &AppState, app: &AppType) -> Result<(), String> {
    if !app.spec().stack {
        return Ok(());
    }
    let _guard = lock_settled(state, app).await.map_err(err)?;
    let current = settled_stack(app)?;
    if current.members.len() < 2 {
        return Ok(());
    }
    // 只读排序；读不出来的成员和不在库里的一样排到最后。
    let providers = state
        .db
        .get_provider_rows(app)
        .map_err(err)?
        .into_readable("Stack 排序");
    let mut next = current.clone();
    // 稳定排序：sort_index 相同或缺失时保留原名单顺序。
    next.members.sort_by_key(|id| {
        providers
            .get(id)
            .and_then(|provider| provider.sort_index)
            .map_or((1, 0), |index| (0, index))
    });
    if next == current {
        return Ok(());
    }
    publish_stack(state, app, next).await
}

/// Stack 模式的状态、名单里的每一家和它发布的模型 id（给前端）。
pub fn stack_views(state: &AppState, app: &AppType) -> Result<StackView, String> {
    if !app.spec().stack {
        return Ok(StackView::default());
    }
    let stack = settled_stack(app)?;
    let mode = current::mode_state(app);
    let route = mode.proxy_route.as_deref().filter(|_| mode.is_proxy());
    let members = stack::members(&state.db, app, &stack, route).map_err(err)?;
    let (notice, notice_reasons) = match app {
        AppType::Codex => codex_stack_notice_with_reasons(state, &stack)
            .map_or((None, Vec::new()), |(notice, reasons)| {
                (Some(notice), reasons)
            }),
        _ => (None, Vec::new()),
    };
    // 名单里每一家都列上（含路由那家）：切换到聚合的确认框里还能换默认那家，由前端按默认
    // 那家去掉它的模型（默认那家不发布 `ccs-*` 模型，选了也写不进去）。
    let subagent_models = match app {
        AppType::Codex => codex_direct::stack_subagent_choices(&members),
        _ => Vec::new(),
    };
    Ok(StackView {
        active: mode.is_proxy() && stack.enabled,
        members: stack::member_views(app, &members),
        notice,
        notice_reasons,
        stale_clients: None,
        stale_revision: None,
        subagent_models,
    })
}

/// [`stack_views`] 再加上 Codex 客户端是否可能缓存旧登录或模型列表（要读进程表，放到阻塞线程
/// 池里）。账号与目录检测都不分模式：切换账号、直连↔路由切换也会让已启动的客户端拿着旧登录
/// 或旧目录，同样要重启。路由那家自己管理目录时，重启也看不到 CC Switch 的目录，已经有
/// `notice` 说明，不再查目录。没有目录时（直连、路由那家没配目录）照样查：之前在 CC Switch
/// 的目录时启动的客户端还拿着它。
pub async fn stack_view_with_clients(state: &AppState, app: &AppType) -> Result<StackView, String> {
    let mut view = stack_views(state, app)?;
    if matches!(app, AppType::Codex) {
        let check_catalog = view.notice != Some("routeOwnsCatalog");
        (view.stale_clients, view.stale_revision) = codex_direct::off_runtime(move || {
            let stale =
                codex_client_catalog::stale_clients(&DeviceStore::for_device(), check_catalog);
            let revision = stale.map(|_| codex_client_catalog::revision());
            (stale, revision)
        })
        .await
        .map_err(err)?;
    }
    Ok(view)
}

/// Codex 在 Stack 模式下有要发布的 Stack 模型，客户端却看不到或看不全：路由那家自己管理模型
/// 目录文件（Stack 模型不发布）；或者官方做默认、最近一次写目录时没拿到官方列表，或者拿到的
/// 列表里没有能选的模型（本机 Codex 太旧）。
fn codex_stack_notice(state: &AppState, stack: &StackState) -> Option<&'static str> {
    codex_stack_notice_with_reasons(state, stack).map(|(notice, _)| notice)
}

/// [`codex_stack_notice`] 再加上没取到列表的原因码（见 [`StackView::notice_reasons`]）。
fn codex_stack_notice_with_reasons(
    state: &AppState,
    stack: &StackState,
) -> Option<(&'static str, Vec<&'static str>)> {
    use codex_official_models::NativeSource;
    let (_, route) = attached_route(state, &AppType::Codex).ok()??;
    let published =
        stack::published_members(&state.db, &AppType::Codex, stack, Some(&route.id)).ok()?;
    if published.is_empty() {
        return None;
    }
    if codex_direct::route_owns_catalog(&route) {
        return Some(("routeOwnsCatalog", Vec::new()));
    }
    if !codex_direct::is_official(&route) {
        return None;
    }
    match codex_official_models::last_source()? {
        NativeSource::Fetched => None,
        NativeSource::Bundled(official) => Some(("officialModelsBundled", vec![official.code()])),
        NativeSource::Unavailable { official, bundled } => Some((
            "officialModelsUnavailable",
            vec![official.code(), bundled.code()],
        )),
        NativeSource::Outdated => Some(("officialModelsOutdated", Vec::new())),
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
            .update_provider_settings_config(&app, &route.id, &settings)
            .map_err(err)?;
        route.settings_config = settings;
    }
    let live_now = LiveNow::of(state, &app, &mode)?;
    write_proxy(state, &app, op::CATALOG, &route, &live_now, mode, None).await?;
    Ok(settled_stack(&app)
        .ok()
        .and_then(|stack| codex_stack_notice(state, &stack)))
}

/// 聚合子 agent 设置（「经典子 agent」开关、子 agent 模型和推理强度）变了：Codex 在 Stack
/// 模式下按新的合并目录和 `[agents]` 重写客户端（契约没变就什么都不做）。不在 Stack 模式时
/// 这些设置不起作用，等进 Stack 时自然按设置写。
pub async fn resync_codex_stack_catalog(state: &AppState) -> Result<(), String> {
    let app = AppType::Codex;
    let _guard = lock_settled(state, &app).await.map_err(err)?;
    if !settled_stack(&app)?.enabled {
        return Ok(());
    }
    let Some((mode, route)) = attached_route(state, &app)? else {
        return Ok(());
    };
    let live_now = LiveNow::of(state, &app, &mode)?;
    write_proxy(state, &app, op::APPLY, &route, &live_now, mode, None)
        .await
        .map(|_| ())
}

/// 「暂停使用官方登录」变了：路由 / 聚合模式下按新契约重写 Codex（契约没变就什么都不做）。
/// 直连或没接上代理时这个开关不起作用，等进代理时自然按设置写。
pub async fn resync_codex_proxy(state: &AppState) -> Result<(), String> {
    let app = AppType::Codex;
    let _guard = lock_settled(state, &app).await.map_err(err)?;
    let Some((mode, route)) = attached_route(state, &app)? else {
        return Ok(());
    };
    let live_now = LiveNow::of(state, &app, &mode)?;
    write_proxy(state, &app, op::APPLY, &route, &live_now, mode, None)
        .await
        .map(|_| ())
}

/// 路由供应商的行或代理地址变了：按新契约重写客户端（契约没变就什么都不做）。调用方
/// 持有切换锁。
pub async fn resync_route_locked(state: &AppState, app: &AppType) -> Result<(), String> {
    let Some((_, route)) = attached_route(state, app)? else {
        return Ok(());
    };
    switch_route_locked(state, app, &route).await.map(|_| ())
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
        return switch_route_locked(state, app, provider).await.map(|_| ());
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
            codex_official_models::catch_up_after_fallback(&login);
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
    codex_direct::predicted_official_login(&state.db, &owner, &spec, &prepared)
        .map(Result::ok)
        .map_err(err)
}

/// 代理换了地址之后，按新地址重写每个接上代理的应用。一个应用失败（比如配置文件解析
/// 不了）不影响其余应用：旧地址已经没人监听，跳过的应用会一直连不上。失败的汇总报错。
pub async fn resync_routes(state: &AppState) -> Result<(), String> {
    let mut failures = Vec::new();
    for app in route_mode_apps() {
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
    for app in route_mode_apps() {
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
    // 记一次新启动的 Codex 会读到的目录：兜住启动时补完的操作和 CC Switch 没开时的外部修改。
    codex_client_catalog::observe(&DeviceStore::for_device());
}

async fn startup_app(state: &AppState, app: &AppType) -> Result<(), String> {
    let had_backup = drain_legacy_backup(state, app).await;
    let mut mode = current::mode_state(app);
    // 新版自己接上时写的占位符不算遗留物（比如重启更新时没来得及分离）。
    let placeholder = !mode.attached && state.proxy_service.live_has_proxy_placeholder(app);
    let legacy = had_backup || placeholder;
    let enabled = if *app == AppType::ClaudeDesktop {
        // 旧版按卡片决定 Desktop 走不走路由：直连指针是模型映射卡就是在用路由。只在第一次
        // （模式还没有值）时看。
        let card = mode
            .mode
            .is_none()
            .then(|| claude_desktop_config::legacy_mapping_card(&state.db))
            .flatten();
        if let Some(card) = &card {
            adopt_legacy_desktop_card(state, &mut mode, card)?;
        }
        card.is_some()
    } else {
        state.db.get_proxy_flags_sync(app).0
    };
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
                let stack = app.spec().stack
                    && settled_stack(app)
                        .map(|stack| stack.enabled)
                        .unwrap_or(false);
                startup_attach_failures()
                    .lock_recover()
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

/// 旧版的 Desktop 映射卡拆成新版的三样：旧版给映射卡写的一律是本地路由地址，所以客户端
/// 已经接上（`attached`，接不上退回直连时才会把它写回去）；路由那家是这张卡；直连指针改成
/// 官方卡（映射卡直连写不进去，退出路由时 Desktop 本来就会回到官方，指针要跟着一致）。
/// 指针只改本机记录：`provider_for` 先读它，库里的 `is_current` 会随云同步到别的设备。
/// 先落定模式再改指针：中途崩了，下次启动按已落定的路由模式接上，不会因为指针已经不是
/// 映射卡而认不出旧版状态。
fn adopt_legacy_desktop_card(
    state: &AppState,
    mode: &mut ModeState,
    card: &Provider,
) -> Result<(), String> {
    let app = AppType::ClaudeDesktop;
    mode.mode = Some(Mode::Proxy);
    mode.attached = true;
    mode.proxy_route = Some(card.id.clone());
    commit_state(state, &app, &PendingTarget::mode(mode.clone()))?;
    state
        .db
        .ensure_official_seed_by_id(
            crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
            app.clone(),
        )
        .map_err(err)?;
    crate::settings::set_current_provider(
        &app,
        Some(crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID),
    )
    .map_err(err)?;
    log::info!(
        "Claude Desktop 旧版映射卡 {} 迁移为路由模式：路由到这张卡，直连指针改为官方",
        card.id
    );
    Ok(())
}

/// 旧版的接管备份行：不回放，转存到本机文件后删除。留着的话，降级后旧版启动时会把这份
/// 陈旧的快照写回客户端。
async fn drain_legacy_backup(state: &AppState, app: &AppType) -> bool {
    let backup = match state.db.get_live_backup(app).await {
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
            if let Err(error) = state.db.delete_live_backup(app).await {
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
    } else if app.spec().stack && settled_stack(app)?.enabled {
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
    startup_attach_failures().lock_recover().clone()
}

/// 取走启动时记下的接上失败（取一次就清空）。
pub fn take_startup_attach_failures() -> Vec<StartupAttachFailure> {
    std::mem::take(&mut *startup_attach_failures().lock_recover())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod mode_tests;
