use tauri::State;

use crate::app_config::AppType;
use crate::commands::agy_account::AgyAccountState;
use crate::commands::claude_account::ClaudeAccountState;
use crate::commands::codex_oauth::CodexOAuthState;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::xai_oauth::XaiOAuthState;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthError;
use crate::proxy::providers::copilot_auth::{
    CopilotAuthError, GitHubAccount, GitHubDeviceCodeResponse,
};
use crate::proxy::providers::xai_oauth_auth::{XaiOAuthAccount, XaiOAuthError};
use crate::services::agy_account::AgyAccount;
use crate::services::claude_account::ClaudeAccount;
use crate::store::AppState;

const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";
const AUTH_PROVIDER_XAI_OAUTH: &str = "xai_oauth";
const AUTH_PROVIDER_AGY_OAUTH: &str = "agy_oauth";
const AUTH_PROVIDER_CLAUDE_OFFICIAL: &str = "claude_official";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthAccount {
    pub id: String,
    pub provider: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub is_default: bool,
    pub github_domain: String,
    /// Codex 专用：旧账号缺少写入原生 Codex auth.json 所需的 id_token。
    pub reauth_required: bool,
    /// xAI 专用：refresh token 已失效，账号不可再用于请求。
    pub requires_reauth: bool,
    /// Claude 官方专用：不在 Claude Code 默认登录里的账号，登录约什么时候过期（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_expires_at: Option<i64>,
    /// Claude 官方专用：套餐（`pro` / `max` / `team` / `enterprise`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Claude 官方专用：凭据里的 `rateLimitTier`，区分 Max 5x / 20x
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_tier: Option<String>,
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
    /// 登录页已经由 CLI 自己在浏览器里打开了（Claude 官方账号在 Windows 上），窗口不要再开
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub browser_opened: bool,
}

fn ensure_auth_provider(auth_provider: &str) -> Result<&'static str, String> {
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => Ok(AUTH_PROVIDER_GITHUB_COPILOT),
        AUTH_PROVIDER_CODEX_OAUTH => Ok(AUTH_PROVIDER_CODEX_OAUTH),
        AUTH_PROVIDER_XAI_OAUTH => Ok(AUTH_PROVIDER_XAI_OAUTH),
        AUTH_PROVIDER_AGY_OAUTH => Ok(AUTH_PROVIDER_AGY_OAUTH),
        AUTH_PROVIDER_CLAUDE_OFFICIAL => Ok(AUTH_PROVIDER_CLAUDE_OFFICIAL),
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
        reauth_required: account.reauth_required,
        requires_reauth: false,
        login_expires_at: None,
        plan: None,
        rate_limit_tier: None,
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
    }
}

fn map_xai_account(
    account: XaiOAuthAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: AUTH_PROVIDER_XAI_OAUTH.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
        reauth_required: false,
        requires_reauth: account.requires_reauth,
        login_expires_at: None,
        plan: None,
        rate_limit_tier: None,
    }
}

fn map_agy_account(account: AgyAccount, current_account_id: Option<&str>) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: current_account_id == Some(account.id.as_str()),
        login: account.label(),
        avatar_url: account.picture,
        authenticated_at: account.authenticated_at,
        github_domain: "google.com".to_string(),
        reauth_required: false,
        requires_reauth: account.requires_reauth,
        login_expires_at: None,
        plan: None,
        rate_limit_tier: None,
        provider: AUTH_PROVIDER_AGY_OAUTH.to_string(),
        id: account.id,
    }
}

