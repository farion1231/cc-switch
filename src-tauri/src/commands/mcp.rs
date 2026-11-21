#![allow(non_snake_case)]

use crate::error::AppError;
use indexmap::IndexMap;
use std::str::FromStr;

use tauri::State;

use super::run_db_task;
use crate::app_config::AppType;
use crate::services::McpService;
use crate::store::AppState;

/// Start a bounded, one-shot batch only while the MCP page is mounted.
#[tauri::command]
pub async fn start_mcp_connectivity_checks(
    state: State<'_, AppState>,
    server_ids: Vec<String>,
    on_result: tauri::ipc::Channel<crate::services::mcp_connectivity::ConnectivityResult>,
) -> Result<String, String> {
    let servers = run_db_task(&state, McpService::get_all_servers).await?;
    Ok(crate::services::mcp_connectivity::start_checks(
        // Use the page's enabled selection: app toggles update it optimistically,
        // so the database's enablement flags can still lag behind the UI.
        servers
            .into_iter()
            .filter(|(id, _)| server_ids.contains(id))
            .map(|(_, server)| server)
            .collect(),
        move |result| on_result.send(result).is_ok(),
    ))
}

#[tauri::command]
pub fn cancel_mcp_connectivity_checks(batch_id: String) {
    crate::services::mcp_connectivity::cancel_checks(&batch_id);
}

// ============================================================================
// v3.7.0 新增：统一 MCP 管理命令
// ============================================================================

use crate::app_config::McpServer;

/// 获取所有 MCP 服务器（统一结构）
#[tauri::command]
pub async fn get_mcp_servers(
    state: State<'_, AppState>,
) -> Result<IndexMap<String, McpServer>, AppError> {
    McpService::get_all_servers(&state)
}

/// 库里读不出来的 MCP 服务器（只给界面展示、删除）。
#[tauri::command]
pub async fn get_unreadable_mcp_servers(
    state: State<'_, AppState>,
) -> Result<Vec<crate::database::UnreadableEntry>, AppError> {
    McpService::list_unreadable(&state)
}

#[tauri::command]
pub async fn upsert_mcp_server(
    state: State<'_, AppState>,
    server: McpServer,
) -> Result<(), AppError> {
    McpService::upsert_server(&state, server)
}

#[tauri::command]
pub async fn delete_mcp_server(state: State<'_, AppState>, id: String) -> Result<bool, AppError> {
    McpService::delete_server(&state, &id)
}

/// 切换 MCP 服务器在指定应用的启用状态
#[tauri::command]
pub async fn toggle_mcp_app(
    state: State<'_, AppState>,
    server_id: String,
    app: String,
    enabled: bool,
) -> Result<(), AppError> {
    let app_ty = AppType::from_str(&app)?;
    McpService::toggle_app_locked(&state, &server_id, app_ty, enabled).await
}

/// 从所有应用导入 MCP 服务器（复用已有的导入逻辑）
#[tauri::command]
pub async fn import_mcp_from_apps(state: State<'_, AppState>) -> Result<usize, AppError> {
    McpService::import_from_all_apps(&state)
}

/// 按数据库里的开关把 MCP 重新写进各应用的 live 配置，逐应用返回结果。
///
/// `apps` 缺省或为空时同步全部受管应用。每个应用先拿它的切换锁再写，
/// 和切换供应商互斥；单个应用失败（如配置文件解析失败）不影响其余应用。
#[tauri::command]
pub async fn resync_mcp_to_apps(
    state: State<'_, AppState>,
    apps: Option<Vec<String>>,
) -> Result<Vec<crate::services::mcp::McpAppSyncOutcome>, String> {
    let targets = McpService::resync_targets(apps.as_deref())?;
    let mut outcomes = Vec::with_capacity(targets.len());
    for app in targets {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        outcomes.push(McpService::resync_app(&state, &app));
    }
    Ok(outcomes)
}
