#[tauri::command]
pub async fn get_copilot_vscode_targets(
) -> Result<Vec<crate::copilot_vscode::VsCodeProfileTarget>, String> {
    tauri::async_runtime::spawn_blocking(crate::copilot_vscode::discover_vscode_targets)
        .await
        .map_err(|error| format!("VS Code profile discovery task failed: {error}"))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn sync_copilot_vscode() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(crate::copilot_config::sync_vscode)
        .await
        .map_err(|error| format!("VS Code synchronization task failed: {error}"))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_copilot_vscode_sync_status() -> Result<crate::copilot_vscode::SyncStatus, String> {
    tauri::async_runtime::spawn_blocking(crate::copilot_config::vscode_sync_status)
        .await
        .map_err(|error| format!("VS Code sync status task failed: {error}"))?
        .map_err(Into::into)
}
