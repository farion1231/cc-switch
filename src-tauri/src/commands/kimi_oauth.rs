use crate::services::kimi_oauth::{
    KimiLoginPoll, KimiLoginStart, KimiOAuthManager, KimiOAuthStatus, KimiRegion,
};
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn kimi_oauth_status(
    state: State<'_, KimiOAuthManager>,
) -> Result<KimiOAuthStatus, String> {
    state.status().await
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

/// Separate accounts are deleted with their saved profile; Kimi's own default
/// login is only removed from the list.
#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_remove(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .remove(&account_id, || {
            app_state.db.delete_provider("kimi", &account_id)?;
            Ok(())
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_quota(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<SubscriptionQuota, String> {
    state.quota(&account_id).await
}

/// The account's current official model catalog, for editing its card.
#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_models(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    state.models(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_use_account(
    account_id: String,
    state: State<'_, KimiOAuthManager>,
) -> Result<(), String> {
    state.use_account(&account_id).await
}
