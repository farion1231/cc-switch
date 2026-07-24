//! xAI OAuth state and xAI-specific commands.

use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::proxy::providers::XAI_API_BASE_URL;
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;
use tokio::sync::RwLock;

pub struct XaiOAuthState(pub Arc<RwLock<XaiOAuthManager>>);

/// 查询 xAI OAuth (SuperGrok 反代) 订阅额度的共享核心
///
/// 与 `get_codex_oauth_quota` 平行：数据走 cc-switch 自管的 xAI OAuth token，
/// 而非 Grok CLI 的 ~/.grok/auth.json。两者是同一个 OAuth client
/// （client_id 与 Grok CLI 一致），token 对 grok.com 账单端点等效，因此
/// 复用 `subscription_grok::query_grok_quota`，协议与 Grok CLI 路径完全一致。
///
/// 供两处调用：`get_xai_oauth_quota` 命令（前端 footer）与
/// `commands::provider` 的 official_subscription 分支（用量脚本/托盘路径，
/// xai_oauth 供应商的额度属绑定的 SuperGrok 账号而非所在 app 的 CLI 凭据）。
///
/// - `account_id` 未指定时回退到 `XaiOAuthManager` 的默认账号
/// - 没有任何账号时返回 `not_found`，前端 `SubscriptionQuotaView` 会静默不渲染
/// - 瞬时传输失败以 `Err` 传播（前端 reject → retry + 保留上次成功值）
pub(crate) async fn query_xai_oauth_quota_for(
    state: &XaiOAuthState,
    account_id: Option<String>,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;

    // 解析最终使用的账号 ID：显式 > 默认账号 > 无账号 (not_found)
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(SubscriptionQuota::not_found("xai_oauth"));
    };

    // 获取（必要时自动刷新）access_token
    let token = match manager.get_valid_token_for_account(&id).await {
        Ok(t) => t,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "xai_oauth",
                CredentialStatus::Expired,
                format!("xAI OAuth token unavailable: {e}"),
            ));
        }
    };

    crate::services::subscription_grok::query_grok_quota(
        &token,
        "xai_oauth",
        "Please re-login via cc-switch.",
    )
    .await
}

/// 查询 xAI OAuth (SuperGrok 反代) 订阅额度
#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_quota(
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    query_xai_oauth_quota_for(&state, account_id).await
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_models(
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
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
    let account_id = resolved.ok_or_else(|| "No usable xAI account available".to_string())?;
    let token = manager
        .get_valid_token_for_account(&account_id)
        .await
        .map_err(|error| format!("xAI OAuth token unavailable: {error}"))?;

    let response = crate::proxy::http_client::get()
        .get(format!("{XAI_API_BASE_URL}/models"))
        .bearer_auth(token)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| format!("xAI models request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("xAI models request failed: HTTP {status}"));
    }
    let payload: ModelsResponse = response
        .json()
        .await
        .map_err(|_| "xAI models response was not valid JSON".to_string())?;
    let mut models: Vec<FetchedModel> = payload
        .data
        .into_iter()
        .map(|model| FetchedModel {
            id: model.id,
            owned_by: model.owned_by,
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}
