use crate::services::kimi_oauth::{
    KimiLoginPoll, KimiLoginStart, KimiOAuthManager, KimiOAuthStatus, KimiRegion,
};
use tauri::State;

#[tauri::command]
pub fn kimi_oauth_status(state: State<'_, KimiOAuthManager>) -> Result<KimiOAuthStatus, String> {
    state.status()
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_start(
    region: KimiRegion,
    account_id: Option<String>,
    state: State<'_, KimiOAuthManager>,
) -> Result<KimiLoginStart, String> {
    state.start(region, account_id).await
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

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_logout(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<(), String> {
    state.logout(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_use_account(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<(), String> {
    state.use_account(&account_id).await
}