/// Claude 官方账号没有「默认账号」：用哪个号由绑定它的官方卡决定。
fn map_claude_account(account: ClaudeAccount) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: false,
        login: account.label(),
        avatar_url: None,
        authenticated_at: account.authenticated_at,
        github_domain: "claude.ai".to_string(),
        reauth_required: false,
        requires_reauth: account.requires_reauth,
        login_expires_at: account.login_expires_at,
        plan: account.plan.clone(),
        rate_limit_tier: account.rate_limit_tier.clone(),
        provider: AUTH_PROVIDER_CLAUDE_OFFICIAL.to_string(),
        id: account.id,
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
        browser_opened: false,
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_start_login(
    auth_provider: String,
    github_domain: Option<String>,
    target_account_id: Option<String>,
    login_method: Option<String>,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
) -> Result<ManagedAuthDeviceCodeResponse, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    let browser_login = match login_method.as_deref().unwrap_or("device_code") {
        "device_code" => false,
        "browser" if auth_provider == AUTH_PROVIDER_CODEX_OAUTH => true,
        _ => return Err("Unsupported login method for this auth provider".into()),
    };
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let auth_manager = copilot_state.0.read().await;
            let response = auth_manager
                .start_device_flow(github_domain.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &codex_state.0;
            let response = if browser_login {
                auth_manager
                    .start_browser_flow(target_account_id.as_deref())
                    .await
            } else {
                auth_manager
                    .start_device_flow(target_account_id.as_deref())
                    .await
            }
            .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let auth_manager = xai_state.0.read().await;
            let response = auth_manager
                .start_device_flow()
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        // 浏览器登录没有用户码：device_code 放这次登录的 id，verification_uri 放授权页地址
        AUTH_PROVIDER_AGY_OAUTH => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let start = agy_state.0.start_login().await.map_err(|e| e.to_string())?;
            Ok(ManagedAuthDeviceCodeResponse {
                provider: auth_provider.to_string(),
                device_code: start.flow_id,
                user_code: String::new(),
                verification_uri: start.auth_url,
                expires_in: start.expires_in,
                interval: 1,
                browser_opened: false,
            })
        }
        // 同上：在新账号目录里跑 `claude auth login`，授权页地址是它要打开的那个
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let start = claude_state
                .0
                .start_login()
                .await
                .map_err(|e| e.to_string())?;
            Ok(ManagedAuthDeviceCodeResponse {
                provider: auth_provider.to_string(),
                device_code: start.flow_id,
                user_code: String::new(),
                verification_uri: start.auth_url,
                expires_in: start.expires_in,
                interval: 1,
                browser_opened: start.browser_opened,
            })
        }
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_poll_for_account(
    auth_provider: String,
    device_code: String,
    github_domain: Option<String>,
    app_state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
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
            let auth_manager = &codex_state.0;
            match auth_manager
                .poll_for_token(&device_code, || async {
                    app_state
                        .proxy_service
                        .lock_switch_for_app(AppType::Codex.as_str())
                        .await
                })
                .await
            {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(CodexOAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.write().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account
                        .map(|account| map_xai_account(account, default_account_id.as_deref())))
                }
                Err(XaiOAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_AGY_OAUTH => {
            let Some(account) = agy_state
                .0
                .poll_login(&device_code)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(None);
            };
            let current = agy_state
                .0
                .status()
                .await
                .ok()
                .and_then(|status| status.current_account_id);
            Ok(Some(map_agy_account(account, current.as_deref())))
        }
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            let account = claude_state
                .0
                .poll_login(&device_code)
                .await
                .map_err(|e| e.to_string())?;
            if account.is_some() {
                // 同一账号重新登录会换掉目录：正在用它的官方卡要把新目录写进 live
                crate::services::provider::claude_official::refresh_live_after_account_change(
                    app_state.inner(),
                    &claude_state.0,
                )
                .await?;
            }
            Ok(account.map(map_claude_account))
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_cancel_login(
    auth_provider: String,
    device_code: String,
    codex_state: State<'_, CodexOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
) -> Result<bool, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_CODEX_OAUTH => Ok(codex_state.0.cancel_device_flow(&device_code).await),
        AUTH_PROVIDER_AGY_OAUTH => Ok(agy_state.0.cancel_login(&device_code).await),
        AUTH_PROVIDER_CLAUDE_OFFICIAL => Ok(claude_state.0.cancel_login(&device_code).await),
        _ => Err("Login cancellation is only supported for Codex OAuth".to_string()),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_list_accounts(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
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
            let auth_manager = &codex_state.0;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_xai_account(account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_AGY_OAUTH => {
            let status = agy_state.0.status().await.map_err(|e| e.to_string())?;
            let current = status.current_account_id;
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_agy_account(account, current.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_CLAUDE_OFFICIAL => Ok(claude_state
            .0
            .list()
            .await
            .into_iter()
            .map(map_claude_account)
            .collect()),
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_get_status(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
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
            let auth_manager = &codex_state.0;
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
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.read().await;
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
                    .map(|account| map_xai_account(account, default_account_id.as_deref()))
                    .collect(),
            })
        }
        // 「默认账号」在这里是 agy 当前登录的那个号
        AUTH_PROVIDER_AGY_OAUTH => {
            let status = agy_state.0.status().await.map_err(|e| e.to_string())?;
            let current = status.current_account_id;
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: !status.accounts.is_empty(),
                default_account_id: current.clone(),
                migration_error: None,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| map_agy_account(account, current.as_deref()))
                    .collect(),
            })
        }
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            let accounts = claude_state.0.list().await;
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: !accounts.is_empty(),
                default_account_id: None,
                migration_error: None,
                accounts: accounts.into_iter().map(map_claude_account).collect(),
            })
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_remove_account(
    auth_provider: String,
    account_id: String,
    app_state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
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
            remove_codex_oauth_account_with_switch_lock(app_state.inner(), &account_id).await
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.write().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        // 只删 CC Switch 里的副本，不替 agy 登出
        AUTH_PROVIDER_AGY_OAUTH => agy_state
            .0
            .remove(&account_id)
            .await
            .map_err(|e| e.to_string()),
        // 删目录和钥匙串条目（不吊销、不登出）；正在用它的官方卡退回默认登录
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            claude_state
                .0
                .remove(&account_id)
                .await
                .map_err(|e| e.to_string())?;
            crate::services::provider::claude_official::refresh_live_after_account_change(
                app_state.inner(),
                &claude_state.0,
            )
            .await
        }
        _ => unreachable!(),
    }
}

