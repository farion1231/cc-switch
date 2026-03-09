#![allow(non_snake_case)]

use crate::bridges::session as session_bridge;
use crate::session_manager;

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    tauri::async_runtime::spawn_blocking(session_bridge::list_sessions)
        .await
        .map_err(|e| format!("Failed to scan sessions: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_session_messages(
    providerId: String,
    sourcePath: String,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_bridge::get_session_messages(&providerId, &sourcePath)
    })
    .await
    .map_err(|e| format!("Failed to load session messages: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn launch_session_terminal(
    command: String,
    cwd: Option<String>,
    custom_config: Option<String>,
) -> Result<bool, String> {
    let preferred = crate::settings::get_preferred_terminal();
    let target = match preferred.as_deref() {
        Some("iterm2") => "iterm".to_string(),
        Some(value) => value.to_string(),
        None => "terminal".to_string(),
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
