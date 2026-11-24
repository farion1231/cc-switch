//! 多环境写入目标的命令（见 `app_targets`）。

use std::collections::BTreeMap;

use crate::app_targets::model::AppTargetsConfig;
use crate::app_targets::service::{self, Resolution};
use crate::store::AppState;

/// 设置页概览：发行版、各应用的目标配置、解析出的目录和最近结果。不启动发行版，
/// `probe_stopped` 仅刷新运行中发行版的文件信息，不执行 shell。
#[tauri::command]
pub async fn get_app_targets_overview(
    probe_stopped: Option<bool>,
    refresh_installs: Option<bool>,
) -> Result<service::AppTargetsOverview, String> {
    let probe = probe_stopped.unwrap_or(false);
    // 保留旧参数兼容 IPC；本阶段不探测 WSL 客户端安装情况。
    let refresh = refresh_installs.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || service::overview(probe, refresh))
        .await
        .map_err(|e| e.to_string())
}

/// 保存前检查：目录校验、新目标里已有的不同配置。只读运行中发行版的 UNC 文件，不启动发行版。
#[tauri::command]
pub async fn inspect_app_targets(
    state: tauri::State<'_, AppState>,
    app: String,
    config: AppTargetsConfig,
) -> Result<service::InspectResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service::inspect(&state, &app, config))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 按应用保存写入目标；`resolutions` 是每个冲突目标的处理方式。
#[tauri::command]
pub async fn save_app_targets(
    state: tauri::State<'_, AppState>,
    app: String,
    config: AppTargetsConfig,
    resolutions: Option<BTreeMap<String, Resolution>>,
) -> Result<service::SaveResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::save(&state, &app, config, resolutions.unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// 立即同步 / 重试。`targets` 为空时同步全部跟随环境。
#[tauri::command]
pub async fn sync_app_targets(
    state: tauri::State<'_, AppState>,
    app: String,
    targets: Option<Vec<String>>,
) -> Result<service::SyncResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service::sync_now(&state, &app, targets))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 各 WSL 发行版的路由客户端地址。`probe` 为真时重新检测网络。
#[tauri::command]
pub async fn get_wsl_route_info(
    state: tauri::State<'_, AppState>,
    probe: Option<bool>,
) -> Result<Vec<service::WslRouteInfo>, String> {
    let state = state.inner().clone();
    let probe = probe.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || service::wsl_route_info(&state, probe))
        .await
        .map_err(|e| e.to_string())
}

/// 保存某个 WSL 发行版的路由客户端主机（空字符串清除，回到自动）。
#[tauri::command]
pub async fn set_wsl_client_host(
    state: tauri::State<'_, AppState>,
    distro: String,
    host: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::set_wsl_client_host(&state, &distro, &host)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
