//! GitHub Copilot request context, model resolution and API format selection.

use super::*;
use crate::proxy::copilot_auto_session::AutoSessionKey;
use crate::proxy::providers::{
    copilot_auto::{extract_prompt_summary_for_auto, RequestProtocol},
    copilot_model_map::advertised_transport,
};

/// True for the supported chat routes that may execute a Copilot Auto choice.
pub(super) fn is_copilot_auto_chat_endpoint(endpoint: &str) -> bool {
    let clean = endpoint.split('?').next().unwrap_or(endpoint);
    clean == "/chat/completions"
        || clean == "/v1/chat/completions"
        || clean == "/v1/v1/chat/completions"
        || clean == "/codex/v1/chat/completions"
}

pub(super) fn is_copilot_auto_messages_endpoint(endpoint: &str) -> bool {
    let clean = endpoint.split('?').next().unwrap_or(endpoint);
    clean == "/v1/messages"
        || clean == "/claude/v1/messages"
        || clean == "/claude-desktop/v1/messages"
}

pub(super) fn is_copilot_auto_compaction_request(
    app_type: &AppType,
    endpoint: &str,
    body: &Value,
) -> bool {
    endpoint
        .split('?')
        .next()
        .unwrap_or(endpoint)
        .ends_with("/responses/compact")
        || crate::proxy::copilot_optimizer::is_compact_request(body)
        || (matches!(app_type, AppType::Codex)
            && crate::proxy::providers::codex_compaction::is_compaction_request(body))
}

pub(super) struct CopilotRequestContext {
    pub(super) classification: crate::proxy::copilot_optimizer::CopilotClassification,
    pub(super) request_id: Option<String>,
    pub(super) interaction_id: Option<String>,
}

impl CopilotRequestContext {
    pub(super) fn apply_headers(
        &self,
        headers: &mut Vec<(http::HeaderName, http::HeaderValue)>,
        request_classification: bool,
    ) -> Result<(), ProxyError> {
        let request_id = self
            .request_id
            .as_deref()
            .map(http::HeaderValue::from_str)
            .transpose()
            .map_err(|_| ProxyError::ConfigError("Invalid Copilot request ID".to_string()))?;
        let interaction_id = self
            .interaction_id
            .as_deref()
            .map(http::HeaderValue::from_str)
            .transpose()
            .map_err(|_| ProxyError::ConfigError("Invalid Copilot interaction ID".to_string()))?;

        for (name, value) in headers.iter_mut() {
            match name.as_str() {
                "x-initiator" if request_classification => {
                    *value = http::HeaderValue::from_static(self.classification.initiator);
                }
                "x-interaction-type" if self.classification.is_subagent => {
                    *value = http::HeaderValue::from_static("conversation-subagent");
                }
                "x-request-id" | "x-agent-task-id" => {
                    if let Some(request_id) = &request_id {
                        *value = request_id.clone();
                    }
                }
                _ => {}
            }
        }

        if let Some(interaction_id) = interaction_id {
            headers.push((
                http::HeaderName::from_static("x-interaction-id"),
                interaction_id,
            ));
        }
        if self.classification.is_subagent {
            log::info!(
                "[Copilot] 子代理请求: x-initiator=agent, x-interaction-type=conversation-subagent"
            );
        }
        Ok(())
    }
}

/// Only the declared type counts for Codex: a Codex provider may point a custom
/// `base_url` at Copilot without being a managed Copilot account. Other apps also
/// accept the resolved request URL, which `Provider::is_github_copilot` never reads.
pub(super) fn is_managed_copilot_request(
    app_type: &AppType,
    provider: &Provider,
    base_url: &str,
) -> bool {
    ProviderKind::declared(provider) == Some(ProviderKind::GitHubCopilot)
        || (!matches!(app_type, AppType::Codex) && is_copilot_url(base_url))
}

pub(super) fn codex_copilot_lookup_error(model_id: &str, error: CopilotAuthError) -> ProxyError {
    let message =
        format!("Failed to resolve GitHub Copilot capabilities for model {model_id}: {error}");
    match error {
        CopilotAuthError::DeviceFlowNotStarted
        | CopilotAuthError::AuthorizationPending
        | CopilotAuthError::AccessDenied
        | CopilotAuthError::ExpiredToken
        | CopilotAuthError::ModelsUnauthorized
        | CopilotAuthError::GitHubTokenInvalid
        | CopilotAuthError::NoCopilotSubscription
        | CopilotAuthError::AccountNotFound(_) => ProxyError::AuthError(message),
        CopilotAuthError::NetworkError(_) | CopilotAuthError::CopilotTokenFetchFailed(_) => {
            ProxyError::ForwardFailed(message)
        }
        CopilotAuthError::ParseError(_)
        | CopilotAuthError::IoError(_)
        | CopilotAuthError::InvalidDomain(_) => ProxyError::ConfigError(message),
    }
}

