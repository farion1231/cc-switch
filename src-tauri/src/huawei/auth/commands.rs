use super::manager::{
    HuaweiCodeAgentAccount, HuaweiCodeAgentAuthManager, HuaweiCodeAgentAuthStatus,
};
use crate::error::AppError;
use crate::services::model_fetch::FetchedModel;
use std::sync::Arc;

pub struct HuaweiCodeAgentAuthState(pub Arc<HuaweiCodeAgentAuthManager>);

#[tauri::command]
pub async fn huawei_codeagent_2_start_login(
    user: String,
    password: String,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<HuaweiCodeAgentAuthStatus, AppError> {
    let token = state.0.login(&user, &password).await?;
    Ok(HuaweiCodeAgentAuthStatus {
        authenticated: true,
        user: Some(token.user),
        expires_at: Some(token.expires_at),
        account_id: Some(token.account_id),
    })
}

#[tauri::command]
pub async fn huawei_codeagent_2_get_status(
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<HuaweiCodeAgentAuthStatus, AppError> {
    Ok(state.0.get_status().await)
}

#[tauri::command]
pub async fn huawei_codeagent_2_logout(
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<(), AppError> {
    state.0.logout().await
}

#[tauri::command]
pub async fn huawei_codeagent_2_refresh_token(
    account_id: String,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<HuaweiCodeAgentAuthStatus, AppError> {
    let token = state.0.refresh_token(&account_id).await?;
    Ok(HuaweiCodeAgentAuthStatus {
        authenticated: true,
        user: Some(token.user),
        expires_at: Some(token.expires_at),
        account_id: Some(token.account_id),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn huawei_codeagent_2_list_accounts(
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<Vec<HuaweiCodeAgentAccount>, AppError> {
    Ok(state.0.list_accounts().await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn huawei_codeagent_2_remove_account(
    account_id: String,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<(), AppError> {
    state.0.remove_account(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn huawei_codeagent_2_set_default_account(
    account_id: String,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<(), AppError> {
    state.0.set_default_account(&account_id).await
}

async fn selected_token(
    manager: &Arc<HuaweiCodeAgentAuthManager>,
    account_id: Option<String>,
) -> Result<String, AppError> {
    let account_id = match account_id.filter(|id| !id.trim().is_empty()) {
        Some(id) => id,
        None => manager
            .get_status()
            .await
            .account_id
            .ok_or_else(|| AppError::Message("华为 CodeAgent 未登录，请先登录".into()))?,
    };
    // Let the manager refresh an expired token, including for a non-default account.
    manager.get_token_with_refresh(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn huawei_codeagent_2_fetch_models(
    account_id: Option<String>,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<Vec<FetchedModel>, AppError> {
    let token = selected_token(&state.0, account_id).await?;
    super::catalog::fetch_models(state.0.get_client(), &token).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn huawei_codeagent_2_fetch_quota(
    account_id: Option<String>,
    model_id: Option<String>,
    state: tauri::State<'_, HuaweiCodeAgentAuthState>,
) -> Result<serde_json::Value, AppError> {
    let token = selected_token(&state.0, account_id).await?;
    super::catalog::fetch_quota(state.0.get_client(), &token, model_id.as_deref()).await
}
