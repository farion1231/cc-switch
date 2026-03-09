use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::bridges::workspace as workspace_bridge;
use crate::openclaw_config::get_openclaw_dir;

#[tauri::command]
pub async fn list_daily_memory_files() -> Result<Vec<cc_switch_core::DailyMemoryFileInfo>, String> {
    workspace_bridge::list_daily_memory_files().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_daily_memory_file(filename: String) -> Result<Option<String>, String> {
    workspace_bridge::read_daily_memory_file(&filename).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_daily_memory_file(filename: String, content: String) -> Result<(), String> {
    workspace_bridge::write_daily_memory_file(&filename, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_daily_memory_files(
    query: String,
) -> Result<Vec<cc_switch_core::DailyMemorySearchResult>, String> {
    workspace_bridge::search_daily_memory_files(&query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_daily_memory_file(filename: String) -> Result<(), String> {
    workspace_bridge::delete_daily_memory_file(&filename).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_workspace_file(filename: String) -> Result<Option<String>, String> {
    workspace_bridge::read_workspace_file(&filename).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_workspace_file(filename: String, content: String) -> Result<(), String> {
    workspace_bridge::write_workspace_file(&filename, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_workspace_directory(handle: AppHandle, subdir: String) -> Result<bool, String> {
    let dir = match subdir.as_str() {
        "memory" => get_openclaw_dir().join("workspace").join("memory"),
        _ => get_openclaw_dir().join("workspace"),
    };

    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Failed to open directory: {e}"))?;

    Ok(true)
}
