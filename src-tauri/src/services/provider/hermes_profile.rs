//! Hermes 按 profile 独立路由：`/hermes/<profile>/v1` 独立命名空间。
//!
//! 每个 profile agent 可以脱离统一路由（`hermes_direct`），单独进入代理模式：自己的
//! 路由目标、故障转移开关、会话粘性/轮询负载均衡。伪应用键 `hermes:<profile>`：
//!
//! - live-state 的模式状态、写入记录、契约按这个键存（`mode::state` 的键就是字符串）；
//! - `proxy_config` 行按这个键存（v24 起 CHECK 放行 `hermes:%`）；
//! - 供应商池仍是 Hermes 应用的全部供应商（按 sort_index 排序），路由目标从
//!   live-state 的 `proxy_route` 读——独立配置只区分「路由谁、开不开故障转移和
//!   粘性」，不再单独维护队列。
//!
//! 独立 profile 的 home 由这里写（单 home），统一路由（`hermes_direct::run_enter`）
//! 跳过已独立的 profile。聚合模式（Stack）只在统一路由提供。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{DeviceStore, LiveFile};
use crate::mode::state::{self, PendingTarget};
use crate::provider::Provider;
use crate::store::AppState;

use super::hermes_direct;

pub(crate) const APP_PREFIX: &str = "hermes:";

/// 伪应用键 `hermes:<profile>`。
pub fn app_key(profile: &str) -> String {
    format!("{APP_PREFIX}{profile}")
}

/// 从伪应用键取出 profile 名；不是伪键时为 `None`。
pub fn profile_of(app_key: &str) -> Option<&str> {
    app_key.strip_prefix(APP_PREFIX)
}

/// 这个键是 Hermes profile 伪应用键。
pub fn is_profile_key(app_key: &str) -> bool {
    app_key.starts_with(APP_PREFIX)
}

/// profile 目录名的合法形状：Hermes 自己的 profile 名是路径段，这里从严——
/// 字母数字、`-`、`_`，1..=64 个字符；`v1` 保留给静态路由。
pub fn validate_profile_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name != "v1"
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "无效的 Hermes agent 名: {name:?}（字母数字、-、_，1..=64 个字符）"
        ))
    }
}

/// 这个 profile 的 home（配置文件必须存在——独立配置只对真实 agent 生效）。
fn home_of(profile: &str) -> Result<(String, LiveFile), String> {
    validate_profile_name(profile)?;
    let home = hermes_direct::homes()
        .into_iter()
        .find(|home| home.name == profile);
    home.map(|home| (home.name, home.file))
        .ok_or_else(|| format!("Hermes agent 不存在: {profile}"))
}

/// 伪应用键当前的路由目标行（不在代理模式时为 `None`）。
pub(crate) fn route_provider(db: &Database, profile: &str) -> Result<Option<Provider>, AppError> {
    let key = app_key(profile);
    let mode = state::mode_state(&DeviceStore::for_device(), &key)?;
    let Some(route_id) = (mode.is_proxy()).then_some(mode.proxy_route).flatten() else {
        return Ok(None);
    };
    db.get_provider_by_id(&route_id, AppType::Hermes.as_str())
}

