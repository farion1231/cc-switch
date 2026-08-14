//! Fallback Chain 命令
//!
//! 管理 oh-my-pi Fallback Chain 架构的配置与运行时状态：
//! - 回退链路定义（fallback_chain_config 表）
//! - fallback 配置开关（proxy_config 列）
//! - Selector 抑制状态（selector_suppression 表）

use crate::database::{FallbackChainEntry, FallbackProxyConfig, SelectorSuppressionRow};
use crate::store::AppState;
use std::collections::HashMap;

/// 获取指定 app 的全部回退链路（chain_key → 有序条目）
#[tauri::command]
pub async fn get_fallback_chains(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<HashMap<String, Vec<FallbackChainEntry>>, String> {
    state
        .db
        .get_all_fallback_chains(&app_type)
        .map_err(|e| e.to_string())
}

/// 保存一条回退链路（整链覆盖写入）
#[tauri::command]
pub async fn save_fallback_chain(
    state: tauri::State<'_, AppState>,
    app_type: String,
    chain_key: String,
    selectors: Vec<FallbackChainEntry>,
) -> Result<(), String> {
    state
        .db
        .save_fallback_chain(&app_type, &chain_key, &selectors)
        .map_err(|e| e.to_string())
}

/// 删除一条回退链路
#[tauri::command]
pub async fn delete_fallback_chain(
    state: tauri::State<'_, AppState>,
    app_type: String,
    chain_key: String,
) -> Result<(), String> {
    state
        .db
        .delete_fallback_chain(&app_type, &chain_key)
        .map_err(|e| e.to_string())
}

/// 获取 fallback 配置（proxy_config 列）
#[tauri::command]
pub async fn get_fallback_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<FallbackProxyConfig, String> {
    state
        .db
        .get_fallback_proxy_config(&app_type)
        .map_err(|e| e.to_string())
}

/// 设置 fallback 配置。
///
/// 启用 fallback 时同步开启 auto_failover_enabled（两者共用故障转移队列做候选），
/// 避免"fallback 开了但 select_providers 只返回单 provider"的失效配置。
#[tauri::command]
pub async fn set_fallback_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    app_type: String,
    config: FallbackProxyConfig,
) -> Result<(), String> {
    log::info!(
        "[Fallback] Setting fallback config: app_type='{app_type}', enabled={}",
        config.fallback_enabled
    );

    if config.fallback_enabled {
        let proxy = state
            .db
            .get_proxy_config_for_app(&app_type)
            .await
            .map_err(|e| e.to_string())?;

        if !proxy.enabled {
            return Err("需要先启用该应用的代理接管，再开启 Fallback Chain".to_string());
        }

        if !proxy.auto_failover_enabled {
            let mut updated = proxy;
            updated.auto_failover_enabled = true;
            state
                .db
                .update_proxy_config_for_app(updated)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    state
        .db
        .set_fallback_proxy_config(&app_type, &config)
        .map_err(|e| e.to_string())?;

    // 刷新托盘菜单（启用/禁用可能影响状态展示）
    if let Ok(new_menu) = crate::tray::create_tray_menu(&app, &state) {
        if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }

    Ok(())
}

/// 获取当前 Selector 抑制状态
#[tauri::command]
pub async fn get_selector_suppressions(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<SelectorSuppressionRow>, String> {
    state
        .db
        .get_selector_suppressions(&app_type)
        .map_err(|e| e.to_string())
}

/// 手动清除 Selector 抑制（用于 UI 上的"立即恢复"）
#[tauri::command]
pub async fn clear_selector_suppression(
    state: tauri::State<'_, AppState>,
    app_type: String,
    selector_identity: String,
) -> Result<(), String> {
    state
        .db
        .clear_selector_suppression(&app_type, &selector_identity)
        .map_err(|e| e.to_string())
}
