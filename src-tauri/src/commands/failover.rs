//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）

use crate::bridges::proxy as proxy_bridge;
use crate::database::FailoverQueueItem;
use crate::provider::Provider;
use crate::store::AppState;
use tauri::Emitter;

/// 获取故障转移队列
#[tauri::command]
pub async fn get_failover_queue(
    _state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<FailoverQueueItem>, String> {
    proxy_bridge::get_failover_queue(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 获取可添加到故障转移队列的供应商（不在队列中的）
#[tauri::command]
pub async fn get_available_providers_for_failover(
    _state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    proxy_bridge::get_available_providers_for_failover(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 添加供应商到故障转移队列
#[tauri::command]
pub async fn add_to_failover_queue(
    _state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    proxy_bridge::add_to_failover_queue(&app_type, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// 从故障转移队列移除供应商
#[tauri::command]
pub async fn remove_from_failover_queue(
    _state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    proxy_bridge::remove_from_failover_queue(&app_type, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定应用的自动故障转移开关状态（从 proxy_config 表读取）
#[tauri::command]
pub async fn get_auto_failover_enabled(
    _state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, String> {
    proxy_bridge::get_auto_failover_enabled(&app_type)
        .await
        .map_err(|e| e.to_string())
}

/// 设置指定应用的自动故障转移开关状态（写入 proxy_config 表）
///
/// 注意：关闭故障转移时不会清除队列，队列内容会保留供下次开启时使用
#[tauri::command]
pub async fn set_auto_failover_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[Failover] Setting auto_failover_enabled: app_type='{app_type}', enabled={enabled}"
    );

    let switched_provider_id = proxy_bridge::set_auto_failover_enabled(&app_type, enabled)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(provider_id) = switched_provider_id {
        let event_data = serde_json::json!({
            "appType": app_type,
            "providerId": provider_id,
            "source": "failoverEnabled"
        });
        let _ = app.emit("provider-switched", event_data);
    }

    // 刷新托盘菜单，确保状态同步
    if let Ok(new_menu) = crate::tray::create_tray_menu(&app, &state) {
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_menu(Some(new_menu));
        }
    }

    Ok(())
}
