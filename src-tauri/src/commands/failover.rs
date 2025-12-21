//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列

use crate::database::FailoverQueueItem;
use crate::provider::Provider;
use crate::store::AppState;

/// 获取故障转移队列
#[tauri::command]
pub async fn get_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<FailoverQueueItem>, String> {
    state
        .db
        .get_failover_queue(&app_type)
        .map_err(|e| e.to_string())
}

/// 获取可添加到故障转移队列的供应商（不在队列中的）
#[tauri::command]
pub async fn get_available_providers_for_failover(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    state
        .db
        .get_available_providers_for_failover(&app_type)
        .map_err(|e| e.to_string())
}

/// 添加供应商到故障转移队列
#[tauri::command]
pub async fn add_to_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    state
        .db
        .add_to_failover_queue(&app_type, &provider_id)
        .map_err(|e| e.to_string())
}

/// 从故障转移队列移除供应商
#[tauri::command]
pub async fn remove_from_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    state
        .db
        .remove_from_failover_queue(&app_type, &provider_id)
        .map_err(|e| e.to_string())
}

/// 重新排序故障转移队列
#[tauri::command]
pub async fn reorder_failover_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_ids: Vec<String>,
) -> Result<(), String> {
    state
        .db
        .reorder_failover_queue(&app_type, &provider_ids)
        .map_err(|e| e.to_string())
}

/// 设置故障转移队列项的启用状态
#[tauri::command]
pub async fn set_failover_item_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .set_failover_item_enabled(&app_type, &provider_id, enabled)
        .map_err(|e| e.to_string())
}

/// 获取自动故障转移总开关状态
#[tauri::command]
pub async fn get_auto_failover_enabled(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state
        .db
        .get_setting("auto_failover_enabled")
        .map(|v| v.map(|s| s == "true").unwrap_or(false)) // 默认关闭
        .map_err(|e| e.to_string())
}

/// 设置自动故障转移总开关状态
#[tauri::command]
pub async fn set_auto_failover_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .set_setting(
            "auto_failover_enabled",
            if enabled { "true" } else { "false" },
        )
        .map_err(|e| e.to_string())
}
