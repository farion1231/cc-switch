//! 多环境写入目标的命令（见 `app_targets`）。

use std::collections::BTreeMap;

use crate::app_targets::model::AppTargetsConfig;
use crate::app_targets::service::{self, Resolution};
use crate::store::AppState;

/// 设置页概览：发行版、各应用的目标配置、解析出的目录和最近结果。不启动发行版，
/// `probe_stopped` 为真时（「重新检测」）除外。
#[tauri::command]
pub async fn get_app_targets_overview(
    probe_stopped: Option<bool>,
) -> Result<service::AppTargetsOverview, String> {
    let probe = probe_stopped.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || service::overview(probe))
        .await
        .map_err(|e| e.to_string())
}

/// 保存前检查：目录校验、新目标里已有的不同配置。只读（可能为读 HOME 启动已勾选的发行版）。
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

/// 在某个环境里启用供应商（供应商页的环境开关）。
#[tauri::command]
pub async fn activate_app_target_provider(
    state: tauri::State<'_, AppState>,
    app: String,
    target: String,
    provider_id: String,
) -> Result<crate::app_targets::sync::TargetReport, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::activate(&state, &app, &target, &provider_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
