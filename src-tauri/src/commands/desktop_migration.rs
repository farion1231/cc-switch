#[cfg(unix)]
use crate::desktop_migration::{self, Inventory, Preview, Receipt, Request, Store};
#[cfg(unix)]
#[tauri::command]
pub async fn list_desktop_migration_profiles() -> Result<Inventory, String> {
    tauri::async_runtime::spawn_blocking(|| Store::live()?.inventory())
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn preview_desktop_session_migration(request: Request) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || Store::live()?.preview(&request))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn apply_desktop_session_migration(
    request: Request,
    token: String,
) -> Result<Receipt, String> {
    tauri::async_runtime::spawn_blocking(move || desktop_migration::apply_live(request, token))
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn list_desktop_migration_profiles() -> Result<serde_json::Value, String> {
    Err("Desktop session migration is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn preview_desktop_session_migration() -> Result<serde_json::Value, String> {
    Err("Desktop session migration is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn apply_desktop_session_migration() -> Result<serde_json::Value, String> {
    Err("Desktop session migration is currently supported on macOS.".into())
}
