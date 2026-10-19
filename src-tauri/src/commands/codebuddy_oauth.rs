//! CodeBuddy OAuth 授权中心状态与额度命令。
//!
//! 登录/轮询/账号管理复用 `commands::auth` 的通用命令（`authProvider: "codebuddy_oauth"`），
//! 这里只提供 CodeBuddy 专属的额度与模型目录命令。

use crate::proxy::providers::codebuddy_oauth_auth::{CodebuddyOAuthManager, CodebuddyQuota};
use crate::services::model_fetch::FetchedModel;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

pub struct CodebuddyOAuthState(pub Arc<RwLock<CodebuddyOAuthManager>>);

/// 查询 CodeBuddy OAuth 账号额度（`accountId` 缺省时用默认账号）。
///
/// 无账号、token 失效、上游失败等都以 `success: false` + 中文 `error` 返回，
/// 便于前端在授权中心行内直接展示失败原因。命令本身始终 `Ok`（Tauri 的
/// `State` 参数要求返回 `Result`），前端 `invoke` 拿到的就是 `CodebuddyQuota`。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codebuddy_oauth_quota(
    account_id: Option<String>,
    state: State<'_, CodebuddyOAuthState>,
) -> Result<CodebuddyQuota, String> {
    Ok(state
        .0
        .read()
        .await
        .query_quota(account_id.as_deref())
        .await)
}

/// 拉取 CodeBuddy 账号可用的 CLI 模型目录（`GET /v3/config`）。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codebuddy_oauth_models(
    account_id: Option<String>,
    state: State<'_, CodebuddyOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    let models = manager
        .fetch_models(account_id.as_deref())
        .await
        .map_err(|error| error.to_string())?;
    Ok(models
        .into_iter()
        .map(|model| FetchedModel {
            id: model.id,
            owned_by: model.vendor,
        })
        .collect())
}
