use tauri::State;

use crate::commands::codex_oauth::CodexOAuthState;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::kiro::KiroAuthState;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthError;
use crate::proxy::providers::copilot_auth::{
    CopilotAuthError, GitHubAccount, GitHubDeviceCodeResponse,
};
use tauri_plugin_opener::OpenerExt;

const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";
const AUTH_PROVIDER_KIRO: &str = "kiro";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthAccount {
    pub id: String,
    pub provider: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub is_default: bool,
    pub github_domain: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthStatus {
    pub provider: String,
    pub authenticated: bool,
    pub default_account_id: Option<String>,
    pub migration_error: Option<String>,
    pub accounts: Vec<ManagedAuthAccount>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthDeviceCodeResponse {
    pub provider: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

fn ensure_auth_provider(auth_provider: &str) -> Result<&'static str, String> {
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => Ok(AUTH_PROVIDER_GITHUB_COPILOT),
        AUTH_PROVIDER_CODEX_OAUTH => Ok(AUTH_PROVIDER_CODEX_OAUTH),
        AUTH_PROVIDER_KIRO => Ok(AUTH_PROVIDER_KIRO),
        _ => Err(format!("Unsupported auth provider: {auth_provider}")),
    }
}

fn map_account(
    provider: &str,
    account: GitHubAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
    }
}

fn map_device_code_response(
    provider: &str,
    response: GitHubDeviceCodeResponse,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_start_login(
    auth_provider: String,
    github_domain: Option<String>,
    start_url: Option<String>,
    region: Option<String>,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<ManagedAuthDeviceCodeResponse, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let response = auth_manager
                .start_device_flow(github_domain.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.read().await;
            let response = auth_manager
                .start_device_flow()
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.read().await;
            let response = auth_manager
                .start_device_flow(start_url.as_deref(), region.as_deref())
                .await?;
            Ok(map_device_code_response(auth_provider, response))
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_poll_for_account(
    auth_provider: String,
    device_code: String,
    github_domain: Option<String>,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<Option<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            match auth_manager
                .poll_for_token(&device_code, github_domain.as_deref())
                .await
            {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(CopilotAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.write().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.default_account_id().await;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(CodexOAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.write().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.default_account_id().await;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(e) => {
                    if e.contains("authorization_pending") {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            }
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_list_accounts(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<Vec<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.read().await;
            let accounts = auth_manager.list_accounts().await;
            let default_account_id = auth_manager.default_account_id().await;
            Ok(accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_get_status(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<ManagedAuthStatus, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: status.migration_error,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: None,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.read().await;
            let accounts = auth_manager.list_accounts().await;
            let authenticated = !accounts.is_empty();
            let default_account_id = auth_manager.default_account_id().await;
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: None,
                accounts: accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_remove_account(
    auth_provider: String,
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.write().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.write().await;
            auth_manager.remove_account(&account_id).await
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_set_default_account(
    auth_provider: String,
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.write().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.write().await;
            auth_manager.set_default_account(&account_id).await
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_logout(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = codex_state.0.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_KIRO => {
            let auth_manager = kiro_state.0.write().await;
            auth_manager.logout().await
        }
        _ => unreachable!(),
    }
}

/// Kiro 社交登录（Google / GitHub）：PKCE + localhost 回调。
/// 该命令会阻塞直到用户在浏览器完成登录或超时。
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_kiro_social_login(
    app: tauri::AppHandle,
    provider: Option<String>,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<ManagedAuthAccount, String> {
    let auth_manager = kiro_state.0.read().await;
    let app_for_open = app.clone();
    let account = auth_manager
        .social_login(provider.as_deref(), move |url| {
            if let Err(e) = app_for_open.opener().open_url(url, None::<&str>) {
                log::warn!("[Kiro] 打开浏览器失败: {e}");
            }
        })
        .await?;
    let default_account_id = auth_manager.default_account_id().await;
    Ok(map_account(
        AUTH_PROVIDER_KIRO,
        account,
        default_account_id.as_deref(),
    ))
}

/// 使用 KIRO_API_KEY（ksk_ 格式）登录 Kiro。
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_kiro_api_key_login(
    api_key: String,
    kiro_state: State<'_, KiroAuthState>,
) -> Result<ManagedAuthAccount, String> {
    let auth_manager = kiro_state.0.read().await;
    let account = auth_manager.apikey_login(&api_key).await?;
    let default_account_id = auth_manager.default_account_id().await;
    Ok(map_account(
        AUTH_PROVIDER_KIRO,
        account,
        default_account_id.as_deref(),
    ))
}

/// Kiro 主动导入本地 kiro-cli / kiro-ide 凭证（仅在用户点击按钮时读取）。
/// 返回本次新导入的账号列表。
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_kiro_import_dynamic(
    kiro_state: State<'_, KiroAuthState>,
) -> Result<Vec<ManagedAuthAccount>, String> {
    let auth_manager = kiro_state.0.read().await;
    let accounts = auth_manager.import_dynamic_accounts().await;
    let default_account_id = auth_manager.default_account_id().await;
    Ok(accounts
        .into_iter()
        .map(|a| map_account(AUTH_PROVIDER_KIRO, a, default_account_id.as_deref()))
        .collect())
}
