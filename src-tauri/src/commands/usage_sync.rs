//! 跨设备用量同步（Usage Sync）命令

use crate::error::AppError;
use crate::services::usage_sync::{self, ConflictResolution, UsageSyncOverview, UsageSyncReport};
use crate::settings::{UsageSyncBackend, UsageSyncDetailLevel, UsageSyncSettings};
use crate::store::AppState;
use tauri::State;

/// 用量同步概览：设置、本机身份、远端设备列表与状态
#[tauri::command]
pub async fn usage_sync_get_overview(
    state: State<'_, AppState>,
) -> Result<UsageSyncOverview, AppError> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || usage_sync::get_overview(&db))
        .await
        .map_err(|error| AppError::Message(format!("用量同步查询失败: {error}")))?
}

/// 保存用量同步偏好（不含状态）
#[tauri::command]
pub fn usage_sync_save_settings(
    enabled: bool,
    auto_sync: bool,
    backend: Option<UsageSyncBackend>,
    detail_level: UsageSyncDetailLevel,
    interval_minutes: u32,
) -> Result<UsageSyncSettings, AppError> {
    usage_sync::save_preferences(enabled, auto_sync, backend, detail_level, interval_minutes)
}

/// 立即同步（上传本机 + 拉取其它设备）
#[tauri::command]
pub async fn usage_sync_run_now(state: State<'_, AppState>) -> Result<UsageSyncReport, AppError> {
    let db = state.db.clone();
    usage_sync::run_sync(&db, "manual").await
}

/// 设置本机在其它设备上显示的名称；空值恢复系统主机名
#[tauri::command]
pub fn usage_sync_set_device_name(
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<(), AppError> {
    usage_sync::set_device_name(&state.db, name)
}

/// 在本机统计中隐藏 / 恢复某个远端设备（不删除远端数据）
#[tauri::command]
pub fn usage_sync_set_device_hidden(
    state: State<'_, AppState>,
    device_id: String,
    hidden: bool,
) -> Result<(), AppError> {
    usage_sync::set_device_hidden(&state.db, &device_id, hidden)
}

/// 处理设备身份冲突（配置目录被复制到另一台机器）
#[tauri::command]
pub async fn usage_sync_resolve_conflict(
    state: State<'_, AppState>,
    resolution: ConflictResolution,
) -> Result<(), AppError> {
    let db = state.db.clone();
    usage_sync::resolve_conflict(&db, resolution).await
}
