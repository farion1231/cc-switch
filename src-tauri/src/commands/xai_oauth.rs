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

/// 要用的账号：显式指定的 > 默认账号 > 没有
async fn resolve_account_id(
    manager: &XaiOAuthManager,
    account_id: Option<String>,
) -> Option<String> {
    match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    }
}

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
/// - `with_plan`：顺带问套餐（多一个请求），只有前端要显示时才开，托盘轮询不问
pub(crate) async fn query_xai_oauth_quota_for(
    state: &XaiOAuthState,
    account_id: Option<String>,
    with_plan: bool,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;

    let Some(id) = resolve_account_id(&manager, account_id).await else {
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

    // 套餐（SuperGrok / Heavy）另问 Grok CLI 的设置接口，和额度并行；问不到只是不显示
    let (quota, plan) = tokio::join!(
        crate::services::subscription_grok::query_grok_quota(
            &token,
            "xai_oauth",
            "Please re-login via cc-switch.",
        ),
        async {
            if with_plan {
                crate::services::subscription_grok::query_grok_plan(&token).await
            } else {
                None
            }
        },
    );
    let mut quota = quota?;
    if quota.success {
        quota.plan_name = plan;
    }
    Ok(quota)
}

/// Grok Build 官方卡的订阅额度：先读 Grok CLI 凭据，查不到时用授权中心的同一账号兜底
///
/// Grok CLI 只在自己运行时刷新 ~/.grok/auth.json 的访问令牌（约 6 小时过期），
/// 几天不用 Grok 卡片就会一直「令牌待刷新」。授权中心的 xAI 账号与 Grok CLI 是
/// 同一个 OAuth client，令牌由 cc-switch 自己刷新；只要账号 id 与 Grok CLI 登录的
/// `user_id` 一致，查到的就是同一份订阅额度。只读，不改 Grok CLI 的凭据文件。
/// 瞬时失败（`Err`）不兜底，保持前端 retry + 保留上次成功值的语义。
pub(crate) async fn query_grokbuild_quota_with_managed_fallback(
    state: Option<&XaiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    let primary = crate::services::subscription_grok::get_grok_subscription_quota().await?;
    if primary.success {
        return Ok(primary);
    }
    let (Some(state), Some(live)) = (state, crate::grok_auth::read_live_login_from_disk()) else {
        return Ok(primary);
    };
    // 授权中心没有这个号或要重新登录时查不成功，照样回到 Grok CLI 凭据的结果
    match query_xai_oauth_quota_for(state, Some(live.user_id), false).await {
        Ok(mut quota) if quota.success => {
            quota.tool = "grokbuild".to_string();
            Ok(quota)
        }
        _ => Ok(primary),
    }
}

/// 查一个账号的额度，按账号存进托盘缓存（绑了账号的 Grok 官方卡在托盘上读它）
pub(crate) async fn query_and_cache_xai_account(
    app: &tauri::AppHandle,
    app_state: &crate::store::AppState,
    state: &XaiOAuthState,
    account_id: Option<String>,
    with_plan: bool,
) -> Result<SubscriptionQuota, String> {
    let resolved = resolve_account_id(&*state.0.read().await, account_id).await;
    let quota = query_xai_oauth_quota_for(state, resolved.clone(), with_plan).await?;
    if let Some(account_id) = resolved {
        app_state
            .usage_cache
            .put_xai_account(account_id, quota.clone());
        crate::tray::schedule_tray_refresh(app);
    }
    Ok(quota)
}

/// 查询 xAI OAuth (SuperGrok 反代) 订阅额度
#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_quota(
    app: tauri::AppHandle,
    app_state: State<'_, crate::store::AppState>,
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    query_and_cache_xai_account(&app, &app_state, &state, account_id, true).await
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
    let account_id = resolve_account_id(&manager, account_id)
        .await
        .ok_or_else(|| "No usable xAI account available".to_string())?;
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
