use crate::services::kimi_oauth::{
    KimiLoginPoll, KimiLoginStart, KimiOAuthManager, KimiOAuthStatus, KimiRegion,
};
use tauri::State;

#[tauri::command]
pub fn kimi_oauth_status(state: State<'_, KimiOAuthManager>) -> Result<KimiOAuthStatus, String> {
    state.status()
}

#[tauri::command]
pub async fn kimi_oauth_start(
    region: KimiRegion,
    state: State<'_, KimiOAuthManager>,
) -> Result<KimiLoginStart, String> {
    state.start(region).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_poll(
    flow_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<KimiLoginPoll, String> {
    state.poll(&flow_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_cancel(
    flow_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<bool, String> {
    Ok(state.cancel(&flow_id).await)
}

#[tauri::command]
pub async fn kimi_oauth_logout(state: State<'_, KimiOAuthManager>) -> Result<(), String> {
    state.logout().await
}

#[tauri::command]
pub async fn kimi_oauth_use_account(state: State<'_, KimiOAuthManager>) -> Result<(), String> {
    state.use_account().await
}
