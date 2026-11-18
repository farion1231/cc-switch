#[cfg(unix)]
use crate::desktop_tasks::{self, Inventory, Preview, Receipt, Request, Store};
#[cfg(unix)]
#[tauri::command]
pub async fn list_desktop_task_profiles() -> Result<Inventory, String> {
    tauri::async_runtime::spawn_blocking(|| Store::live()?.inventory())
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn preview_desktop_task_recovery(request: Request) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || Store::live()?.preview(&request))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn apply_desktop_task_recovery(
    request: Request,
    token: String,
) -> Result<Receipt, String> {
    tauri::async_runtime::spawn_blocking(move || desktop_tasks::apply_live(request, token))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn read_desktop_saved_prompt(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || Store::live()?.prompt(&id))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn list_desktop_task_profiles() -> Result<serde_json::Value, String> {
    Err("Desktop task recovery is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn preview_desktop_task_recovery() -> Result<serde_json::Value, String> {
    Err("Desktop task recovery is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn apply_desktop_task_recovery() -> Result<serde_json::Value, String> {
    Err("Desktop task recovery is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn read_desktop_saved_prompt() -> Result<serde_json::Value, String> {
    Err("Desktop task recovery is currently supported on macOS.".into())
}
