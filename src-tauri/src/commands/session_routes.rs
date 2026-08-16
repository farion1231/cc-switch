//! Session 路由命令
//!
//! 管理 session 级 provider 路由的配置和状态查询

use crate::proxy::session_router::{ProviderLoadInfo, SessionRouteInfo, SessionRoutingConfig};
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

/// 获取所有活跃 session 路由（含 Claude Code 终端名称）
#[tauri::command]
pub async fn get_active_session_routes(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<SessionRouteInfo>, String> {
    let mut routes = state
        .db
        .get_all_session_routes(&app_type)
        .map_err(|e| e.to_string())?;

    // 从 Claude Code 本地会话文件读取终端名称 (sessionId → name)
    let name_map = load_claude_session_names();

    for route in &mut routes {
        route.session_name = name_map
            .get(&route.session_id)
            .cloned()
            .unwrap_or_default();
    }

    Ok(routes)
}

/// 读取 ~/.claude/sessions/*.json，构建 sessionId → name 映射
///
/// Claude Code 为每个终端在本地保存一个会话文件，其中含 sessionId 和
/// 终端名（如 "multiple-claude-54"）。
fn load_claude_session_names() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let sessions_dir = crate::config::get_home_dir().join(".claude").join("sessions");
    let Ok(entries) = std::fs::read_dir(&sessions_dir) else {
        return map;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let session_id = json
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if !session_id.is_empty() && !name.is_empty() {
            map.insert(session_id, name);
        }
    }
    map
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

/// 获取每个 provider 的 session 负载统计（含名称）
///
/// 只显示故障转移队列中的 provider（可分配 session 的候选），
/// 外加当前有 session 但已移出队列的 provider（仍承载负载）。
#[tauri::command]
pub async fn get_session_provider_load(
    state: tauri::State<'_, AppState>,
    app_type: String,
) -> Result<Vec<ProviderLoadInfo>, String> {
    let counts = state
        .db
        .count_sessions_per_provider(&app_type)
        .map_err(|e| e.to_string())?;
    let queue_providers = state
        .db
        .get_failover_providers(&app_type)
        .map_err(|e| e.to_string())?;

    let mut result: Vec<ProviderLoadInfo> = queue_providers
        .iter()
        .map(|p| ProviderLoadInfo {
            provider_id: p.id.clone(),
            provider_name: p.name.clone(),
            session_count: counts.get(&p.id).copied().unwrap_or(0),
        })
        .collect();

    // 有 session 但不在队列里的 provider 也列出（名称标记为队列外）
    for (id, count) in counts.iter() {
        if *count > 0 && !queue_providers.iter().any(|p| p.id == *id) {
            let name = state
                .db
                .get_provider_by_id(id, &app_type)
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_else(|| "(deleted)".to_string());
            result.push(ProviderLoadInfo {
                provider_id: id.clone(),
                provider_name: format!("{name} (未入队)"),
                session_count: *count,
            });
        }
    }

    // 按 session 数降序
    result.sort_by_key(|b| std::cmp::Reverse(b.session_count));
    Ok(result)
}