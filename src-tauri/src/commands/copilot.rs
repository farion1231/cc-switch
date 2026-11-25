//! GitHub Copilot Tauri Commands
//!
//! 提供 Copilot OAuth 认证相关的 Tauri 命令，支持多账号管理。

use crate::proxy::providers::copilot_auth::{
    CopilotAuthManager, CopilotModelOptions, CopilotUsageResponse, GitHubAccount,
};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Copilot 认证状态
pub struct CopilotAuthState(pub Arc<RwLock<CopilotAuthManager>>);

// ==================== 设备码流程 ====================

// ==================== 多账号管理 ====================

// ==================== 状态查询 ====================

// ==================== Token 获取 ====================

// ==================== 模型和使用量 ====================

/// 获取当前默认账号可供用户选择的 Copilot 模型。
#[tauri::command]
pub async fn copilot_get_models(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotModelOptions, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_model_options()
        .await
        .map_err(|e| e.to_string())
}

/// 获取指定账号可供用户选择的 Copilot 模型。
#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_models_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotModelOptions, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_model_options_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

/// 获取 Copilot 使用量信息（向后兼容：使用第一个账号）
#[tauri::command]
pub async fn copilot_get_usage(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_usage().await.map_err(|e| e.to_string())
}

/// 获取指定账号的 Copilot 使用量信息
#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_usage_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_usage_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

// ==================== Copilot CLI 当前登录 ====================

/// Copilot CLI 当前登录的 GitHub 账号，以及授权中心里与它对应的账号（没有时为 `None`）。
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CopilotCliSignedInAccount {
    pub login: String,
    pub github_domain: String,
    pub account_id: Option<String>,
}

/// 读 `~/.copilot/config.json` 的 `lastLoggedInUser`（Copilot 写的是带注释的 JSON）。
fn copilot_cli_login() -> Option<(String, String)> {
    let path = crate::copilot_config::copilot_dir().join("config.json");
    let config = crate::copilot_config::read_json_document(&path).ok()??;
    let user = config.get("lastLoggedInUser")?;
    let login = user.get("login")?.as_str()?.trim().to_string();
    if login.is_empty() {
        return None;
    }
    let host = user
        .get("host")
        .and_then(|value| value.as_str())
        .unwrap_or("https://github.com");
    let domain = url::Url::parse(host)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| host.trim().trim_end_matches('/').to_string());
    Some((login, domain))
}

fn match_signed_in_account(
    login: String,
    github_domain: String,
    accounts: &[GitHubAccount],
) -> CopilotCliSignedInAccount {
    let account_id = accounts
        .iter()
        .find(|account| {
            account.login.eq_ignore_ascii_case(&login)
                && account.github_domain.eq_ignore_ascii_case(&github_domain)
        })
        .map(|account| account.id.clone());
    CopilotCliSignedInAccount {
        login,
        github_domain,
        account_id,
    }
}

/// 官方 Copilot CLI 卡片没绑账号时，用量查询跟着 Copilot CLI 当前登录的账号走。
/// Copilot CLI 没登录时返回 `None`。
#[tauri::command]
pub async fn copilot_cli_signed_in_account(
    state: State<'_, CopilotAuthState>,
) -> Result<Option<CopilotCliSignedInAccount>, String> {
    let Some((login, github_domain)) = copilot_cli_login() else {
        return Ok(None);
    };
    let accounts = state.0.read().await.list_accounts().await;
    Ok(Some(match_signed_in_account(
        login,
        github_domain,
        &accounts,
    )))
}

#[cfg(test)]
mod copilot_cli_login_tests {
    use super::*;

    fn account(id: &str, login: &str, domain: &str) -> GitHubAccount {
        GitHubAccount {
            id: id.into(),
            login: login.into(),
            avatar_url: None,
            authenticated_at: 0,
            github_domain: domain.into(),
            reauth_required: false,
        }
    }

    #[test]
    fn signed_in_login_matches_auth_center_account_by_login_and_domain() {
        let accounts = [
            account("1", "Octo", "github.com"),
            account("ghe.example.com:2", "octo", "ghe.example.com"),
        ];
        let matched = match_signed_in_account("octo".into(), "github.com".into(), &accounts);
        assert_eq!(matched.account_id.as_deref(), Some("1"));
        let matched = match_signed_in_account("octo".into(), "ghe.example.com".into(), &accounts);
        assert_eq!(matched.account_id.as_deref(), Some("ghe.example.com:2"));
        let missing = match_signed_in_account("other".into(), "github.com".into(), &accounts);
        assert_eq!(missing.account_id, None);
        assert_eq!(missing.login, "other");
    }
}
