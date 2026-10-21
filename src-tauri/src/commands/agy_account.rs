//! Antigravity 官方账号的状态与专用命令（加号、切号走授权中心的通用 `auth_*` 命令）。

use std::sync::Arc;

use crate::services::agy_account::{AccountQuotaError, AgyAccountManager};
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};

const TOOL: &str = "agy";

pub struct AgyAccountState(pub Arc<AgyAccountManager>);

/// 切号后提醒用：在跑的 agy 还用旧账号，续期时可能把旧登录写回钥匙串。
#[tauri::command]
pub async fn agy_account_is_running() -> Result<bool, String> {
    tokio::task::spawn_blocking(crate::services::agy_account::agy_running)
        .await
        .map_err(|error| error.to_string())
}

/// 授权中心逐个账号显示的剩余额度：用该账号自己的令牌直接查，不用先切号。
#[tauri::command(rename_all = "camelCase")]
pub async fn agy_account_quota(
    account_id: String,
    state: tauri::State<'_, AgyAccountState>,
) -> Result<SubscriptionQuota, String> {
    Ok(match state.0.quota(&account_id).await {
        Ok(tiers) => SubscriptionQuota {
            tool: TOOL.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers,
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            error: None,
            queried_at: Some(chrono::Utc::now().timestamp_millis()),
        },
        Err(AccountQuotaError::NotFound) => SubscriptionQuota::not_found(TOOL),
        Err(AccountQuotaError::NeedsReauth) => SubscriptionQuota::error(
            TOOL,
            CredentialStatus::Expired,
            "Antigravity sign-in expired".to_string(),
        ),
        Err(AccountQuotaError::Failed(message)) => {
            SubscriptionQuota::error(TOOL, CredentialStatus::Valid, message)
        }
    })
}
