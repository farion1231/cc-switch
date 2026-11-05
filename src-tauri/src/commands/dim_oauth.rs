//! DimAgent account sign-in commands. Tokens remain in the Rust manager.
use crate::proxy::providers::dim_oauth_auth::{
    DimLoginStart, DimModel, DimOAuthAccount, DimOAuthManager, DimOAuthStatus, DimSite, DimUsage,
};
use std::sync::Arc;
use tauri::State;

pub struct DimOAuthState(pub Result<Arc<DimOAuthManager>, String>);

impl DimOAuthState {
    pub fn manager(&self) -> Result<&Arc<DimOAuthManager>, String> {
        self.0.as_ref().map_err(Clone::clone)
    }
}

#[tauri::command]
pub async fn start_dim_oauth_login(
    site: DimSite,
    state: State<'_, DimOAuthState>,
) -> Result<DimLoginStart, String> {
    state.manager()?.start_login(site).await
}
#[tauri::command]
pub async fn poll_dim_oauth_login(
    flow_id: String,
    state: State<'_, DimOAuthState>,
) -> Result<Option<DimOAuthAccount>, String> {
    state.manager()?.poll_login(&flow_id).await
}
#[tauri::command]
pub async fn cancel_dim_oauth_login(
    flow_id: String,
    state: State<'_, DimOAuthState>,
) -> Result<(), String> {
    state.manager()?.cancel_login(&flow_id).await
}
#[tauri::command]
pub async fn get_dim_oauth_status(
    state: State<'_, DimOAuthState>,
) -> Result<DimOAuthStatus, String> {
    Ok(state.manager()?.status().await)
}
#[tauri::command]
pub async fn remove_dim_oauth_account(
    account_id: String,
    state: State<'_, DimOAuthState>,
) -> Result<(), String> {
    state.manager()?.remove_account(&account_id).await
}
#[tauri::command]
pub async fn set_dim_oauth_default_account(
    account_id: String,
    state: State<'_, DimOAuthState>,
) -> Result<(), String> {
    state.manager()?.set_default_account(&account_id).await
}
#[tauri::command]
pub async fn get_dim_oauth_models(
    account_id: Option<String>,
    state: State<'_, DimOAuthState>,
) -> Result<Vec<DimModel>, String> {
    let id = state
        .manager()?
        .resolve_account_id(account_id.as_deref())
        .await?;
    state.manager()?.get_models(&id).await
}
#[tauri::command]
pub async fn get_dim_oauth_usage(
    account_id: Option<String>,
    state: State<'_, DimOAuthState>,
) -> Result<DimUsage, String> {
    let id = state
        .manager()?
        .resolve_account_id(account_id.as_deref())
        .await?;
    state.manager()?.get_usage(&id).await
}
