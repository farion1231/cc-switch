#[cfg(unix)]
use crate::{
    desktop_data::ProfileKey,
    desktop_history::{Inventory, Store},
};
#[cfg(unix)]
#[tauri::command]
pub async fn list_desktop_history() -> Result<Inventory, String> {
    tauri::async_runtime::spawn_blocking(|| Store::live()?.inventory())
        .await
        .map_err(|e| e.to_string())?
}
#[cfg(unix)]
#[tauri::command]
pub async fn open_desktop_history_transcript(
    profile: ProfileKey,
    session_id: String,
    transcript_id: String,
) -> Result<crate::session_manager::SessionMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Store::live()?.open_transcript(&profile, &session_id, &transcript_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn list_desktop_history() -> Result<serde_json::Value, String> {
    Err("Desktop history is currently supported on macOS.".into())
}
#[cfg(not(unix))]
#[tauri::command]
pub async fn open_desktop_history_transcript() -> Result<serde_json::Value, String> {
    Err("Desktop history is currently supported on macOS.".into())
}
