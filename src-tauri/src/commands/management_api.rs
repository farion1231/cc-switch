use crate::database::{ApiPairingSessionRecord, ApiTokenRecord};
use crate::services::management_api::{
    approve_pairing, CreateApiTokenRequest, CreateApiTokenResponse, ManagementApiStatus,
};
use crate::services::ManagementApiService;
use crate::store::AppState;

#[tauri::command]
pub async fn get_management_api_status(
    state: tauri::State<'_, AppState>,
    service: tauri::State<'_, ManagementApiService>,
) -> Result<ManagementApiStatus, String> {
    let _ = state; // Keeps command shape aligned with other state-backed commands.
    service.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_management_api(
    service: tauri::State<'_, ManagementApiService>,
) -> Result<ManagementApiStatus, String> {
    let mut settings = crate::settings::get_settings();
    settings.management_api.enabled = true;
    settings.management_api.normalize();
    service.start(settings.management_api.clone()).await?;
    if let Err(e) = crate::settings::update_settings(settings) {
        let _ = service.stop().await;
        return Err(e.to_string());
    }
    service.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_management_api(
    service: tauri::State<'_, ManagementApiService>,
) -> Result<ManagementApiStatus, String> {
    let mut settings = crate::settings::get_settings();
    settings.management_api.enabled = false;
    crate::settings::update_settings(settings).map_err(|e| e.to_string())?;
    let _ = service.stop().await;
    service.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restart_management_api(
    service: tauri::State<'_, ManagementApiService>,
) -> Result<ManagementApiStatus, String> {
    let mut settings = crate::settings::get_settings();
    settings.management_api.enabled = true;
    settings.management_api.normalize();
    crate::settings::update_settings(settings.clone()).map_err(|e| e.to_string())?;
    let _ = service.stop().await;
    service.start(settings.management_api).await?;
    service.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_management_api_tokens(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ApiTokenRecord>, String> {
    state.db.list_api_tokens().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_management_api_token(
    service: tauri::State<'_, ManagementApiService>,
    request: CreateApiTokenRequest,
) -> Result<CreateApiTokenResponse, String> {
    service
        .create_token(
            &request.name,
            request.scopes,
            request.expires_at,
            Some("ui"),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn revoke_management_api_token(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    state.db.revoke_api_token(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_management_api_pairing_sessions(
    state: tauri::State<'_, AppState>,
    include_consumed: Option<bool>,
) -> Result<Vec<ApiPairingSessionRecord>, String> {
    state
        .db
        .list_api_pairing_sessions(include_consumed.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn approve_management_api_pairing(
    state: tauri::State<'_, AppState>,
    service: tauri::State<'_, ManagementApiService>,
    pairing_id: String,
    name: String,
    scopes: Vec<String>,
    expires_at: Option<i64>,
) -> Result<ApiTokenRecord, String> {
    let created = approve_pairing(&state.db, &service, &pairing_id, &name, scopes, expires_at)
        .map_err(|e| e.to_string())?;
    Ok(created.record)
}

#[tauri::command]
pub async fn reject_management_api_pairing(
    state: tauri::State<'_, AppState>,
    pairing_id: String,
) -> Result<bool, String> {
    state
        .db
        .reject_api_pairing_session(&pairing_id)
        .map_err(|e| e.to_string())
}
