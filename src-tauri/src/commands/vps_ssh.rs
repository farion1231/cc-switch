//! Explicit VPS SSH testing and fingerprint confirmation commands.

use crate::services::vps::{
    ssh::{VpsConnectionTestResult, VpsSshState},
    VpsServer,
};
use tauri::State;

#[tauri::command]
pub async fn test_vps_connection(
    server: VpsServer,
    request_id: String,
    password: Option<String>,
    state: State<'_, VpsSshState>,
) -> Result<VpsConnectionTestResult, String> {
    let state = state.inner().clone();
    Ok(state.test_connection(server, request_id, password).await)
}

#[tauri::command]
pub async fn cancel_vps_connection_test(
    request_id: String,
    state: State<'_, VpsSshState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || state.cancel_test(&request_id))
        .await
        .map_err(|_| "SSH cancellation worker failed".to_string())?
}

#[tauri::command]
pub async fn confirm_vps_host_key(
    confirmation_token: String,
    state: State<'_, VpsSshState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || state.confirm_host_key(&confirmation_token))
        .await
        .map_err(|_| "SSH confirmation worker failed".to_string())?
}
