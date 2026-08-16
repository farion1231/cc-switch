//! Session 路由命令
//!
//! 管理 session 级 provider 路由的配置和状态查询

use crate::proxy::session_router::{SessionRouteInfo, SessionRoutingConfig};
use crate::store::AppState;

/// 获取 Session 路由配置
#[tauri::command]
pub async fn get_session_routing_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<SessionRoutingConfig, String> {
    state
        .db
        .get_session_routing_config(&app_type)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未找到配置".to_string())
}

/// 更新 Session 路由配置
#[tauri::command]
pub async fn update_session_routing_config(
    state: tauri::State<'_, AppState>,
    app_type: String,
    config: SessionRoutingConfig,
) -> Result<(), String> {
    log::info!(
        "[SessionRoutes] 更新配置: app={} enabled={} strategy={} ttl={}",
        app_type,
        config.enabled,
        config.strategy.as_str(),
        config.session_ttl_seconds,
    );
    state
        .db
        .update_session_routing_config(&app_type, &config)
        .map_err(|e| e.to_string())
}

/// 获取所有活跃 session 路由
#[tauri::command]
pub async fn get_active_session_routes(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<SessionRouteInfo>, String> {
    state
        .db
        .get_all_session_routes(&app_type)
        .map_err(|e| e.to_string())
}

/// 删除指定 session 路由
#[tauri::command]
pub async fn delete_session_route(
    state: tauri::State<'_, AppState>,
    session_id: String,
    app_type: String,
) -> Result<(), String> {
    state
        .db
        .delete_session_route(&session_id, &app_type)
        .map_err(|e| e.to_string())
}

/// 手动指定 session 使用某个 provider（覆盖自动分配）
#[tauri::command]
pub async fn set_session_route_provider(
    state: tauri::State<'_, AppState>,
    session_id: String,
    app_type: String,
    provider_id: String,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    log::info!(
        "[SessionRoutes] 手动指定: session={} app={} → provider={}",
        &session_id[..8.min(session_id.len())],
        app_type,
        provider_id,
    );
    state
        .db
        .update_session_route_provider(&session_id, &app_type, &provider_id, now)
        .map_err(|e| e.to_string())
}

/// 清理过期 session 路由
#[tauri::command]
pub async fn cleanup_expired_session_routes(
    state: tauri::State<'_, AppState>,
    _app_type: String,
    ttl_seconds: u64,
) -> Result<u64, String> {
    let cutoff = chrono::Utc::now().timestamp_millis() - (ttl_seconds as i64 * 1000);
    state
        .db
        .delete_expired_session_routes(cutoff)
        .map_err(|e| e.to_string())
}

/// 获取每个 provider 的 session 负载统计
#[tauri::command]
pub async fn get_session_provider_load(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<std::collections::HashMap<String, u64>, String> {
    state
        .db
        .count_sessions_per_provider(&app_type)
        .map_err(|e| e.to_string())
}