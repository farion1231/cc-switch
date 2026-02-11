//! GitHub Copilot Tauri Commands
//!
//! 提供 Copilot OAuth 认证相关的 Tauri 命令。

use crate::proxy::providers::copilot_auth::{
    CopilotAuthManager, CopilotAuthStatus, CopilotModel, CopilotUsageResponse,
    GitHubDeviceCodeResponse,
};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Copilot 认证状态
pub struct CopilotAuthState(pub Arc<RwLock<CopilotAuthManager>>);

/// 启动设备码流程
///
/// 返回设备码和用户码，用于 OAuth 认证
#[tauri::command]
pub async fn copilot_start_device_flow(
    state: State<'_, CopilotAuthState>,
) -> Result<GitHubDeviceCodeResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .start_device_flow()
        .await
        .map_err(|e| e.to_string())
}

/// 轮询 OAuth Token
///
/// 使用设备码轮询 GitHub，等待用户完成授权
#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_poll_for_auth(
    device_code: String,
    state: State<'_, CopilotAuthState>,
) -> Result<bool, String> {
    let auth_manager = state.0.read().await;
    match auth_manager.poll_for_token(&device_code).await {
        Ok(()) => {
            log::info!("[CopilotAuth] 用户已授权");
            Ok(true)
        }
        Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
            Ok(false)
        }
        Err(e) => {
            log::error!("[CopilotAuth] 轮询失败: {}", e);
            Err(e.to_string())
        }
    }
}

/// 获取认证状态
#[tauri::command]
pub async fn copilot_get_auth_status(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotAuthStatus, String> {
    let auth_manager = state.0.read().await;
    Ok(auth_manager.get_status().await)
}

/// 注销 Copilot 认证
#[tauri::command]
pub async fn copilot_logout(state: State<'_, CopilotAuthState>) -> Result<(), String> {
    let auth_manager = state.0.read().await;
    auth_manager.clear_auth().await.map_err(|e| e.to_string())
}

/// 检查是否已认证
#[tauri::command]
pub async fn copilot_is_authenticated(state: State<'_, CopilotAuthState>) -> Result<bool, String> {
    let auth_manager = state.0.read().await;
    Ok(auth_manager.is_authenticated().await)
}

/// 获取有效的 Copilot Token
///
/// 内部使用，用于代理请求
#[tauri::command]
pub async fn copilot_get_token(state: State<'_, CopilotAuthState>) -> Result<String, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .get_valid_token()
        .await
        .map_err(|e| e.to_string())
}

/// 获取 Copilot 可用模型列表
#[tauri::command]
pub async fn copilot_get_models(
    state: State<'_, CopilotAuthState>,
) -> Result<Vec<CopilotModel>, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_models().await.map_err(|e| e.to_string())
}

/// 获取 Copilot 使用量信息
#[tauri::command]
pub async fn copilot_get_usage(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_usage().await.map_err(|e| e.to_string())
}
