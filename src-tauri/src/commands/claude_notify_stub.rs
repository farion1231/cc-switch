#![allow(non_snake_case)]

use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeNotifyStatus {
    pub port: Option<u16>,
    pub listening: bool,
    pub hooks_applied: bool,
}

pub async fn sync_claude_notify_runtime_if_needed(
    _app: tauri::AppHandle,
    _state: &tauri::State<'_, crate::store::AppState>,
    _existing: &crate::settings::AppSettings,
    _merged: &crate::settings::AppSettings,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn apply_claude_notify_hook_config(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    Err(AppError::localized(
        "claude_notify.unsupported_platform",
        "当前平台不支持 Claude 后台通知 Hook 配置",
        "Claude background notification hook configuration is not supported on this platform",
    )
    .to_string())
}

#[tauri::command]
pub async fn clear_claude_notify_hook_config(
    _state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    Ok(false)
}

#[tauri::command]
pub async fn get_claude_notify_status(
    _state: tauri::State<'_, crate::store::AppState>,
) -> Result<ClaudeNotifyStatus, String> {
    Ok(ClaudeNotifyStatus {
        port: None,
        listening: false,
        hooks_applied: false,
    })
}
