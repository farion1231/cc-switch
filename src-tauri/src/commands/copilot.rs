//! GitHub Copilot Tauri Commands
//!
//! 提供 Copilot OAuth 认证相关的 Tauri 命令，支持多账号管理。

use crate::proxy::providers::copilot_auth::{
    CopilotAuthManager, CopilotAuthStatus, CopilotModel, CopilotUsageResponse, GitHubAccount,
    GitHubDeviceCodeResponse,
};
use cc_command_api::command_api;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Copilot 认证状态
pub struct CopilotAuthState(pub Arc<RwLock<CopilotAuthManager>>);

// ==================== 设备码流程 ====================

/// 启动设备码流程
///
/// 返回设备码和用户码，用于 OAuth 认证
#[command_api]
pub async fn copilot_start_device_flow(
    github_domain: Option<String>,
    state: &CopilotAuthState,
) -> Result<GitHubDeviceCodeResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .start_device_flow(github_domain.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// 轮询 OAuth Token（向后兼容）
///
/// 使用设备码轮询 GitHub，等待用户完成授权
/// 返回 true 表示授权成功，false 表示等待中
#[command_api]
pub async fn copilot_poll_for_auth(
    device_code: String,
    github_domain: Option<String>,
    state: &CopilotAuthState,
) -> Result<bool, String> {
    let auth_manager = state.0.write().await;
    match auth_manager
        .poll_for_token(&device_code, github_domain.as_deref())
        .await
    {
        Ok(Some(_account)) => {
            log::info!("[CopilotAuth] 用户已授权");
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
            Ok(false)
        }
        Err(e) => {
            log::error!("[CopilotAuth] 轮询失败: {e}");
            Err(e.to_string())
        }
    }
}

/// 轮询 OAuth Token（多账号版本）
///
/// 返回新添加的账号信息，如果授权成功
#[command_api]
pub async fn copilot_poll_for_account(
    device_code: String,
    github_domain: Option<String>,
    state: &CopilotAuthState,
) -> Result<Option<GitHubAccount>, String> {
    let auth_manager = state.0.write().await;
    match auth_manager
        .poll_for_token(&device_code, github_domain.as_deref())
        .await
    {
        Ok(account) => Ok(account),
        Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
            Ok(None)
        }
        Err(e) => {
            log::error!("[CopilotAuth] 轮询失败: {e}");
            Err(e.to_string())
        }
    }
}

// ==================== 多账号管理 ====================

/// 列出所有已认证的账号
#[command_api]
pub async fn copilot_list_accounts(
    state: &CopilotAuthState,
) -> Result<Vec<GitHubAccount>, String> {
    let auth_manager = state.0.read().await;
    Ok(auth_manager.list_accounts().await)
}

/// 移除指定账号
#[command_api]
pub async fn copilot_remove_account(
    account_id: String,
    state: &CopilotAuthState,
) -> Result<(), String> {
    let auth_manager = state.0.write().await;
    auth_manager
        .remove_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

/// 设置默认账号
#[command_api]
pub async fn copilot_set_default_account(
    account_id: String,
    state: &CopilotAuthState,
) -> Result<(), String> {
    let auth_manager = state.0.write().await;
    auth_manager
        .set_default_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== 状态查询 ====================

/// 获取认证状态（包含所有账号）
#[command_api]
pub async fn copilot_get_auth_status(
    state: &CopilotAuthState,
) -> Result<CopilotAuthStatus, String> {
    let auth_manager = state.0.read().await;
    Ok(auth_manager.get_status().await)
}

/// 检查是否已认证（有任意账号）
#[command_api]
pub async fn copilot_is_authenticated(state: &CopilotAuthState) -> Result<bool, String> {
    let auth_manager = state.0.read().await;
    Ok(auth_manager.is_authenticated().await)
}

/// 注销所有 Copilot 认证
#[command_api]
pub async fn copilot_logout(state: &CopilotAuthState) -> Result<(), String> {
    let auth_manager = state.0.write().await;
    auth_manager.clear_auth().await.map_err(|e| e.to_string())
}

// ==================== Token 获取 ====================

/// 获取有效的 Copilot Token（向后兼容：使用第一个账号）
///
/// 内部使用，用于代理请求
#[command_api]
pub async fn copilot_get_token(state: &CopilotAuthState) -> Result<String, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .get_valid_token()
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定账号的有效 Copilot Token
#[command_api]
pub async fn copilot_get_token_for_account(
    account_id: String,
    state: &CopilotAuthState,
) -> Result<String, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .get_valid_token_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== 模型和使用量 ====================

/// 获取 Copilot 可用模型列表（向后兼容：使用第一个账号）
#[command_api]
pub async fn copilot_get_models(
    state: &CopilotAuthState,
) -> Result<Vec<CopilotModel>, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_models().await.map_err(|e| e.to_string())
}

/// 获取指定账号的 Copilot 可用模型列表
#[command_api]
pub async fn copilot_get_models_for_account(
    account_id: String,
    state: &CopilotAuthState,
) -> Result<Vec<CopilotModel>, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_models_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取 Copilot 使用量信息（向后兼容：使用第一个账号）
#[command_api]
pub async fn copilot_get_usage(
    state: &CopilotAuthState,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_usage().await.map_err(|e| e.to_string())
}

/// 获取指定账号的 Copilot 使用量信息
#[command_api]
pub async fn copilot_get_usage_for_account(
    account_id: String,
    state: &CopilotAuthState,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_usage_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}
