//! Hermes 按 profile 独立路由的 Tauri 命令。
//!
//! 每个 Hermes agent（`~/.hermes/profiles/<name>/`）可以单独进入代理模式：
//! `/hermes/<name>/v1` 独立命名空间，自己的路由目标、故障转移与负载均衡。

use crate::services::provider::hermes_profile::{self, ProfileStatus};
use crate::store::AppState;

/// 全部 Hermes agent 的独立配置状态（含没开独立的）。
#[tauri::command]
pub async fn list_hermes_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProfileStatus>, String> {
    hermes_profile::list(&state).await
}

/// 独立进入代理模式：`{profile}` 的 home 指向 `/hermes/{profile}/v1`，路由到
/// `routeProviderId` 那家；`autoFailover` / `loadBalanceMode`（"off" | "session"）
/// 落这个 agent 自己的代理配置。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn enter_hermes_profile_proxy(
    state: tauri::State<'_, AppState>,
    profile: String,
    route_provider_id: String,
    auto_failover: Option<bool>,
    load_balance_mode: Option<String>,
) -> Result<(), String> {
    hermes_profile::enter(
        &state,
        &profile,
        &route_provider_id,
        auto_failover.unwrap_or(false),
        load_balance_mode.as_deref().unwrap_or("off"),
    )
    .await
}

/// 独立退出代理模式：home 恢复原值。
#[tauri::command]
pub async fn exit_hermes_profile_proxy(
    state: tauri::State<'_, AppState>,
    profile: String,
) -> Result<(), String> {
    hermes_profile::exit(&state, &profile).await
}

/// 独立换路由目标。
#[tauri::command]
pub async fn switch_hermes_profile_route(
    state: tauri::State<'_, AppState>,
    profile: String,
    route_provider_id: String,
) -> Result<bool, String> {
    hermes_profile::switch_route(&state, &profile, &route_provider_id).await
}
