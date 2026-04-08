//! Rule commands.
//!
//! Unified rules management structure, aligned with skills.

use crate::app_config::{AppType, InstalledRule, UnmanagedRule};
use crate::services::rule::{
    DiscoverableRule, ImportRuleSelection, Rule, RuleBackupEntry, RuleRepo, RuleService,
    RuleUninstallResult,
};
use crate::store::AppState;
use std::sync::Arc;
use tauri::State;

/// RuleService state wrapper.
pub struct RuleServiceState(pub Arc<RuleService>);

fn parse_app_type(app: &str) -> Result<AppType, String> {
    match app.to_lowercase().as_str() {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        "gemini" => Ok(AppType::Gemini),
        "opencode" => Ok(AppType::OpenCode),
        _ => Err(format!("Unsupported app type: {app}")),
    }
}

fn resolve_rule_install_target(
    rules: Vec<DiscoverableRule>,
    identifier: &str,
) -> Result<DiscoverableRule, String> {
    if let Some(rule) = rules.iter().find(|r| {
        r.key.eq_ignore_ascii_case(identifier) || r.directory.eq_ignore_ascii_case(identifier)
    }) {
        return Ok(rule.clone());
    }

    let mut basename_matches: Vec<DiscoverableRule> = rules
        .into_iter()
        .filter(|r| {
            std::path::Path::new(&r.directory)
                .file_name()
                .map(|n| n.to_string_lossy().eq_ignore_ascii_case(identifier))
                .unwrap_or(false)
        })
        .collect();

    match basename_matches.len() {
        1 => Ok(basename_matches.remove(0)),
        0 => Err(format!("Rule not found: {identifier}")),
        _ => Err(format!(
            "Rule basename '{identifier}' is ambiguous; use the full path or rule key instead"
        )),
    }
}

fn resolve_rule_uninstall_target(
    rules: Vec<InstalledRule>,
    identifier: &str,
) -> Result<InstalledRule, String> {
    if let Some(rule) = rules.iter().find(|r| {
        r.id.eq_ignore_ascii_case(identifier) || r.directory.eq_ignore_ascii_case(identifier)
    }) {
        return Ok(rule.clone());
    }

    let mut basename_matches: Vec<InstalledRule> = rules
        .into_iter()
        .filter(|r| {
            std::path::Path::new(&r.directory)
                .file_name()
                .map(|n| n.to_string_lossy().eq_ignore_ascii_case(identifier))
                .unwrap_or(false)
        })
        .collect();

    match basename_matches.len() {
        1 => Ok(basename_matches.remove(0)),
        0 => Err(format!("Rule not found: {identifier}")),
        _ => Err(format!(
            "Rule basename '{identifier}' is ambiguous; use the full path or rule id instead"
        )),
    }
}

#[tauri::command]
pub fn get_installed_rules(app_state: State<'_, AppState>) -> Result<Vec<InstalledRule>, String> {
    RuleService::get_all_installed(&app_state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_rule_backups() -> Result<Vec<RuleBackupEntry>, String> {
    RuleService::list_backups().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_rule_backup(backup_id: String) -> Result<bool, String> {
    RuleService::delete_backup(&backup_id).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn install_rule_unified(
    rule: DiscoverableRule,
    current_app: String,
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<InstalledRule, String> {
    let app_type = parse_app_type(&current_app)?;
    service
        .0
        .install(&app_state.db, &rule, &app_type)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall_rule_unified(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<RuleUninstallResult, String> {
    RuleService::uninstall(&app_state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_rule_backup(
    backup_id: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<InstalledRule, String> {
    let app_type = parse_app_type(&current_app)?;
    RuleService::restore_from_backup(&app_state.db, &backup_id, &app_type)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_rule_app(
    id: String,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;
    RuleService::toggle_app(&app_state.db, &id, &app_type, enabled).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn scan_unmanaged_rules(app_state: State<'_, AppState>) -> Result<Vec<UnmanagedRule>, String> {
    RuleService::scan_unmanaged(&app_state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_rules_from_apps(
    imports: Vec<ImportRuleSelection>,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledRule>, String> {
    RuleService::import_from_apps(&app_state.db, imports).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn discover_available_rules(
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<DiscoverableRule>, String> {
    let repos = app_state.db.get_rule_repos().map_err(|e| e.to_string())?;
    service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_rules(
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Rule>, String> {
    let repos = app_state.db.get_rule_repos().map_err(|e| e.to_string())?;
    service
        .0
        .list_rules(repos, &app_state.db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_rules_for_app(
    app: String,
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<Vec<Rule>, String> {
    let _ = parse_app_type(&app)?;
    get_rules(service, app_state).await
}

#[tauri::command]
pub async fn install_rule(
    directory: String,
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    install_rule_for_app("claude".to_string(), directory, service, app_state).await
}

#[tauri::command]
pub async fn install_rule_for_app(
    app: String,
    directory: String,
    service: State<'_, RuleServiceState>,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    let app_type = parse_app_type(&app)?;

    let repos = app_state.db.get_rule_repos().map_err(|e| e.to_string())?;
    let rules = service
        .0
        .discover_available(repos)
        .await
        .map_err(|e| e.to_string())?;

    let rule = resolve_rule_install_target(rules, &directory)?;

    service
        .0
        .install(&app_state.db, &rule, &app_type)
        .await
        .map_err(|e| e.to_string())?;

    Ok(true)
}

#[tauri::command]
pub fn uninstall_rule(
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<RuleUninstallResult, String> {
    uninstall_rule_for_app("claude".to_string(), directory, app_state)
}

#[tauri::command]
pub fn uninstall_rule_for_app(
    app: String,
    directory: String,
    app_state: State<'_, AppState>,
) -> Result<RuleUninstallResult, String> {
    let _ = parse_app_type(&app)?;

    let rules = RuleService::get_all_installed(&app_state.db).map_err(|e| e.to_string())?;
    let rule = resolve_rule_uninstall_target(rules, &directory)?;

    RuleService::uninstall(&app_state.db, &rule.id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_rule_repos(app_state: State<'_, AppState>) -> Result<Vec<RuleRepo>, String> {
    app_state.db.get_rule_repos().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_rule_repo(repo: RuleRepo, app_state: State<'_, AppState>) -> Result<bool, String> {
    app_state
        .db
        .save_rule_repo(&repo)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn remove_rule_repo(
    owner: String,
    name: String,
    app_state: State<'_, AppState>,
) -> Result<bool, String> {
    app_state
        .db
        .delete_rule_repo(&owner, &name)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn install_rules_from_zip(
    file_path: String,
    current_app: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<InstalledRule>, String> {
    let app_type = parse_app_type(&current_app)?;
    let path = std::path::Path::new(&file_path);
    RuleService::install_from_zip(&app_state.db, path, &app_type).map_err(|e| e.to_string())
}
