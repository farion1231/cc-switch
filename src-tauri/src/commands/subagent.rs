//! Claude Code 用户级子代理（`~/.claude/agents/*.md`）命令层
//!
//! 冲突类错误以 `SUBAGENT_*:` 前缀返回，前端据此弹出“强制执行”确认。

use tauri::State;

use super::run_db_task;
use crate::services::subagent::{
    PluginSubagent, Subagent, SubagentDriftStrategy, SubagentInput, SubagentService,
    SubagentsDirInfo, UnmanagedSubagent,
};
use crate::store::AppState;

#[tauri::command]
pub async fn list_subagents(state: State<'_, AppState>) -> Result<Vec<Subagent>, String> {
    run_db_task(&state, |state| {
        SubagentService::list(&state.db, &SubagentService::agents_dir()).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_subagents_dir() -> Result<SubagentsDirInfo, String> {
    Ok(SubagentService::dir_info(&SubagentService::agents_dir()))
}

#[tauri::command]
pub async fn upsert_subagent(
    input: SubagentInput,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Subagent, String> {
    run_db_task(&state, move |state| {
        SubagentService::upsert(
            &state.db,
            &SubagentService::agents_dir(),
            input,
            force.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_subagent(
    id: String,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    run_db_task(&state, move |state| {
        SubagentService::delete(
            &state.db,
            &SubagentService::agents_dir(),
            &id,
            force.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn toggle_subagent(
    id: String,
    enabled: bool,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Subagent, String> {
    run_db_task(&state, move |state| {
        SubagentService::toggle(
            &state.db,
            &SubagentService::agents_dir(),
            &id,
            enabled,
            force.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn scan_unmanaged_subagents(
    state: State<'_, AppState>,
) -> Result<Vec<UnmanagedSubagent>, String> {
    run_db_task(&state, |state| {
        SubagentService::scan_unmanaged(&state.db, &SubagentService::agents_dir())
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn import_subagents(
    file_names: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<Subagent>, String> {
    run_db_task(&state, move |state| {
        SubagentService::import(&state.db, &SubagentService::agents_dir(), file_names)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn resolve_subagent_drift(
    id: String,
    strategy: SubagentDriftStrategy,
    state: State<'_, AppState>,
) -> Result<Subagent, String> {
    run_db_task(&state, move |state| {
        SubagentService::resolve_drift(&state.db, &SubagentService::agents_dir(), &id, strategy)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_subagent_disk_content(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    run_db_task(&state, move |state| {
        SubagentService::disk_content(&state.db, &SubagentService::agents_dir(), &id)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_plugin_subagents() -> Result<Vec<PluginSubagent>, String> {
    // 只读文件系统，不碰数据库；放到阻塞线程池避免插件目录很大时卡住 UI
    tauri::async_runtime::spawn_blocking(|| {
        SubagentService::list_plugin_subagents(&SubagentService::claude_root_dir())
    })
    .await
    .map_err(|e| format!("读取插件子代理失败: {e}"))
}
