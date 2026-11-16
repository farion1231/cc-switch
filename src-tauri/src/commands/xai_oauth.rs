//! xAI OAuth state and xAI-specific commands.

use crate::proxy::providers::xai_oauth_auth::{XaiOAuthError, XaiOAuthManager};
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
/// - `with_plan`：顺带问套餐（多一个请求），只有前端要显示时才开，托盘轮询不问
pub(crate) async fn query_xai_oauth_quota_for(
    state: &XaiOAuthState,
    account_id: Option<String>,
    with_plan: bool,
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
        // 网络 / 刷新端点暂时失败不代表登录失效：以 Err 传播，前端 retry 并保留
        // 上次成功值；只有确定的凭据问题才标成过期。
        Err(e) if is_transient_token_error(&e) => {
            return Err(format!("xAI OAuth token unavailable: {e}"));
        }
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

/// 查询 xAI OAuth (SuperGrok 反代) 订阅额度
#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_quota(
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    query_xai_oauth_quota_for(&state, account_id, true).await
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

fn is_transient_token_error(error: &XaiOAuthError) -> bool {
    matches!(
        error,
        XaiOAuthError::NetworkError(_)
            | XaiOAuthError::TokenFetchFailed(_)
            | XaiOAuthError::ParseError(_)
            | XaiOAuthError::IoError(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn serve_single_http_response(
        status_line: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{addr}/oauth2/token")
    }

    #[test]
    fn transient_token_errors_are_classified_separately_from_auth_failures() {
        assert!(is_transient_token_error(&XaiOAuthError::NetworkError(
            "timeout".to_string()
        )));
        assert!(is_transient_token_error(&XaiOAuthError::TokenFetchFailed(
            "HTTP 503 Service Unavailable".to_string()
        )));
        assert!(is_transient_token_error(&XaiOAuthError::ParseError(
            "OAuth 响应不是有效 JSON".to_string()
        )));
        assert!(is_transient_token_error(&XaiOAuthError::IoError(
            "disk full".to_string()
        )));

        assert!(!is_transient_token_error(
            &XaiOAuthError::RefreshTokenInvalid
        ));
        assert!(!is_transient_token_error(&XaiOAuthError::ReauthRequired(
            "acct-1".to_string()
        )));
        assert!(!is_transient_token_error(&XaiOAuthError::AccountNotFound(
            "acct-1".to_string()
        )));
        assert!(!is_transient_token_error(&XaiOAuthError::AccessDenied));
        assert!(!is_transient_token_error(&XaiOAuthError::ExpiredToken));
        assert!(!is_transient_token_error(
            &XaiOAuthError::AuthorizationPending
        ));
    }

    #[tokio::test]
    async fn query_xai_oauth_quota_propagates_transient_refresh_errors() {
        // 1. Transport failure (connection refused)
        let closed_endpoint = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener);
            format!("http://{addr}/oauth2/token")
        };
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        manager
            .seed_test_account_with_token_endpoint("acct-net", "refresh-1", closed_endpoint)
            .await;
        let state = XaiOAuthState(Arc::new(RwLock::new(manager)));

        let net_err = query_xai_oauth_quota_for(&state, None, false)
            .await
            .expect_err("connection failure during refresh must propagate as Err");
        assert!(net_err.contains("xAI OAuth token unavailable"), "{net_err}");
        assert!(
            state.0.read().await.get_status().await.authenticated,
            "transient transport failure must not mark account requires_reauth"
        );

        // 2. Upstream HTTP 503 JSON response
        let json_503 = serve_single_http_response(
            "503 Service Unavailable",
            "application/json",
            r#"{"error":"temporarily_unavailable"}"#,
        )
        .await;
        state
            .0
            .read()
            .await
            .seed_test_account_with_token_endpoint("acct-503", "refresh-2", json_503)
            .await;
        let err_503 = query_xai_oauth_quota_for(&state, Some("acct-503".to_string()), false)
            .await
            .expect_err("HTTP 503 during refresh must propagate as Err");
        assert!(err_503.contains("xAI OAuth token unavailable"), "{err_503}");

        // 3. Upstream HTTP 502 HTML gateway page
        let html_502 = serve_single_http_response(
            "502 Bad Gateway",
            "text/html",
            "<html><body>502 Bad Gateway</body></html>",
        )
        .await;
        state
            .0
            .read()
            .await
            .seed_test_account_with_token_endpoint("acct-502", "refresh-3", html_502)
            .await;
        let err_502 = query_xai_oauth_quota_for(&state, Some("acct-502".to_string()), false)
            .await
            .expect_err("HTTP 502 HTML gateway error during refresh must propagate as Err");
        assert!(err_502.contains("xAI OAuth token unavailable"), "{err_502}");
    }

    #[tokio::test]
    async fn query_xai_oauth_quota_marks_auth_failures_expired_and_empty_not_found() {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = XaiOAuthManager::new(data_dir.path().to_path_buf());
        let state = XaiOAuthState(Arc::new(RwLock::new(manager)));

        let empty = query_xai_oauth_quota_for(&state, None, false)
            .await
            .expect("missing default account returns Ok(not_found)");
        assert!(matches!(
            empty.credential_status,
            CredentialStatus::NotFound
        ));

        let missing = query_xai_oauth_quota_for(&state, Some("unknown-id".to_string()), false)
            .await
            .expect("explicit missing account returns Ok(Expired)");
        assert!(!missing.success);
        assert!(matches!(
            missing.credential_status,
            CredentialStatus::Expired
        ));

        let unauthorized = serve_single_http_response(
            "401 Unauthorized",
            "application/json",
            r#"{"error":"invalid_grant"}"#,
        )
        .await;
        state
            .0
            .read()
            .await
            .seed_test_account_with_token_endpoint("acct-expired", "bad-refresh", unauthorized)
            .await;

        let expired = query_xai_oauth_quota_for(&state, Some("acct-expired".to_string()), false)
            .await
            .expect("HTTP 401 refresh failure returns Ok(Expired)");
        assert!(!expired.success);
        assert!(matches!(
            expired.credential_status,
            CredentialStatus::Expired
        ));
        assert!(!state.0.read().await.get_status().await.authenticated);
    }
}