/// 独立进入代理模式：`{proxy}/hermes/<profile>/v1` 上的 Responses 路由条目写进这个
/// home，`model:` 段指过去（原值记录在写入记录里，退出恢复）。`route_id` 是路由那家；
/// `auto_failover` / `load_balance_mode` 落这个伪键的 `proxy_config` 行。
pub async fn enter(
    state: &AppState,
    profile: &str,
    route_id: &str,
    auto_failover: bool,
    load_balance_mode: &str,
) -> Result<(), String> {
    let key = app_key(profile);
    let (home_name, file) = home_of(profile)?;
    let route = state
        .db
        .get_provider_by_id(route_id, AppType::Hermes.as_str())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("供应商不存在: {route_id}"))?;
    if route.category.as_deref() == Some("official") {
        return Err(
            "官方账号不能经本地路由使用 (Official accounts cannot use local routing)".to_string(),
        );
    }
    if !state.proxy_service.is_running().await {
        state.proxy_service.start().await?;
    }
    let proxy_url = state.proxy_service.build_proxy_urls().await?.0;
    let base_url = format!("{}/hermes/{profile}/v1", proxy_url.trim_end_matches('/'));

    // 伪键的代理配置行：默认值 + 调用方给的开关。
    {
        let mut config = state
            .db
            .get_proxy_config_for_app(&key)
            .await
            .map_err(|e| e.to_string())?;
        config.auto_failover_enabled = auto_failover;
        if matches!(load_balance_mode, "off" | "session") {
            config.load_balance_mode = load_balance_mode.to_string();
        }
        state
            .db
            .update_proxy_config_for_app(config)
            .await
            .map_err(|e| e.to_string())?;
    }

    let guard = state.proxy_service.lock_switch_for_app(&key).await;
    super::hermes_profile::settle(&state.db, &key)?;
    let projection = hermes_direct::proxy_contract(&route, &base_url).map_err(|e| e.to_string())?;
    let contract = crate::mode::contract::hermes(
        &projection,
        std::slice::from_ref(&home_name),
    );
    hermes_direct::run_enter_homes(
        &state.db,
        "enter",
        &[(home_name, file)],
        &projection,
        &key,
        PendingTarget {
            state: Some(crate::mode::state::ModeState {
                mode: Some(crate::mode::state::Mode::Proxy),
                attached: true,
                proxy_route: Some(route.id.clone()),
                contract: Some(contract),
            }),
            ..PendingTarget::default()
        },
    )
    .map_err(|e| e.to_string())?;
    drop(guard);
    state
        .proxy_service
        .set_active_target(&AppType::Hermes, &route)
        .await;
    Ok(())
}

/// 独立退出代理模式：按写入记录恢复这个 home 的 `model:` 段，删路由条目。
pub async fn exit(state: &AppState, profile: &str) -> Result<(), String> {
    let key = app_key(profile);
    let (home_name, file) = home_of(profile)?;
    let guard = state.proxy_service.lock_switch_for_app(&key).await;
    settle(&state.db, &key)?;
    hermes_direct::run_exit_homes(
        &state.db,
        "exit",
        &[(home_name, file)],
        &key,
        None,
        PendingTarget {
            state: Some(crate::mode::state::ModeState {
                mode: Some(crate::mode::state::Mode::Direct),
                attached: false,
                proxy_route: None,
                contract: None,
            }),
            ..PendingTarget::default()
        },
    )
    .map_err(|e| e.to_string())?;
    drop(guard);
    Ok(())
}

/// 独立换路由：契约没变就不碰文件。
pub async fn switch_route(state: &AppState, profile: &str, route_id: &str) -> Result<bool, String> {
    let key = app_key(profile);
    let (home_name, file) = home_of(profile)?;
    let route = state
        .db
        .get_provider_by_id(route_id, AppType::Hermes.as_str())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("供应商不存在: {route_id}"))?;
    if route.category.as_deref() == Some("official") {
        return Err(
            "官方账号不能经本地路由使用 (Official accounts cannot use local routing)".to_string(),
        );
    }
    let proxy_url = state.proxy_service.build_proxy_urls().await?.0;
    let base_url = format!("{}/hermes/{profile}/v1", proxy_url.trim_end_matches('/'));

    let guard = state.proxy_service.lock_switch_for_app(&key).await;
    settle(&state.db, &key)?;
    let mode = state::mode_state(&DeviceStore::for_device(), &key).map_err(|e| e.to_string())?;
    if !mode.is_proxy() {
        return Err(format!("{profile} 不在独立代理模式"));
    }
    let projection = hermes_direct::proxy_contract(&route, &base_url).map_err(|e| e.to_string())?;
    let contract = crate::mode::contract::hermes(
        &projection,
        std::slice::from_ref(&home_name),
    );
    let unchanged = mode
        .contract
        .as_ref()
        .is_some_and(|current| current.key == contract.key);
    hermes_direct::run_enter_homes(
        &state.db,
        "route",
        &[(home_name, file)],
        &projection,
        &key,
        PendingTarget {
            state: Some(crate::mode::state::ModeState {
                mode: Some(crate::mode::state::Mode::Proxy),
                attached: true,
                proxy_route: Some(route.id.clone()),
                contract: Some(contract),
            }),
            ..PendingTarget::default()
        },
    )
    .map_err(|e| e.to_string())?;
    drop(guard);
    state
        .proxy_service
        .set_active_target(&AppType::Hermes, &route)
        .await;
    Ok(!unchanged)
}

