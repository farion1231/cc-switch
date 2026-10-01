//! VPS host mutations finish only after catalogs and managed Skill deployments converge.

use crate::services::vps::{VpsServer, VpsService};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_vps_servers(app_state: State<'_, AppState>) -> Result<Vec<VpsServer>, String> {
    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || VpsService::new().get_servers_with_skills(&db))
        .await
        .map_err(|error| format!("VPS worker failed: {error}"))?
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn save_vps_server(
    server: VpsServer,
    password: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<Vec<VpsServer>, String> {
    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        VpsService::new().save_server_with_skills_and_password(&db, server, password)
    })
    .await
    .map_err(|error| format!("VPS worker failed: {error}"))?
    .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn delete_vps_server(
    id: String,
    app_state: State<'_, AppState>,
) -> Result<Vec<VpsServer>, String> {
    let db = app_state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        VpsService::new().delete_server_with_skills(&db, &id)
    })
    .await
    .map_err(|error| format!("VPS worker failed: {error}"))?
    .map_err(|error| format!("{error:#}"))
}