pub(super) fn apply_codex_copilot_model(
    body: &mut Value,
    resolved: Option<ResolvedCopilotModel>,
    api_format: CodexCopilotApiFormat,
) -> Result<CopilotTransport, ProxyError> {
    let requested = match api_format {
        CodexCopilotApiFormat::Auto => "a Responses or Chat Completions endpoint",
        CodexCopilotApiFormat::OpenaiResponses => {
            "the requested Responses endpoint (codexCopilotApiFormat=openai_responses)"
        }
        CodexCopilotApiFormat::OpenaiChat => {
            "the requested Chat Completions endpoint (codexCopilotApiFormat=openai_chat)"
        }
    };
    let Some(resolved) = resolved else {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        return Err(ProxyError::ConfigError(format!(
            "GitHub Copilot model {model} is unavailable for {requested} in this provider's catalogue"
        )));
    };
    let Some(transport) = resolved.transport else {
        return Err(ProxyError::ConfigError(format!(
            "GitHub Copilot model {} does not advertise {requested}",
            resolved.id
        )));
    };
    body["model"] = Value::String(resolved.id);
    Ok(transport)
}

/// Pick the Copilot upstream protocol for a Claude-client request.
///
/// Anthropic models that advertise `/v1/messages` are passed through natively:
/// the Chat Completions bridge drops `cache_control` breakpoints and Copilot's
/// chat endpoint stops prefix caching at the first mid-conversation `system`
/// message Claude Code inserts, so every turn re-bills the whole history.
pub(super) fn copilot_api_format_for_model(
    vendor: Option<&str>,
    supported_endpoints: &[String],
) -> &'static str {
    if vendor.is_some_and(|vendor| vendor.eq_ignore_ascii_case("openai")) {
        "openai_responses"
    } else if vendor.is_some_and(|vendor| vendor.eq_ignore_ascii_case("anthropic"))
        && supported_endpoints
            .iter()
            .any(|endpoint| endpoint.trim_end_matches('/') == "/v1/messages")
    {
        "anthropic"
    } else if transport_for(supported_endpoints, CodexCopilotApiFormat::OpenaiResponses).is_some()
        && transport_for(supported_endpoints, CodexCopilotApiFormat::OpenaiChat).is_none()
    {
        // Copilot rejects these models on the chat endpoint (e.g. xAI grok)
        "openai_responses"
    } else {
        "openai_chat"
    }
}

/// Auto 选中的模型给 Claude 客户端用哪个端点。
///
/// 非 Anthropic 模型优先 `/responses`：Auto 发的 `Copilot-Session-Token` 在 Free 账号上只被
/// `/responses` 接受，同一个模型走 `/chat/completions` 会报 `no_available_model_endpoints`。
pub(super) fn copilot_auto_claude_transport(
    vendor: &str,
    supported_endpoints: &[String],
) -> Option<CopilotTransport> {
    use CopilotProtocol::{Chat, Messages, Responses};
    let preferred = if vendor.eq_ignore_ascii_case("anthropic") {
        [Messages, Chat, Responses]
    } else {
        [Responses, Chat, Messages]
    };
    advertised_transport(supported_endpoints, &preferred)
}

pub(super) struct CopilotAutoRoute {
    pub(super) key: AutoSessionKey,
    pub(super) model_id: String,
    pub(super) transport: CopilotTransport,
    pub(super) session_token: String,
    pub(super) access_token: String,
    pub(super) compaction: bool,
}

impl RequestForwarder {
    pub(super) fn copilot_auth(&self) -> Option<Arc<RwLock<CopilotAuthManager>>> {
        #[cfg(test)]
        if let Some(auth) = &self.test_copilot_auth {
            return Some(auth.clone());
        }
        self.app_handle
            .as_ref()
            .map(|app| app.state::<CopilotAuthState>().0.clone())
    }