/// 补完伪键上一次没做完的写操作（拿锁后调用）。
pub(crate) fn settle(db: &Database, key: &str) -> Result<(), String> {
    crate::mode::operation::settle(db, key)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 一个 profile 的独立配置状态（给命令层列表用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    pub name: String,
    /// 独立代理模式开着。
    pub independent: bool,
    /// 客户端文件当前接在独立命名空间上。
    pub attached: bool,
    pub route_provider_id: Option<String>,
    pub route_provider_name: Option<String>,
    pub auto_failover_enabled: bool,
    pub load_balance_mode: String,
}

/// 全部 profile 的独立配置状态（含没开独立的）。
pub async fn list(state: &AppState) -> Result<Vec<ProfileStatus>, String> {
    let store = DeviceStore::for_device();
    let mut out = Vec::new();
    for home in hermes_direct::homes() {
        if home.name.is_empty() {
            continue;
        }
        let key = app_key(&home.name);
        let mode = state::mode_state(&store, &key).map_err(|e| e.to_string())?;
        let independent = mode.is_proxy();
        let route = match mode.proxy_route.as_deref() {
            Some(id) => state
                .db
                .get_provider_by_id(id, AppType::Hermes.as_str())
                .map_err(|e| e.to_string())?,
            None => None,
        };
        let config = state
            .db
            .get_proxy_config_for_app(&key)
            .await
            .map_err(|e| e.to_string())?;
        out.push(ProfileStatus {
            name: home.name.clone(),
            independent,
            attached: mode.attached,
            route_provider_id: mode.proxy_route.clone(),
            route_provider_name: route.as_ref().map(|route| route.name.clone()),
            auto_failover_enabled: config.auto_failover_enabled,
            load_balance_mode: config.load_balance_mode,
        });
    }
    Ok(out)
}

/// 退出 CC Switch 时分离全部独立 profile（客户端指回原值，模式保留）。
pub async fn detach_all(state: &AppState) {
    let store = DeviceStore::for_device();
    let Ok(state_all) = state::load(&store) else {
        return;
    };
    for key in state_all.apps.keys() {
        let Some(profile) = profile_of(key) else {
            continue;
        };
        let Ok(mode) = state::mode_state(&store, key) else {
            continue;
        };
        if !mode.is_proxy() || !mode.attached {
            continue;
        }
        let Ok((home_name, file)) = home_of(profile) else {
            continue;
        };
        let _ = hermes_direct::run_exit_homes(
            &state.db,
            "detach",
            &[(home_name, file)],
            key,
            None,
            PendingTarget {
                state: Some(crate::mode::state::ModeState {
                    mode: Some(crate::mode::state::Mode::Proxy),
                    attached: false,
                    proxy_route: mode.proxy_route.clone(),
                    contract: mode.contract.clone(),
                }),
                ..PendingTarget::default()
            },
        );
    }
}

/// 启动时把已独立的 profile 重新接上（按保存的路由重写）。
pub async fn attach_all(state: &AppState) {
    let store = DeviceStore::for_device();
    let Ok(state_all) = state::load(&store) else {
        return;
    };
    for key in state_all.apps.keys().cloned().collect::<Vec<_>>() {
        let Some(profile) = profile_of(&key) else {
            continue;
        };
        let Ok(mode) = state::mode_state(&store, &key) else {
            continue;
        };
        if !mode.is_proxy() || !mode.attached {
            continue;
        }
        let Some(route_id) = mode.proxy_route.clone() else {
            continue;
        };
        // 重接上不改用户设置：故障转移与负载均衡沿用已存的配置行。
        let key = app_key(profile);
        let Ok(config) = state.db.get_proxy_config_for_app(&key).await else {
            continue;
        };
        let _ = enter(
            state,
            profile,
            &route_id,
            config.auto_failover_enabled,
            &config.load_balance_mode,
        )
        .await;
    }
}

/// 统一路由应跳过的 home：已独立（伪键在代理模式）的 profile。
pub(crate) fn is_independent(profile: &str) -> bool {
    state::mode_state(&DeviceStore::for_device(), &app_key(profile))
        .map(|mode| mode.is_proxy())
        .unwrap_or(false)
}
