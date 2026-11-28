pub use crate::services::desktop_app::DesktopAppStatus;

#[tauri::command]
pub async fn get_desktop_app_status(app: String) -> Result<DesktopAppStatus, String> {
    tauri::async_runtime::spawn_blocking(move || crate::services::desktop_app::status(&app))
        .await
        .map_err(|error| format!("Failed to inspect desktop app status: {error}"))?
}

#[tauri::command]
pub async fn launch_desktop_app(app: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::services::desktop_app::launch(&app))
        .await
        .map_err(|error| format!("Failed to launch desktop app: {error}"))?
}

#[tauri::command]
pub async fn restart_desktop_app(app: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::services::desktop_app::restart(&app))
        .await
        .map_err(|error| format!("Failed to restart desktop app: {error}"))?
}
