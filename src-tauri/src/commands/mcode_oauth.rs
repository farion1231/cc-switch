use crate::services::mcode_oauth::{
    McodeLoginPoll, McodeLoginStart, McodeOAuthManager, McodeOAuthStatus, McodeRegion,
};
use crate::services::subscription::SubscriptionQuota;
use tauri::State;

#[tauri::command]
pub async fn mcode_oauth_status(
    state: State<'_, McodeOAuthManager>,
) -> Result<McodeOAuthStatus, String> {
    state.status().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_start(
    region: McodeRegion,
    account_id: Option<String>,
    state: State<'_, McodeOAuthManager>,
) -> Result<McodeLoginStart, String> {
    state.start(region, account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_poll(
    flow_id: String,
    state: State<'_, McodeOAuthManager>,
) -> Result<McodeLoginPoll, String> {
    state.poll(&flow_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_cancel(
    flow_id: String,
    state: State<'_, McodeOAuthManager>,
) -> Result<bool, String> {
    Ok(state.cancel(&flow_id).await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_remove(
    account_id: String,
    state: State<'_, McodeOAuthManager>,
) -> Result<(), String> {
    state.remove(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_quota(
    account_id: String,
    state: State<'_, McodeOAuthManager>,
) -> Result<SubscriptionQuota, String> {
    state.quota(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mcode_oauth_use_account(
    account_id: String,
    state: State<'_, McodeOAuthManager>,
) -> Result<(), String> {
    state.use_account(&account_id).await
}
