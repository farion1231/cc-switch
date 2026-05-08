//! 故障转移队列命令
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）
//! 以及智能路由配置与队列管理

use crate::database::FailoverQueueItem;
use crate::provider::Provider;
use crate::store::AppState;
use std::str::FromStr;
use tauri::Emitter;

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

/// 获取指定应用的自动故障转移开关状态（从 proxy_config 表读取）
#[tauri::command]
pub async fn get_auto_failover_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, String> {
    state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map(|config| config.auto_failover_enabled)
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

    // 强一致语义：开启故障转移后立即切到队列 P1（并确保队列非空）
    //
    // 说明：
    // - 仅在 enabled=true 时执行“切到 P1”
    // - 若队列为空，则尝试把“当前供应商”自动加入队列作为 P1，避免用户在 UI 上陷入死锁（无法先加队列再开启）
    let p1_provider_id = if enabled {
        let mut queue = state
            .db
            .get_failover_queue(&app_type)
            .map_err(|e| e.to_string())?;

        if queue.is_empty() {
            let app_enum = crate::app_config::AppType::from_str(&app_type)
                .map_err(|_| format!("无效的应用类型: {app_type}"))?;

            let current_id = crate::settings::get_effective_current_provider(&state.db, &app_enum)
                .map_err(|e| e.to_string())?;

            let Some(current_id) = current_id else {
                return Err("故障转移队列为空，且未设置当前供应商，无法开启故障转移".to_string());
            };

            state
                .db
                .add_to_failover_queue(&app_type, &current_id)
                .map_err(|e| e.to_string())?;

            queue = state
                .db
                .get_failover_queue(&app_type)
                .map_err(|e| e.to_string())?;
        }

        queue
            .first()
            .map(|item| item.provider_id.clone())
            .ok_or_else(|| "故障转移队列为空，无法开启故障转移".to_string())?
    } else {
        String::new()
    };

    // 读取当前配置
    let mut config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    // 更新 auto_failover_enabled 字段
    config.auto_failover_enabled = enabled;

    // 写回数据库
    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    // 开启后立即切到 P1：更新 is_current + 本地 settings + Live 备份（接管模式下）
    if enabled {
        state
            .proxy_service
            .switch_proxy_target(&app_type, &p1_provider_id)
            .await?;

        // 发射 provider-switched 事件（让前端刷新当前供应商）
        let event_data = serde_json::json!({
            "appType": app_type,
            "providerId": p1_provider_id,
            "source": "failoverEnabled"
        });
        let _ = app.emit("provider-switched", event_data);
    }

    // 刷新托盘菜单，确保状态同步
    if let Ok(new_menu) = crate::tray::create_tray_menu(&app, &state) {
        if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }

    Ok(())
}

// ==================== 智能路由命令 ====================

/// 获取智能路由开关状态
#[tauri::command]
pub async fn get_smart_routing_enabled(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<bool, String> {
    state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map(|config| config.smart_routing_enabled)
        .map_err(|e| e.to_string())
}

/// 设置智能路由开关状态
#[tauri::command]
pub async fn set_smart_routing_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[SmartRouting] Setting smart_routing_enabled: app_type='{app_type}', enabled={enabled}"
    );

    let mut config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    config.smart_routing_enabled = enabled;

    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    // 关闭智能路由时，清除内存中的 others_provider 状态，避免 UI 显示 stale 数据
    if !enabled {
        state.proxy_service.clear_smart_routing_state(&app_type).await;
    }

    // 刷新托盘菜单
    if let Ok(new_menu) = crate::tray::create_tray_menu(&app, &state) {
        if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }

    Ok(())
}

/// 获取智能路由队列（main 或 others）
#[tauri::command]
pub async fn get_smart_routing_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    queue_type: String,
) -> Result<Vec<FailoverQueueItem>, String> {
    let config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    let provider_ids = match queue_type.as_str() {
        "main" => config.main_request_queue,
        "others" => config.others_request_queue,
        _ => return Err(format!("Invalid queue_type: {queue_type}, must be 'main' or 'others'")),
    };

    let all_providers = state
        .db
        .get_all_providers(&app_type)
        .map_err(|e| e.to_string())?;

    let items: Vec<FailoverQueueItem> = provider_ids
        .iter()
        .enumerate()
        .filter_map(|(idx, pid)| {
            all_providers.get(pid).map(|p| FailoverQueueItem {
                provider_id: p.id.clone(),
                provider_name: p.name.clone(),
                sort_index: Some(idx),
                provider_notes: p.notes.clone(),
            })
        })
        .collect();

    Ok(items)
}

/// 添加供应商到智能路由队列
#[tauri::command]
pub async fn add_to_smart_routing_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
    queue_type: String,
) -> Result<(), String> {
    let mut config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    let other_queue_name;
    let provider_in_other_queue;

    {
        other_queue_name = if queue_type.as_str() == "main" { "others" } else { "main" };
        provider_in_other_queue = if queue_type.as_str() == "main" {
            config.others_request_queue.contains(&provider_id)
        } else {
            config.main_request_queue.contains(&provider_id)
        };
    }

    if provider_in_other_queue {
        return Err(format!(
            "Provider {provider_id} already in the other queue ({}), remove it first",
            other_queue_name
        ));
    }

    let queue = match queue_type.as_str() {
        "main" => &mut config.main_request_queue,
        "others" => &mut config.others_request_queue,
        _ => return Err(format!("Invalid queue_type: {queue_type}")),
    };

    if queue.contains(&provider_id) {
        return Err(format!("Provider {provider_id} already in {queue_type} queue"));
    }

    queue.push(provider_id);

    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 从智能路由队列移除供应商
#[tauri::command]
pub async fn remove_from_smart_routing_queue(
    state: tauri::State<'_, AppState>,
    app_type: String,
    provider_id: String,
    queue_type: String,
) -> Result<(), String> {
    let mut config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    let queue = match queue_type.as_str() {
        "main" => &mut config.main_request_queue,
        "others" => &mut config.others_request_queue,
        _ => return Err(format!("Invalid queue_type: {queue_type}")),
    };

    queue.retain(|id| id != &provider_id);

    state
        .db
        .update_proxy_config_for_app(config)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// 获取可添加到智能路由队列的供应商
///
/// 返回既不在 main_request_queue 也不在 others_request_queue 中的供应商
/// 注意：供应商可以同时存在于故障转移队列和智能路由队列中（故障转移队列是回退路径）
#[tauri::command]
pub async fn get_available_providers_for_smart_routing(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<Provider>, String> {
    let config = state
        .db
        .get_proxy_config_for_app(&app_type)
        .await
        .map_err(|e| e.to_string())?;

    let all_providers = state
        .db
        .get_all_providers(&app_type)
        .map_err(|e| e.to_string())?;

    let used_ids: std::collections::HashSet<String> = config
        .main_request_queue
        .iter()
        .chain(config.others_request_queue.iter())
        .cloned()
        .collect();

    let available: Vec<Provider> = all_providers
        .into_values()
        .filter(|p| !used_ids.contains(&p.id))
        .collect();

    Ok(available)
}