    pub(super) async fn resolve_copilot_auto_route(
        &self,
        attempt: &Attempt<'_>,
        body: &Value,
        deadline: tokio::time::Instant,
        retry_auto: &mut bool,
    ) -> Result<CopilotAutoRoute, ProxyError> {
        let Attempt {
            app_type,
            provider,
            endpoint,
            ..
        } = *attempt;
        let auth = self.copilot_auth().ok_or_else(|| {
            ProxyError::AuthError("Copilot Auto requires managed account authentication".into())
        })?;
        let auth = auth.read().await;
        let account_id = match provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"))
        {
            Some(id) => id,
            None => auth.resolve_default_account_id().await.ok_or_else(|| {
                ProxyError::AuthError("No GitHub Copilot account is available".into())
            })?,
        };
        let (access_token, catalogue, api_base) = tokio::time::timeout_at(deadline, async {
            let token = auth
                .get_valid_token_for_account(&account_id)
                .await
                .map_err(|err| {
                    ProxyError::AuthError(format!("GitHub Copilot authentication failed: {err}"))
                })?;
            let catalogue = auth
                .fetch_catalog_for_account(&account_id)
                .await
                .map_err(|err| {
                    if matches!(err, CopilotAuthError::ModelsUnauthorized) {
                        *retry_auto = true;
                        ProxyError::UpstreamError {
                            status: 401,
                            body: Some(err.to_string()),
                        }
                    } else {
                        ProxyError::ForwardFailed(format!("Copilot catalogue lookup failed: {err}"))
                    }
                })?;
            Ok::<_, ProxyError>((token, catalogue, auth.get_api_endpoint(&account_id).await))
        })
        .await
        .map_err(|_| ProxyError::Timeout("Copilot Auto account lookup timed out".into()))??;

        // 不按目录的 `auto_discount` 拦：Free 账号没有这个计价字段，/auto 却照样可用。
        // 账号真不能用 Auto 时，/auto 自己会报错。
        let protocol = if is_copilot_auto_chat_endpoint(endpoint) {
            RequestProtocol::ChatCompletions
        } else if matches!(app_type, AppType::Codex) {
            RequestProtocol::CodexResponses
        } else {
            RequestProtocol::AnthropicMessages
        };
        let (prompt, has_image) = extract_prompt_summary_for_auto(body, protocol);
        let key = AutoSessionKey {
            app_namespace: if matches!(app_type, AppType::Codex) {
                "codex"
            } else {
                "claude"
            }
            .into(),
            provider_id: provider.id.clone(),
            account_id,
            api_base,
            credential_generation: auth.get_credential_generation(),
            client_session_id: self.session_id.clone(),
        };
        let timeout = deadline
            .saturating_duration_since(tokio::time::Instant::now())
            .min(std::time::Duration::from_secs(15));
        let snapshot = if self.session_client_provided {
            self.copilot_auto_sessions
                .get_or_resolve(&key, &access_token, &prompt, has_image, timeout)
                .await
        } else {
            self.copilot_auto_sessions
                .resolve_uncached(&key, &access_token, &prompt, has_image, timeout)
                .await
        };
        if matches!(snapshot, Err(ProxyError::UpstreamError { status: 401, .. })) {
            auth.invalidate_token_if_matches(&key.account_id, &access_token)
                .await;
            *retry_auto = true;
        }
        let snapshot = snapshot?;
        let model = catalogue.iter().find(|m| m.id == snapshot.selected_model);
        let transport = model.and_then(|m| match protocol {
            RequestProtocol::ChatCompletions => {
                transport_for(&m.supported_endpoints, CodexCopilotApiFormat::OpenaiChat)
            }
            RequestProtocol::CodexResponses => transport_for(
                &m.supported_endpoints,
                CodexCopilotApiFormat::from_meta(provider.meta.as_ref()),
            ),
            RequestProtocol::AnthropicMessages => {
                copilot_auto_claude_transport(&m.vendor, &m.supported_endpoints)
            }
        });
        let Some(transport) = transport else {
            self.copilot_auto_sessions
                .invalidate_if_token_matches(&key, &snapshot.session_token)
                .await;
            let reason = if model.is_none() {
                "is absent from the account catalogue"
            } else {
                "has no advertised endpoint compatible with the request"
            };
            return Err(ProxyError::ConfigError(format!(
                "Copilot Auto selected model '{}' {reason}",
                snapshot.selected_model
            )));
        };
        Ok(CopilotAutoRoute {
            key,
            model_id: snapshot.selected_model,
            transport,
            session_token: snapshot.session_token,
            access_token,
            compaction: is_copilot_auto_compaction_request(app_type, endpoint, body),
        })
    }

