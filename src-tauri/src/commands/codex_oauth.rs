//! Codex OAuth Tauri Commands
//!
//! 提供 OpenAI ChatGPT Plus/Pro OAuth 认证相关的 Tauri 命令。
//!
//! 大部分认证命令通过通用 `auth_*` 命令（参见 `commands::auth`）暴露给前端，
//! 此处定义 State wrapper 以及 Codex OAuth 专属的订阅额度和模型列表查询命令。

use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{query_codex_quota, CredentialStatus, SubscriptionQuota};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Codex OAuth 认证状态
pub struct CodexOAuthState(pub Arc<RwLock<CodexOAuthManager>>);

/// 查询 Codex OAuth (ChatGPT Plus/Pro) 订阅额度
///
/// - `account_id` 未指定时回退到 `CodexOAuthManager` 的默认账号
/// - 没有任何账号时返回 `not_found`，前端 `SubscriptionQuotaView` 会静默不渲染
/// - 复用 `services::subscription::query_codex_quota`，因此 wham/usage 端点协议
///   与 Codex CLI 路径完全一致
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_quota(
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;

    // 解析最终使用的账号 ID：显式 > 默认账号 > 无账号 (not_found)
    let resolved = match account_id {
        Some(id) => Some(id),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(SubscriptionQuota::not_found("codex_oauth"));
    };

    // 获取（必要时自动刷新）access_token
    let token = match manager.get_valid_token_for_account(&id).await {
        Ok(t) => t,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "codex_oauth",
                CredentialStatus::Expired,
                format!("Codex OAuth token unavailable: {e}"),
            ));
        }
    };

    Ok(query_codex_quota(
        &token,
        Some(&id),
        "codex_oauth",
        "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
    )
    .await)
}

/// 获取 Codex OAuth (ChatGPT Plus/Pro) 可用模型列表
///
/// ChatGPT Codex 反代使用 `chatgpt.com/backend-api/codex/*`，不是 OpenAI 兼容
/// `/v1/models`。这里复用托管 OAuth 账号的 access_token，直接读取 Codex 后端
/// 暴露的模型列表端点。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_models(
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Err("No ChatGPT account available".to_string());
    };

    let token = manager
        .get_valid_token_for_account(&id)
        .await
        .map_err(|e| format!("Codex OAuth token unavailable: {e}"))?;

    crate::services::codex_oauth_models::fetch_models_with_token(&token, &id).await
}
