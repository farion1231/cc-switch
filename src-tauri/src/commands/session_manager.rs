#![allow(non_snake_case)]

use crate::session_manager;
use tauri_plugin_dialog::DialogExt;

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
pub async fn save_session_export_file_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    defaultName: String,
    format: String,
) -> Result<Option<String>, String> {
    let ext = match format.as_str() {
        "md" => "md",
        "json" => "json",
        _ => return Err(format!("Unsupported export format: {format}")),
    };

    let result = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(&defaultName)
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("Failed to open save dialog: {e}"))?;

    Ok(result.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn export_session_to_file(
    providerId: String,
    sourcePath: String,
    format: String,
    filePath: String,
    sessionId: Option<String>,
    title: Option<String>,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::export_session_to_file(
            &providerId,
            &sourcePath,
            sessionId.as_deref(),
            title.as_deref(),
            &format,
            &filePath,
        )
    })
    .await
    .map_err(|e| format!("Failed to export session: {e}"))??;

    Ok(true)
}

#[tauri::command]
pub async fn export_sessions_to_directory(
    sessions: Vec<session_manager::SessionExportTarget>,
    format: String,
    directoryPath: String,
) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        session_manager::export_sessions_to_directory(sessions, &format, &directoryPath)
    })
    .await
    .map_err(|e| format!("Failed to batch export sessions: {e}"))?
}