    pub(super) fn codex_copilot_request_context(
        &self,
        is_codex_copilot_responses: bool,
        body: &Value,
    ) -> Option<CopilotRequestContext> {
        if !is_codex_copilot_responses || !self.copilot_optimizer_config.enabled {
            return None;
        }
        Some(CopilotRequestContext {
            classification: crate::proxy::copilot_optimizer::classify_responses_request(body),
            // Keep Codex request IDs per-request; only the interaction ID groups a session.
            request_id: None,
            interaction_id: self
                .session_client_provided
                .then(|| {
                    crate::proxy::copilot_optimizer::deterministic_interaction_id(&self.session_id)
                })
                .flatten(),
        })
    }

    pub(super) async fn resolve_claude_api_format(
        &self,
        provider: &Provider,
        body: &Value,
        is_copilot: bool,
    ) -> String {
        if !is_copilot {
            return crate::proxy::providers::get_claude_api_format(provider).to_string();
        }

        self.resolve_copilot_api_format(provider, body)
            .await
            .to_string()
    }

    pub(super) async fn resolve_copilot_api_format(
        &self,
        provider: &Provider,
        body: &Value,
    ) -> &'static str {
        let model = body.get("model").and_then(|value| value.as_str());
        let record = match model {
            Some(model_id) => self.copilot_model_record(provider, model_id).await,
            None => None,
        };
        copilot_api_format_for_model(
            record.as_ref().map(|model| model.vendor.as_str()),
            record
                .as_ref()
                .map(|model| model.supported_endpoints.as_slice())
                .unwrap_or_default(),
        )
    }

    /// 用 Copilot live `/models` 列表确认 model ID 真实可用，找不到时按 family 降级。
    /// 命中缓存后是同步的；首次请求或 5 min 缓存过期后会触发一次 HTTP。
    pub(super) async fn apply_copilot_live_model_resolution(
        &self,
        provider: &Provider,
        body: &mut serde_json::Value,
    ) {
        let Some(model_id) = body.get("model").and_then(|v| v.as_str()) else {
            return;
        };
        let model_id = model_id.to_string();

        let Some(auth) = self.copilot_auth() else {
            return;
        };
        let copilot_auth = auth.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let models_result = match account_id.as_deref() {
            Some(id) => copilot_auth.fetch_models_for_account(id).await,
            None => copilot_auth.fetch_models().await,
        };

        let models = match models_result {
            Ok(m) => m,
            Err(err) => {
                log::debug!("[Copilot] live model list unavailable, skip resolution: {err}");
                return;
            }
        };

        if let Some(resolved) =
            crate::proxy::providers::copilot_model_map::resolve_against_models(&model_id, &models)
        {
            log::info!("[Copilot] live-model resolve: {model_id} → {resolved}");
            body["model"] = serde_json::Value::String(resolved);
        }
    }

    pub(super) async fn resolve_codex_copilot_model(
        &self,
        provider: &Provider,
        body: &Value,
        api_format: CodexCopilotApiFormat,
    ) -> Result<Option<ResolvedCopilotModel>, ProxyError> {
        let model_id = body
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                ProxyError::InvalidRequest(
                    "Codex Copilot requests require a non-empty string model".to_string(),
                )
            })?;
        let auth = self.copilot_auth().ok_or_else(|| {
            ProxyError::ConfigError(format!(
                "GitHub Copilot capabilities for model {model_id} cannot be resolved: AppHandle unavailable"
            ))
        })?;

        let copilot_auth = auth.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("github_copilot"));
        let resolved = match account_id.as_deref() {
            Some(id) => {
                copilot_auth
                    .resolve_model_for_account(id, model_id, api_format)
                    .await
            }
            None => copilot_auth.resolve_model(model_id, api_format).await,
        };

        resolved.map_err(|error| codex_copilot_lookup_error(model_id, error))
    }

    pub(super) async fn copilot_model_record(
        &self,
        provider: &Provider,
        model_id: &str,
    ) -> Option<crate::proxy::providers::copilot_auth::CopilotModel> {
        let Some(auth) = self.copilot_auth() else {
            log::debug!("[Copilot] AppHandle unavailable, fallback to chat/completions");
            return None;
        };

        let copilot_auth = auth.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let record_result = match account_id.as_deref() {
            Some(id) => {
                copilot_auth
                    .get_model_record_for_account(id, model_id)
                    .await
            }
            None => copilot_auth.get_model_record(model_id).await,
        };

        match record_result {
            Ok(Some(record)) => Some(record),
            Ok(None) => {
                log::debug!(
                    "[Copilot] Model vendor unavailable for {model_id}, fallback to chat/completions"
                );
                None
            }
            Err(err) => {
                log::warn!(
                    "[Copilot] Failed to resolve model vendor for {model_id}, fallback to chat/completions: {err}"
                );
                None
            }
        }
    }
}
