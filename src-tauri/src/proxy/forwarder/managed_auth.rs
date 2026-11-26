//! Live tokens for managed-account providers (GitHub Copilot, Codex OAuth, xAI OAuth).

use super::*;

impl RequestForwarder {
    /// Live token for the GitHub Copilot account bound in `provider.meta`
    /// (the first signed-in account when none is bound).
    pub(super) async fn copilot_access_token(
        &self,
        provider: &Provider,
    ) -> Result<String, ProxyError> {
        let Some(auth) = self.copilot_auth() else {
            log::error!("[Copilot] AppHandle 不可用");
            return Err(ProxyError::AuthError(
                "GitHub Copilot 认证不可用（无 AppHandle）".to_string(),
            ));
        };
        let copilot_auth = auth.read().await;

        // 从 provider.meta 获取关联的 GitHub 账号 ID（多账号支持）
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        // 根据账号 ID 获取对应 token（向后兼容：无账号 ID 时使用第一个账号）
        let token_result = match &account_id {
            Some(id) => {
                log::debug!("[Copilot] 使用指定账号 {id} 获取 token");
                copilot_auth.get_valid_token_for_account(id).await
            }
            None => {
                log::debug!("[Copilot] 使用默认账号获取 token");
                copilot_auth.get_valid_token().await
            }
        };

        match token_result {
            Ok(token) => {
                log::debug!(
                    "[Copilot] 成功获取 Copilot token (account={})",
                    account_id.as_deref().unwrap_or("default")
                );
                Ok(token)
            }
            Err(e) => {
                log::error!(
                    "[Copilot] 获取 Copilot token 失败 (account={}): {e}",
                    account_id.as_deref().unwrap_or("default")
                );
                Err(ProxyError::AuthError(format!(
                    "GitHub Copilot 认证失败: {e}"
                )))
            }
        }
    }

    /// Live access token for the ChatGPT account bound in `provider.meta`
    /// (the default account when none is bound), plus the upstream workspace
    /// ID that goes into `chatgpt-account-id`.
    pub(super) async fn codex_oauth_access_token(
        &self,
        provider: &Provider,
    ) -> Result<(String, Option<String>), ProxyError> {
        let Some(app_handle) = &self.app_handle else {
            log::error!("[CodexOAuth] AppHandle 不可用");
            return Err(ProxyError::AuthError(
                "Codex OAuth 认证不可用（无 AppHandle）".to_string(),
            ));
        };
        let codex_state = app_handle.state::<CodexOAuthState>();
        let codex_auth = &codex_state.0;

        // 从 provider.meta 获取关联的 ChatGPT 账号 ID
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("codex_oauth"));

        let resolved_account_id = match account_id {
            Some(id) => Some(id),
            None => codex_auth.default_account_id().await,
        };

        let token_result = match &resolved_account_id {
            Some(id) => {
                log::debug!("[CodexOAuth] 使用指定账号 {id} 获取 token");
                codex_auth.get_valid_token_for_account(id).await
            }
            None => {
                return Err(ProxyError::AuthError(
                    "Codex OAuth 认证失败: 无可用的 ChatGPT 账号".to_string(),
                ));
            }
        };

        match token_result {
            Ok(token) => {
                // 本地账号 ID 只用于绑定；请求头必须使用上游 workspace ID。
                let codex_oauth_account_id = match resolved_account_id.as_deref() {
                    Some(id) => Some(
                        codex_auth
                            .chatgpt_account_id_for_account(id)
                            .await
                            .map_err(|e| {
                                ProxyError::AuthError(format!("Codex OAuth 账号解析失败: {e}"))
                            })?,
                    ),
                    None => None,
                };
                log::debug!(
                    "[CodexOAuth] 成功获取 access_token (account={})",
                    codex_oauth_account_id.as_deref().unwrap_or("default")
                );
                Ok((token, codex_oauth_account_id))
            }
            Err(e) => {
                log::error!("[CodexOAuth] 获取 access_token 失败: {e}");
                Err(ProxyError::AuthError(format!("Codex OAuth 认证失败: {e}")))
            }
        }
    }

    /// Live access token for the xAI account bound in `provider.meta` (the
    /// default account when none is bound). Invalid refresh credentials are
    /// persisted as requiring re-authentication by the manager.
    pub(super) async fn xai_oauth_access_token(
        &self,
        provider: &Provider,
    ) -> Result<String, ProxyError> {
        let Some(app_handle) = &self.app_handle else {
            return Err(ProxyError::AuthError(
                "xAI OAuth 认证不可用（无 AppHandle）".to_string(),
            ));
        };
        let xai_state = app_handle.state::<XaiOAuthState>();
        let xai_auth: tokio::sync::RwLockReadGuard<'_, XaiOAuthManager> = xai_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("xai_oauth"));
        let token_result = match &account_id {
            Some(id) => xai_auth.get_valid_token_for_account(id).await,
            None => xai_auth.get_valid_token().await,
        };
        match token_result {
            Ok(token) => {
                log::debug!(
                    "[XaiOAuth] 成功获取 access_token (account={})",
                    account_id.as_deref().unwrap_or("default")
                );
                Ok(token)
            }
            Err(error) => {
                log::error!("[XaiOAuth] 获取 access_token 失败: {error}");
                Err(ProxyError::AuthError(format!(
                    "xAI OAuth 认证失败: {error}"
                )))
            }
        }
    }
}
