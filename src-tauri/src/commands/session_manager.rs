#![allow(non_snake_case)]

use crate::session_manager;
use crate::store::AppState;
use std::collections::HashMap;

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = tauri::async_runtime::spawn_blocking(session_manager::scan_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?;
    Ok(sessions)
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    let provider_id = providerId.clone();
    let source_path = sourcePath.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::load_messages(&provider_id, &source_path)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
}

#[tauri::command]
pub async fn launch_session_terminal(
    command: String,
    cwd: Option<String>,
    custom_config: Option<String>,
) -> Result<bool, String> {
    let command = command.clone();
    let cwd = cwd.clone();
    let custom_config = custom_config.clone();

    // Read preferred terminal from global settings
    let preferred = crate::settings::get_preferred_terminal();
    // Map global setting terminal names to session terminal names
    // Global uses "iterm2", session terminal uses "iterm"
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(t) => t.to_string(),
        None => "terminal".to_string(), // Default to Terminal.app on macOS
    };

    tauri::async_runtime::spawn_blocking(move || {
        session_manager::terminal::launch_terminal(
            &target,
            &command,
            cwd.as_deref(),
            custom_config.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Failed to launch terminal: {e}"))??;

    Ok(true)
}

#[tauri::command]
pub async fn get_all_session_aliases(
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, String>, String> {
    state.db.get_all_session_aliases().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_session_alias(
    state: tauri::State<'_, AppState>,
    sessionKey: String,
    alias: String,
) -> Result<(), String> {
    state
        .db
        .set_session_alias(&sessionKey, &alias)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_session_alias(
    state: tauri::State<'_, AppState>,
    sessionKey: String,
) -> Result<(), String> {
    state
        .db
        .delete_session_alias(&sessionKey)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_session_config(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    state.db.get_session_config(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_session_config(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    state
        .db
        .set_session_config(&key, &value)
        .map_err(|e| e.to_string())
}