pub(crate) async fn remove_codex_oauth_account_with_switch_lock(
    app_state: &AppState,
    account_id: &str,
) -> Result<(), String> {
    // Serialize Auth Center credential deletion with managed provider
    // add/update/switch/hot-switch. Otherwise a switch that already preflighted
    // a bundle could recreate auth.json after removal.
    let _switch_guard = app_state
        .proxy_service
        .lock_switch_for_app(AppType::Codex.as_str())
        .await;
    app_state
        .codex_oauth_manager
        .remove_account(account_id)
        .await
        .map_err(|error| error.to_string())?;
    app_state
        .db
        .unbind_codex_managed_accounts(&[account_id.to_string()])
        .map_err(|error| {
            format!("账号已删除，但 Codex 供应商解绑失败；请编辑供应商重新选择账号或切换供应商: {error}")
        })?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_set_default_account(
    auth_provider: String,
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_state: State<'_, CodexOAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    _claude_state: State<'_, ClaudeAccountState>,
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
            let auth_manager = &codex_state.0;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.write().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        // 切 agy 的全局登录（写钥匙串）
        AUTH_PROVIDER_AGY_OAUTH => agy_state
            .0
            .switch_to(&account_id)
            .await
            .map_err(|e| e.to_string()),
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            let _ = account_id;
            Err("Claude accounts are used through the official provider card bound to them".into())
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_logout(
    auth_provider: String,
    app_state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    agy_state: State<'_, AgyAccountState>,
    claude_state: State<'_, ClaudeAccountState>,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => logout_codex_oauth_with_switch_lock(app_state.inner()).await,
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = xai_state.0.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_AGY_OAUTH => agy_state
            .0
            .remove_all()
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        AUTH_PROVIDER_CLAUDE_OFFICIAL => {
            claude_state
                .0
                .remove_all()
                .await
                .map_err(|e| e.to_string())?;
            crate::services::provider::claude_official::refresh_live_after_account_change(
                app_state.inner(),
                &claude_state.0,
            )
            .await
        }
        _ => unreachable!(),
    }
}

pub(crate) async fn logout_codex_oauth_with_switch_lock(
    app_state: &AppState,
) -> Result<(), String> {
    let _switch_guard = app_state
        .proxy_service
        .lock_switch_for_app(AppType::Codex.as_str())
        .await;
    // Only this device's accounts: rows synced from another device stay bound there.
    let removed: Vec<String> = app_state
        .codex_oauth_manager
        .list_accounts()
        .await
        .into_iter()
        .map(|account| account.id)
        .collect();
    app_state
        .codex_oauth_manager
        .clear_auth()
        .await
        .map_err(|error| error.to_string())?;
    app_state
        .db
        .unbind_codex_managed_accounts(&removed)
        .map_err(|error| {
            format!("托管账号已登出，但 Codex 供应商解绑失败；请编辑供应商重新选择账号或切换供应商: {error}")
        })?;
    Ok(())
}
