//! Agent commands.
//!
//! Unified agents management structure, aligned with rules.

use crate::app_config::{AppType, InstalledAgent, UnmanagedAgent};
use crate::services::agent::{
    Agent, AgentBackupEntry, AgentRepo, AgentService, AgentUninstallResult, DiscoverableAgent,
    ImportAgentSelection,
};
use crate::store::AppState;
use std::sync::Arc;
use tauri::State;

/// AgentService state wrapper.
pub struct AgentServiceState(pub Arc<AgentService>);

fn parse_app_type(app: &str) -> Result<AppType, String> {
    match app.to_lowercase().as_str() {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        "gemini" => Ok(AppType::Gemini),
        "opencode" => Ok(AppType::OpenCode),
        _ => Err(format!("Unsupported app type: {app}")),
    }
}

fn resolve_agent_install_target(
    agents: Vec<DiscoverableAgent>,
    identifier: &str,
) -> Result<DiscoverableAgent, String> {
    if let Some(agent) = agents.iter().find(|a| {
        a.key.eq_ignore_ascii_case(identifier) || a.directory.eq_ignore_ascii_case(identifier)
    }) {
        return Ok(agent.clone());
    }

    let mut basename_matches: Vec<DiscoverableAgent> = agents
        .into_iter()
        .filter(|a| {
            std::path::Path::new(&a.directory)
                .file_name()
                .map(|n| n.to_string_lossy().eq_ignore_ascii_case(identifier))
                .unwrap_or(false)
        })
        .collect();

    match basename_matches.len() {
        1 => Ok(basename_matches.remove(0)),
        0 => Err(format!("Agent not found: {identifier}")),
        _ => Err(format!(
            "Agent basename '{identifier}' is ambiguous; use the full path or agent key instead"
        )),
    }
}

fn resolve_agent_uninstall_target(
    agents: Vec<InstalledAgent>,
    identifier: &str,
) -> Result<InstalledAgent, String> {
    if let Some(agent) = agents.iter().find(|a| {
        a.id.eq_ignore_ascii_case(identifier) || a.directory.eq_ignore_ascii_case(identifier)
    }) {
        return Ok(agent.clone());
    }

    let mut basename_matches: Vec<InstalledAgent> = agents
        .into_iter()
        .filter(|a| {
            std::path::Path::new(&a.directory)
                .file_name()
                .map(|n| n.to_string_lossy().eq_ignore_ascii_case(identifier))
                .unwrap_or(false)
        })
        .collect();

    match basename_matches.len() {
        1 => Ok(basename_matches.remove(0)),
        0 => Err(format!("Agent not found: {identifier}")),
        _ => Err(format!(
            "Agent basename '{identifier}' is ambiguous; use the full path or agent id instead"
        )),
    }
}

#[tauri::command]
pub fn get_installed_agents(app_state: State<'_, AppState>) -> Result<Vec<InstalledAgent>, String> {
    AgentService::get_all_installed(&app_state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent_backups() -> Result<Vec<AgentBackupEntry>, String> {
    AgentService::list_backups().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_agent_backup(backup_id: String) -> Result<bool, String> {
    AgentService::delete_backup(&backup_id).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn install_agent_unified(
    agent: DiscoverableAgent,
    current_app: String,
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledAgent, String> {
    let app_type = parse_app_type(&current_app)?;
    service
        .0
        .install(&app_state.db, &agent, &app_type)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall_agent_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<AgentUninstallResult, String> {
    AgentService::uninstall(&app_state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_agent_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledAgent, String> {
    let app_type = parse_app_type(&current_app)?;
    AgentService::restore_from_backup(&app_state.db, &backup_id, &app_type)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_agent_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    AgentService::toggle_app(&app_state.db, &id, &app_type, enabled)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn scan_unmanaged_agents(
    app_state: State<'_, AppState>,
) -> Result<Vec<UnmanagedAgent>, String> {
    AgentService::scan_unmanaged(&app_state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_agents_from_apps(
    imports: Vec<ImportAgentSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledAgent>, String> {
    AgentService::import_from_apps(&app_state.db, imports).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn discover_available_agents(
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableAgent>, String> {
    let repos = app_state.db.get_agent_repos().map_err(|e| e.to_string())?;
    service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agents(
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Agent>, String> {
    let repos = app_state.db.get_agent_repos().map_err(|e| e.to_string())?;
    service
        .0
        .list_agents(repos, &app_state.db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agents_for_app(
    app: String,
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Agent>, String> {
    let _ = parse_app_type(&app)?;
    get_agents(service, app_state).await
}

#[tauri::command]
pub async fn install_agent(
    directory: String,
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_agent_for_app("claude".to_string(), directory, service, app_state).await
}

#[tauri::command]
pub async fn install_agent_for_app(
    app: String,
    directory: String,
    service: State<'_, AgentServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;

    let repos = app_state.db.get_agent_repos().map_err(|e| e.to_string())?;
    let agents = service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())?;

    let agent = resolve_agent_install_target(agents, &directory)?;

    service
        .0
        .install(&app_state.db, &agent, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

#[tauri::command]
pub fn uninstall_agent(
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<AgentUninstallResult, String> {
    uninstall_agent_for_app("claude".to_string(), directory, app_state)
}

#[tauri::command]
pub fn uninstall_agent_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<AgentUninstallResult, String> {
    let _ = parse_app_type(&app)?;

    let agents = AgentService::get_all_installed(&app_state.db).map_err(|e| e.to_string())?;
    let agent = resolve_agent_uninstall_target(agents, &directory)?;

    AgentService::uninstall(&app_state.db, &agent.id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent_repos(app_state: State<'_, AppState>) -> Result<Vec<AgentRepo>, String> {
    app_state.db.get_agent_repos().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_agent_repo(repo: AgentRepo, app_state: State<'_, AppState>) -> Result<bool, String> {
    app_state
        .db
        .save_agent_repo(&repo)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn remove_agent_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .delete_agent_repo(&owner, &name)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn install_agents_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledAgent>, String> {
    let app_type = parse_app_type(&current_app)?;
    let path = std::path::Path::new(&file_path);
    AgentService::install_from_zip(&app_state.db, path, &app_type).map_err(|e| e.to_string())
}
