//! 请求处理器
//!
//! 处理各种API端点的HTTP请求
//!
//! 重构后的结构：
//! - 通用逻辑提取到 `handler_context` 和 `response_processor` 模块
//! - 各 handler 只保留独特的业务逻辑
//! - Claude 的格式转换逻辑保留在此文件（用于 OpenRouter 旧接口回退）

use super::{
    error_mapper::{get_error_message, map_proxy_error_to_status},
    external_openai_api::{
        self, ExternalOpenAiApiAuthError, ExternalOpenAiApiBackendType, ExternalOpenAiApiProfile,
    },
    forwarder::ActiveConnectionGuard,
    handler_config::{
        claude_stream_usage_event_filter, codex_stream_usage_event_filter, CLAUDE_PARSER_CONFIG,
        CODEX_PARSER_CONFIG, GEMINI_PARSER_CONFIG, OPENAI_PARSER_CONFIG,
    },
    handler_context::RequestContext,
    providers::{
        codex_chat_history::record_responses_sse_stream, get_adapter, get_claude_api_format,
        openai_compat, streaming::create_anthropic_sse_stream,
        streaming_codex_chat::create_responses_sse_stream_from_chat_with_context,
        streaming_gemini::create_anthropic_sse_stream_from_gemini,
        streaming_responses::create_anthropic_sse_stream_from_responses, transform,
        transform_codex_chat, transform_gemini, transform_responses,
    },
    response_processor::{
        create_logged_passthrough_stream, process_response, read_decoded_body,
        strip_entity_headers_for_rebuilt_body, strip_hop_by_hop_response_headers,
        usage_logging_enabled, SseUsageCollector,
    },
    server::ProxyState,
    sse::{strip_sse_field, take_sse_block},
    types::*,
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::app_config::AppType;
use crate::database::PRICING_SOURCE_REQUEST;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use http_body_util::BodyExt;
use serde_json::{json, Value};

// ============================================================================
// 健康检查和状态查询（简单端点）
// ============================================================================

/// 健康检查
pub async fn health_check() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// 获取服务状态
pub async fn get_status(State(state): State<ProxyState>) -> Result<Json<ProxyStatus>, ProxyError> {
    let status = state.status.read().await.clone();
    Ok(Json(status))
}

/// GET /v1/models — Codex model list (reachability check)
///
/// Codex CLI probes this endpoint at startup and deserializes the response as a
/// catalog with a top-level `models` field.  Return the cc-switch–managed model
/// catalog file directly so the format always matches what the current version
/// of Codex expects.
///
/// Only serves the catalog when the live config.toml still references the
/// cc-switch–owned `model_catalog_json`, using the same path ownership rules as
/// Codex live-setting import.
pub async fn handle_models(
    State(state): State<ProxyState>,
    headers: HeaderMap,
) -> Result<axum::response::Response, ProxyError> {
    let generated_path = crate::codex_config::get_codex_model_catalog_path();
    let active_catalog_path = match crate::codex_config::read_codex_config_text() {
        Ok(config_text) => {
            crate::codex_config::resolve_cc_switch_catalog_path(&config_text, &generated_path)
        }
        Err(_) => None,
    };

    if should_handle_as_codex_client(&headers) {
        let catalog = if let Some(catalog_path) =
            active_catalog_path.as_ref().filter(|path| path.exists())
        {
            let text = std::fs::read_to_string(catalog_path).unwrap_or_default();
            serde_json::from_str(&text).unwrap_or(json!({"models": []}))
        } else {
            if active_catalog_path.is_none() {
                log::debug!(
                    "[models] stale guard: catalog not served (model_catalog_json not set to cc-switch catalog)"
                );
            }
            json!({"models": []})
        };
        return Ok(Json(catalog).into_response());
    }

    let external_api_profile = match external_openai_api::validate_request(&state.db, &headers) {
        Ok(profile) => profile,
        Err(err) => return Ok(external_openai_api_auth_error_response(err)),
    };
    let models = external_openai_api_models_response(&state, &external_api_profile)?;

    Ok(Json(models).into_response())
}

/// 判断 `/v1/models` 调用方是否是 Codex 自身。
fn is_codex_model_catalog_client(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|user_agent| user_agent.to_ascii_lowercase().contains("codex"))
        .unwrap_or(false)
}

/// 判断请求是否应按 Codex 自身客户端处理。
///
/// External API key 优先级高于 User-Agent，避免第三方 agent 名称中包含
/// `codex` 时绕过 External API profile。
fn should_handle_as_codex_client(headers: &HeaderMap) -> bool {
    is_codex_model_catalog_client(headers) && !external_openai_api::has_external_api_key(headers)
}

/// 告诉 Codex 客户端本地代理不支持 Responses WebSocket。
pub async fn handle_responses_websocket_fallback() -> axum::response::Response {
    (
        StatusCode::UPGRADE_REQUIRED,
        Json(json!({
            "error": {
                "message": "CC Switch local proxy does not expose Responses WebSocket; use HTTP Responses instead.",
                "type": "cc_switch_websocket_not_supported",
                "code": "responses_websocket_not_supported"
            }
        })),
    )
        .into_response()
}

// ============================================================================
// Claude API 处理器（包含格式转换逻辑）
// ============================================================================

/// 处理 /v1/messages 请求（Claude API）
///
/// Claude 处理器包含独特的格式转换逻辑：
/// - 过去用于 OpenRouter 的 OpenAI Chat Completions 兼容接口（Anthropic ↔ OpenAI 转换）
/// - 现在 OpenRouter 已推出 Claude Code 兼容接口，默认不再启用该转换（逻辑保留以备回退）
pub async fn handle_messages(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_messages_for_app(state, request, AppType::Claude, "Claude", "claude", None).await
}

pub async fn handle_claude_desktop_messages(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, request.headers())?;
    handle_messages_for_app(
        state,
        request,
        AppType::ClaudeDesktop,
        "Claude Desktop",
        "claude-desktop",
        Some("/claude-desktop"),
    )
    .await
}

pub async fn handle_claude_desktop_models(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, &headers)?;
    let providers = state
        .provider_router
        .select_providers("claude-desktop")
        .await
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    let provider = providers.first().ok_or(ProxyError::NoAvailableProvider)?;
    let response = crate::claude_desktop_config::model_list_response(provider)
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    Ok(Json(response))
}

async fn handle_messages_for_app(
    state: ProxyState,
    request: axum::extract::Request,
    app_type: AppType,
    tag: &'static str,
    app_type_str: &'static str,
    strip_prefix: Option<&'static str>,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, app_type.clone(), tag, app_type_str).await?;

    let raw_endpoint = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str())
        .unwrap_or(uri.path());
    let endpoint = strip_prefix
        .and_then(|prefix| raw_endpoint.strip_prefix(prefix))
        .unwrap_or(raw_endpoint);

    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // 转发请求
    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &app_type,
            method,
            endpoint,
            body.clone(),
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return Err(err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.provider = result.provider;
    let api_format = result
        .claude_api_format
        .as_deref()
        .unwrap_or_else(|| get_claude_api_format(&ctx.provider))
        .to_string();
    let response = result.response;

    // 检查是否需要格式转换（OpenRouter 等中转服务）
    let adapter = get_adapter(&app_type);
    let needs_transform = adapter.needs_transform(&ctx.provider);

    // Claude 特有：格式转换处理
    if needs_transform {
        return handle_claude_transform(
            response,
            &ctx,
            &state,
            &body,
            is_stream,
            &api_format,
            connection_guard,
        )
        .await;
    }

    // 通用响应处理（透传模式）
    process_response(
        response,
        &ctx,
        &state,
        &CLAUDE_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

fn validate_claude_desktop_gateway_auth(
    state: &ProxyState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProxyError> {
    let expected = crate::claude_desktop_config::get_or_create_gateway_token(state.db.as_ref())
        .map_err(|e| ProxyError::AuthError(e.to_string()))?;
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(ProxyError::AuthError(
            "Claude Desktop gateway 缺少 Authorization 头".to_string(),
        ));
    };
    let value = value
        .to_str()
        .map_err(|_| ProxyError::AuthError("Authorization 头格式无效".to_string()))?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or("")
        .trim();
    if token != expected {
        return Err(ProxyError::AuthError(
            "Claude Desktop gateway token 无效".to_string(),
        ));
    }
    Ok(())
}

/// Claude 格式转换处理（独有逻辑）
///
/// 支持 OpenAI Chat Completions 和 Responses API 两种格式的转换
async fn handle_claude_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    original_body: &Value,
    is_stream: bool,
    api_format: &str,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();
    let is_codex_oauth = ctx
        .provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("codex_oauth");
    // Codex OAuth 会把 openai_responses 响应强制升级为 SSE，即使客户端发的是 stream:false。
    // should_use_claude_transform_streaming 默认会把这个组合路由到流式转换器——虽然能避免
    // JSON parse 报 422，但会让非流客户端收到 text/event-stream，违反 Anthropic 非流语义。
    // 这里为这个特定组合打开 override：把上游 SSE 聚合成 Anthropic JSON 回给客户端，其它
    // 场景（任意上游 is_sse、非 Codex OAuth 等）仍沿用原有流式兜底。
    let aggregate_codex_oauth_responses_sse =
        !is_stream && is_codex_oauth && api_format == "openai_responses";
    let use_streaming = if aggregate_codex_oauth_responses_sse {
        false
    } else {
        should_use_claude_transform_streaming(
            is_stream,
            response.is_sse(),
            api_format,
            is_codex_oauth,
        )
    };
    let tool_schema_hints = transform_gemini::extract_anthropic_tool_schema_hints(original_body);
    let tool_schema_hints = (!tool_schema_hints.is_empty()).then_some(tool_schema_hints);

    if use_streaming {
        // 根据 api_format 选择流式转换器
        let stream = response.bytes_stream();
        let sse_stream: Box<
            dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin,
        > = if api_format == "openai_responses" {
            Box::new(Box::pin(create_anthropic_sse_stream_from_responses(stream)))
        } else if api_format == "gemini_native" {
            Box::new(Box::pin(create_anthropic_sse_stream_from_gemini(
                stream,
                Some(state.gemini_shadow.clone()),
                Some(ctx.provider.id.clone()),
                Some(ctx.session_id.clone()),
                tool_schema_hints.clone(),
            )))
        } else {
            Box::new(Box::pin(create_anthropic_sse_stream(stream)))
        };

        // 创建使用量收集器；关闭 usage logging 时不要再解析转换后的 SSE。
        let usage_collector = if usage_logging_enabled(state) {
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let request_model = ctx.request_model.clone();
            let status_code = status.as_u16();
            let start_time = ctx.start_time;
            let session_id = ctx.session_id.clone();

            Some(SseUsageCollector::new(
                start_time,
                Some(claude_stream_usage_event_filter),
                move |events, first_token_ms| {
                    if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                        let model = usage.model.clone().unwrap_or(request_model.clone());
                        let latency_ms = start_time.elapsed().as_millis() as u64;
                        let state = state.clone();
                        let provider_id = provider_id.clone();
                        let session_id = session_id.clone();
                        let request_model = request_model.clone();

                        tokio::spawn(async move {
                            log_usage(
                                &state,
                                &provider_id,
                                "claude",
                                &model,
                                &request_model,
                                usage,
                                latency_ms,
                                first_token_ms,
                                true,
                                status_code,
                                Some(session_id),
                            )
                            .await;
                        });
                    } else {
                        log::debug!("[Claude] OpenRouter 流式响应缺少 usage 统计，跳过消费记录");
                    }
                },
            ))
        } else {
            None
        };

        // 获取流式超时配置
        let timeout_config = ctx.streaming_timeout_config();

        let logged_stream = create_logged_passthrough_stream(
            sse_stream,
            "Claude/OpenRouter",
            usage_collector,
            timeout_config,
            connection_guard,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );

        let body = axum::body::Body::from_stream(logged_stream);
        return Ok((headers, body).into_response());
    }

    // 非流式响应转换 (OpenAI/Responses → Anthropic)
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;

    let body_str = String::from_utf8_lossy(&body_bytes);

    let upstream_response: Value = if aggregate_codex_oauth_responses_sse {
        responses_sse_to_response_value(&body_str)?
    } else {
        serde_json::from_slice(&body_bytes).map_err(|e| {
            log::error!("[Claude] 解析上游响应失败: {e}, body: {body_str}");
            ProxyError::TransformError(format!("Failed to parse upstream response: {e}"))
        })?
    };

    // 根据 api_format 选择非流式转换器
    let anthropic_response = if api_format == "openai_responses" {
        transform_responses::responses_to_anthropic(upstream_response)
    } else if api_format == "gemini_native" {
        transform_gemini::gemini_to_anthropic_with_shadow_and_hints(
            upstream_response,
            Some(state.gemini_shadow.as_ref()),
            Some(&ctx.provider.id),
            Some(&ctx.session_id),
            tool_schema_hints.as_ref(),
        )
    } else {
        transform::openai_to_anthropic(upstream_response)
    }
    .map_err(|e| {
        log::error!("[Claude] 转换响应失败: {e}");
        e
    })?;

    // 记录使用量
    if let Some(usage) = TokenUsage::from_claude_response(&anthropic_response) {
        let model = anthropic_response
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let latency_ms = ctx.latency_ms();

        let request_model = ctx.request_model.clone();
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = model.to_string();
            let session_id = ctx.session_id.clone();
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    "claude",
                    &model,
                    &request_model,
                    usage,
                    latency_ms,
                    None,
                    false,
                    status.as_u16(),
                    Some(session_id),
                )
                .await;
            }
        });
    }

    // 构建响应
    let mut builder = axum::response::Response::builder().status(status);
    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    // Builder::header 是 append 语义；不先 remove 会和上游 Content-Type 双发。
    response_headers.remove(axum::http::header::CONTENT_TYPE);

    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }

    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let response_body = serde_json::to_vec(&anthropic_response).map_err(|e| {
        log::error!("[Claude] 序列化响应失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize response: {e}"))
    })?;

    let body = axum::body::Body::from(response_body);
    builder.body(body).map_err(|e| {
        log::error!("[Claude] 构建响应失败: {e}");
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

fn endpoint_with_query(uri: &axum::http::Uri, endpoint: &str) -> String {
    match uri.query() {
        Some(query) => format!("{endpoint}?{query}"),
        None => endpoint.to_string(),
    }
}

// ============================================================================
// Codex API 处理器
// ============================================================================

/// 处理 /v1/chat/completions 请求（OpenAI Chat Completions API - Codex CLI）
pub async fn handle_chat_completions(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let is_external_openai_client = !should_handle_as_codex_client(&headers);
    let mut ctx = if is_external_openai_client {
        let external_api_profile = match external_openai_api::validate_request(&state.db, &headers)
        {
            Ok(profile) => profile,
            Err(err) => return Ok(external_openai_api_auth_error_response(err)),
        };
        let provider = match resolve_external_openai_compatible_provider(
            &state,
            &body,
            &external_api_profile,
        )? {
            Some(provider) => provider,
            None => {
                return Ok(external_openai_api_route_error_response(
                    &request_model_from_body(&body),
                ))
            }
        };
        RequestContext::new_with_provider(
            &state,
            &body,
            &headers,
            AppType::Codex,
            "Codex",
            "codex",
            provider,
        )
        .await?
    } else {
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?
    };

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_chat_to_responses = ctx.provider.is_codex_oauth();
    let endpoint = if codex_chat_to_responses {
        endpoint_with_query(&uri, "/responses")
    } else {
        endpoint_with_query(&uri, "/chat/completions")
    };
    let outbound_body = if codex_chat_to_responses {
        openai_compat::chat_completions_request_to_codex_responses(body)?
    } else {
        body
    };

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            method,
            &endpoint,
            outbound_body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            if codex_chat_to_responses {
                return build_chat_proxy_error_response(&ctx, &endpoint, &err.error);
            }
            return build_codex_proxy_error_response(&ctx, &endpoint, &err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.provider = result.provider;
    let response = result.response;

    if codex_chat_to_responses {
        return handle_codex_responses_to_chat_transform(
            response,
            &ctx,
            &state,
            is_stream,
            connection_guard,
        )
        .await;
    }

    process_response(
        response,
        &ctx,
        &state,
        &OPENAI_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

/// 为第三方 Agent OpenAI-compatible API 解析本次请求的后端 provider。
fn resolve_external_openai_compatible_provider(
    state: &ProxyState,
    body: &Value,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Option<crate::provider::Provider>, ProxyError> {
    match profile.backend_type {
        ExternalOpenAiApiBackendType::Provider => resolve_external_provider_target(state, profile),
        ExternalOpenAiApiBackendType::CodexRouterRoute => {
            resolve_external_codex_router_target(state, body, profile)
        }
    }
}

/// 从请求体中提取模型名，仅用于尚未创建 RequestContext 时构造错误响应。
fn request_model_from_body(body: &Value) -> String {
    body.get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

/// 解析普通 provider target；非 Codex provider 会被临时包装成 OpenAI wire provider。
fn resolve_external_provider_target(
    state: &ProxyState,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Option<crate::provider::Provider>, ProxyError> {
    let Some(app_type) = profile.app_type.as_deref() else {
        return Ok(None);
    };
    let Some(provider_id) = profile.provider_id.as_deref() else {
        return Ok(None);
    };
    let providers = state
        .db
        .get_all_providers(app_type)
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    let Some(provider) = providers.get(provider_id).cloned() else {
        return Ok(None);
    };
    if app_type == AppType::Codex.as_str() {
        if is_codex_official_managed_oauth_provider(&provider) {
            return Ok(Some(build_external_codex_official_oauth_provider(provider)));
        }
        return Ok(Some(provider));
    }
    let app_type = app_type
        .parse::<AppType>()
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    Ok(Some(build_external_openai_provider_from_app_provider(
        provider, &app_type,
    )))
}

/// 判断 Codex 内置官方源是否表示“使用 CC Switch 托管的 Codex OAuth 登录态”。
fn is_codex_official_managed_oauth_provider(provider: &crate::provider::Provider) -> bool {
    provider.id == "codex-official"
}

/// 解析 Codex router route target。
fn resolve_external_codex_router_target(
    state: &ProxyState,
    body: &Value,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Option<crate::provider::Provider>, ProxyError> {
    let providers = state
        .db
        .get_all_providers("codex")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    for provider in providers.values() {
        if !is_codex_router_provider(provider) {
            continue;
        }
        if profile
            .provider_id
            .as_deref()
            .is_some_and(|id| id != provider.id)
        {
            continue;
        }
        if !codex_router_contains_route(provider, profile.route_id.as_deref()) {
            continue;
        }
        if let Some(resolved) =
            super::providers::resolve_codex_model_routed_provider(provider, body)
        {
            if profile.route_id.is_some()
                && resolved
                    .settings_config
                    .get("routeId")
                    .and_then(|value| value.as_str())
                    != profile.route_id.as_deref()
            {
                continue;
            }
            return Ok(Some(resolved));
        }
    }
    Ok(None)
}

/// 判断 provider 是否是显式开启的 Codex router。
fn is_codex_router_provider(provider: &crate::provider::Provider) -> bool {
    provider
        .settings_config
        .get("codexRouting")
        .and_then(|routing| routing.get("enabled"))
        .and_then(|enabled| enabled.as_bool())
        .unwrap_or(false)
}

/// 判断 Codex router 是否包含指定 route。
fn codex_router_contains_route(
    provider: &crate::provider::Provider,
    route_id: Option<&str>,
) -> bool {
    let Some(route_id) = route_id else {
        return true;
    };
    provider
        .settings_config
        .pointer("/codexRouting/routes")
        .and_then(|routes| routes.as_array())
        .map(|routes| {
            routes.iter().any(|route| {
                route.get("enabled").and_then(|value| value.as_bool()) != Some(false)
                    && route.get("id").and_then(|value| value.as_str()) == Some(route_id)
            })
        })
        .unwrap_or(false)
}

/// 生成第三方 Agent API 可见的模型列表。
fn external_openai_api_models_response(
    state: &ProxyState,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Value, ProxyError> {
    let models = match profile.backend_type {
        ExternalOpenAiApiBackendType::Provider => {
            let provider = resolve_external_provider_source(state, profile)?.ok_or_else(|| {
                ProxyError::InvalidRequest("External API provider target not found".to_string())
            })?;
            external_provider_model_entries(&provider, profile)
        }
        ExternalOpenAiApiBackendType::CodexRouterRoute => {
            external_codex_router_model_entries(state, profile)?
        }
    };
    Ok(json!({
        "object": "list",
        "data": models,
        "models": models
            .iter()
            .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
            .map(|id| json!({ "id": id }))
            .collect::<Vec<_>>()
    }))
}

/// 解析原始 provider target；用于展示模型列表，避免 synthetic provider 丢失模型 catalog。
fn resolve_external_provider_source(
    state: &ProxyState,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Option<crate::provider::Provider>, ProxyError> {
    let Some(app_type) = profile.app_type.as_deref() else {
        return Ok(None);
    };
    let Some(provider_id) = profile.provider_id.as_deref() else {
        return Ok(None);
    };
    let providers = state
        .db
        .get_all_providers(app_type)
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    Ok(providers.get(provider_id).cloned())
}

/// 从普通 provider 配置中提取可展示模型。
fn external_provider_model_entries(
    provider: &crate::provider::Provider,
    profile: &ExternalOpenAiApiProfile,
) -> Vec<Value> {
    let mut ids = Vec::new();
    collect_string_array(&mut ids, provider.settings_config.get("models"));
    collect_string_array(&mut ids, provider.settings_config.get("modelList"));
    collect_model_objects(&mut ids, provider.settings_config.get("models"));
    collect_model_objects(&mut ids, provider.settings_config.get("modelCatalog"));
    if let Some(model) = provider
        .settings_config
        .get("model")
        .and_then(|value| value.as_str())
    {
        ids.push(model.to_string());
    }
    if let Some(model) = profile.default_model.as_deref() {
        ids.push(model.to_string());
    }
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter(|id| !id.trim().is_empty())
        .map(|id| openai_model_entry(&id, "cc-switch"))
        .collect()
}

/// 从 Codex router route 中提取可展示模型。
fn external_codex_router_model_entries(
    state: &ProxyState,
    profile: &ExternalOpenAiApiProfile,
) -> Result<Vec<Value>, ProxyError> {
    let Some(provider_id) = profile.provider_id.as_deref() else {
        return Ok(Vec::new());
    };
    let providers = state
        .db
        .get_all_providers("codex")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    let Some(provider) = providers.get(provider_id) else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    if let Some(routes) = provider
        .settings_config
        .pointer("/codexRouting/routes")
        .and_then(|routes| routes.as_array())
    {
        for route in routes {
            if route.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
                continue;
            }
            if profile.route_id.is_some()
                && route.get("id").and_then(|value| value.as_str()) != profile.route_id.as_deref()
            {
                continue;
            }
            collect_string_array(&mut ids, route.pointer("/match/models"));
            if ids.is_empty() {
                collect_string_array(&mut ids, route.pointer("/match/prefixes"));
            }
        }
    }
    if let Some(model) = profile.default_model.as_deref() {
        ids.push(model.to_string());
    }
    ids.sort();
    ids.dedup();
    Ok(ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .map(|id| openai_model_entry(&id, "cc-switch"))
        .collect())
}

/// 收集字符串数组里的模型 id。
fn collect_string_array(ids: &mut Vec<String>, value: Option<&Value>) {
    if let Some(values) = value.and_then(|value| value.as_array()) {
        for value in values {
            if let Some(id) = value.as_str() {
                ids.push(id.to_string());
            }
        }
    }
}

/// 收集对象数组里的模型 id。
fn collect_model_objects(ids: &mut Vec<String>, value: Option<&Value>) {
    if let Some(values) = value.and_then(|value| value.as_array()) {
        for value in values {
            if let Some(id) = value
                .get("id")
                .or_else(|| value.get("model"))
                .or_else(|| value.get("name"))
                .and_then(|value| value.as_str())
            {
                ids.push(id.to_string());
            }
        }
    }
}

/// 构造 OpenAI-compatible model entry。
fn openai_model_entry(id: &str, owner: &str) -> Value {
    json!({
        "id": id,
        "object": "model",
        "created": 0,
        "owned_by": owner
    })
}

/// 从其他 app provider 构造本次请求专用的 OpenAI wire provider。
fn build_external_openai_provider_from_app_provider(
    provider: crate::provider::Provider,
    app_type: &AppType,
) -> crate::provider::Provider {
    let (base_url, api_key) = provider.resolve_usage_credentials(app_type);
    let model = provider
        .settings_config
        .get("model")
        .or_else(|| provider.settings_config.get("defaultModel"))
        .and_then(|value| value.as_str())
        .unwrap_or("default");
    let mut synthetic = provider.clone();
    synthetic.id = format!(
        "external-openai-api::{}::{}",
        app_type.as_str(),
        provider.id
    );
    synthetic.name = format!("{} via OpenAI API", provider.name);
    synthetic.settings_config = json!({
        "base_url": base_url,
        "auth": { "OPENAI_API_KEY": api_key },
        "model": model,
        "apiFormat": "openai_chat"
    });
    synthetic
}

/// 为第三方 Agent API 构造只在本次请求内使用的 Codex 官方 OAuth provider。
///
/// 内置 `codex-official` 在数据库里故意保存为空 config/auth，用于表达“恢复官方登录态”。
/// 代理请求需要显式标记 `codex_oauth`，这样下游适配器会注入托管 OAuth 并转到 ChatGPT Codex 后端。
fn build_external_codex_official_oauth_provider(
    provider: crate::provider::Provider,
) -> crate::provider::Provider {
    let mut synthetic = provider.clone();
    let mut meta = synthetic.meta.unwrap_or_default();
    meta.provider_type = Some("codex_oauth".to_string());
    synthetic.meta = Some(meta);
    synthetic.settings_config = json!({
        "config": "model_provider = \"openai\"\nmodel = \"gpt-5.4-mini\"\n",
        "auth": {},
        "models": ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex-spark"]
    });
    synthetic
}

/// 生成 External OpenAI-compatible API 的鉴权错误响应。
fn external_openai_api_auth_error_response(
    err: ExternalOpenAiApiAuthError,
) -> axum::response::Response {
    let (status, message, code) = match err {
        ExternalOpenAiApiAuthError::Disabled => (
            StatusCode::FORBIDDEN,
            "External OpenAI-compatible API is disabled in CC Switch.",
            "external_openai_api_disabled",
        ),
        ExternalOpenAiApiAuthError::MissingKey => (
            StatusCode::UNAUTHORIZED,
            "Missing External OpenAI-compatible API key.",
            "external_openai_api_key_missing",
        ),
        ExternalOpenAiApiAuthError::InvalidKey => (
            StatusCode::UNAUTHORIZED,
            "Invalid External OpenAI-compatible API key.",
            "external_openai_api_key_invalid",
        ),
    };
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": "authentication_error",
                "code": code,
                "param": Value::Null
            }
        })),
    )
        .into_response()
}

/// 生成 External OpenAI-compatible API 的路由错误响应。
fn external_openai_api_route_error_response(model: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "message": format!(
                    "External OpenAI-compatible API has no enabled backend route for model `{model}`."
                ),
                "type": "invalid_request_error",
                "code": "external_openai_api_route_not_found",
                "param": "model"
            }
        })),
    )
        .into_response()
}

/// 生成 External OpenAI-compatible API 的暂不支持能力错误响应。
fn external_openai_api_unsupported_response(
    message: impl Into<String>,
    param: &str,
) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "invalid_request_error",
                "code": "external_openai_api_unsupported_backend",
                "param": param
            }
        })),
    )
        .into_response()
}

async fn handle_codex_responses_to_chat_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();
    if !status.is_success() {
        return handle_codex_responses_error_response(response, ctx, status).await;
    }
    if is_stream {
        let stream = response.bytes_stream();
        let sse_stream = openai_compat::create_chat_sse_stream_from_codex_responses(
            stream,
            ctx.request_model.clone(),
        );
        let logged_stream = create_logged_passthrough_stream(
            sse_stream,
            ctx.tag,
            None,
            ctx.streaming_timeout_config(),
            connection_guard,
        );
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );
        let body = axum::body::Body::from_stream(logged_stream);
        return Ok((headers, body).into_response());
    }

    let _connection_guard = connection_guard;
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let responses_response = if body_str.contains("event:") || body_str.contains("data:") {
        responses_sse_to_response_value(&body_str)?
    } else {
        serde_json::from_slice::<Value>(&body_bytes).map_err(|e| {
            ProxyError::TransformError(format!("Failed to parse upstream responses body: {e}"))
        })?
    };
    let chat_response =
        openai_compat::codex_responses_to_chat_completion(responses_response, &ctx.request_model)?;

    if let Some(usage) = TokenUsage::from_openai_response(&chat_response) {
        let model = chat_response
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&ctx.request_model);
        let request_model = ctx.request_model.clone();
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = model.to_string();
            let session_id = ctx.session_id.clone();
            let latency_ms = ctx.latency_ms();
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    "codex",
                    &model,
                    &request_model,
                    usage,
                    latency_ms,
                    None,
                    false,
                    status.as_u16(),
                    Some(session_id),
                )
                .await;
            }
        });
    }

    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    response_headers.remove(axum::http::header::CONTENT_TYPE);
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    let response_body = serde_json::to_vec(&chat_response).map_err(|e| {
        ProxyError::TransformError(format!("Failed to serialize chat response: {e}"))
    })?;
    builder
        .body(axum::body::Body::from(response_body))
        .map_err(|e| ProxyError::Internal(format!("Failed to build response: {e}")))
}

async fn handle_codex_responses_error_response(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    status: axum::http::StatusCode,
) -> Result<axum::response::Response, ProxyError> {
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    let parsed_value: Value = serde_json::from_slice(&body_bytes)
        .unwrap_or_else(|_| json!(String::from_utf8_lossy(&body_bytes).to_string()));
    let chat_error = openai_compat::responses_error_to_chat_error(Some(&parsed_value));
    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    response_headers.remove(axum::http::header::CONTENT_TYPE);
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    let body = serde_json::to_vec(&chat_error).map_err(|e| {
        ProxyError::TransformError(format!("Failed to serialize chat error response: {e}"))
    })?;
    builder
        .body(axum::body::Body::from(body))
        .map_err(|e| ProxyError::Internal(format!("Failed to build chat error response: {e}")))
}

fn build_chat_proxy_error_response(
    ctx: &RequestContext,
    endpoint: &str,
    error: &ProxyError,
) -> Result<axum::response::Response, ProxyError> {
    let status = axum::http::StatusCode::from_u16(map_proxy_error_to_status(error))
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let body = json!({
        "error": {
            "message": format!(
                "{} (provider={}, model={}, endpoint={})",
                get_error_message(error),
                ctx.provider.name,
                ctx.request_model,
                endpoint
            ),
            "type": "proxy_error",
            "code": codex_proxy_error_code(error),
            "param": Value::Null
        }
    });
    let body = serde_json::to_vec(&body)
        .map_err(|e| ProxyError::Internal(format!("Failed to serialize proxy error: {e}")))?;
    axum::response::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        )
        .body(axum::body::Body::from(body))
        .map_err(|e| ProxyError::Internal(format!("Failed to build proxy error response: {e}")))
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI 透传）
pub async fn handle_responses(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let endpoint = endpoint_with_query(&uri, "/responses");
    let mut ctx = if !is_codex_model_catalog_client(&headers) {
        let external_api_profile = match external_openai_api::validate_request(&state.db, &headers)
        {
            Ok(profile) => profile,
            Err(err) => return Ok(external_openai_api_auth_error_response(err)),
        };
        let provider = match resolve_external_openai_compatible_provider(
            &state,
            &body,
            &external_api_profile,
        )? {
            Some(provider) => provider,
            None => {
                return Ok(external_openai_api_route_error_response(
                    &request_model_from_body(&body),
                ))
            }
        };
        if !provider.is_codex_oauth() {
            return Ok(external_openai_api_unsupported_response(
                "/v1/responses is only available for OpenAI Official/Codex OAuth backends in the External OpenAI-compatible API. Use /v1/chat/completions for this backend.",
                "backend",
            ));
        }
        RequestContext::new_with_provider(
            &state,
            &body,
            &headers,
            AppType::Codex,
            "Codex",
            "codex",
            provider,
        )
        .await?
    } else {
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?
    };

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            method,
            &endpoint,
            body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return build_codex_proxy_error_response(&ctx, &endpoint, &err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.provider = result.provider;
    let response = result.response;

    if super::providers::should_convert_codex_responses_to_chat(&ctx.provider, &endpoint) {
        return handle_codex_chat_to_responses_transform(
            response,
            &ctx,
            &state,
            is_stream,
            connection_guard,
            codex_tool_context,
        )
        .await;
    }

    process_response(
        response,
        &ctx,
        &state,
        &CODEX_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

/// 处理 /v1/responses/compact 请求（OpenAI Responses Compact API - Codex CLI 透传）
pub async fn handle_responses_compact(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    let endpoint = endpoint_with_query(&uri, "/responses/compact");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            method,
            &endpoint,
            body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return build_codex_proxy_error_response(&ctx, &endpoint, &err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.provider = result.provider;
    let response = result.response;

    if super::providers::should_convert_codex_responses_to_chat(&ctx.provider, &endpoint) {
        return handle_codex_chat_to_responses_transform(
            response,
            &ctx,
            &state,
            is_stream,
            connection_guard,
            codex_tool_context,
        )
        .await;
    }

    process_response(
        response,
        &ctx,
        &state,
        &CODEX_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

async fn handle_codex_chat_to_responses_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
    tool_context: transform_codex_chat::CodexToolContext,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if !status.is_success() {
        // 上游 Chat 错误体形状与 Responses 不一致（如 MiniMax 的 base_resp、自定义 detail 字段）；
        // 直接透传会让 Codex 客户端无法识别错误码。这里统一转换为 Responses 风格
        // `{"error": {message, type, code, param}}`，保留原始 HTTP 状态码。
        return handle_codex_chat_error_response(response, ctx, status).await;
    }

    if is_stream || response.is_sse() {
        let stream = response.bytes_stream();
        let sse_stream = create_responses_sse_stream_from_chat_with_context(stream, tool_context);
        let sse_stream = record_responses_sse_stream(sse_stream, state.codex_chat_history.clone());

        let usage_collector = if usage_logging_enabled(state) {
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let request_model = ctx.request_model.clone();
            let start_time = ctx.start_time;
            let session_id = ctx.session_id.clone();

            Some(SseUsageCollector::new(
                start_time,
                Some(codex_stream_usage_event_filter),
                move |events, first_token_ms| {
                    let usage =
                        TokenUsage::from_codex_stream_events_auto(&events).unwrap_or_default();
                    let model = usage.model.clone().unwrap_or_else(|| request_model.clone());
                    let latency_ms = start_time.elapsed().as_millis() as u64;

                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    let request_model = request_model.clone();
                    let session_id = session_id.clone();

                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "codex",
                            &model,
                            &request_model,
                            usage,
                            latency_ms,
                            first_token_ms,
                            true,
                            status.as_u16(),
                            Some(session_id),
                        )
                        .await;
                    });
                },
            ))
        } else {
            None
        };

        let logged_stream = create_logged_passthrough_stream(
            sse_stream,
            ctx.tag,
            usage_collector,
            ctx.streaming_timeout_config(),
            connection_guard,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );

        let body = axum::body::Body::from_stream(logged_stream);
        return Ok((headers, body).into_response());
    }

    let _connection_guard = connection_guard;
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let chat_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        log::error!("[Codex] 解析 Chat 上游响应失败: {e}, body: {body_str}");
        ProxyError::TransformError(format!("Failed to parse upstream chat response: {e}"))
    })?;
    let responses_response = transform_codex_chat::chat_completion_to_response_with_context(
        chat_response,
        &tool_context,
    )
    .map_err(|e| {
        log::error!("[Codex] Chat → Responses 响应转换失败: {e}");
        e
    })?;
    state
        .codex_chat_history
        .record_response(&responses_response)
        .await;

    if let Some(usage) = TokenUsage::from_codex_response_auto(&responses_response) {
        let model = responses_response
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&ctx.request_model);
        let request_model = ctx.request_model.clone();
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = model.to_string();
            let session_id = ctx.session_id.clone();
            let latency_ms = ctx.latency_ms();
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    "codex",
                    &model,
                    &request_model,
                    usage,
                    latency_ms,
                    None,
                    false,
                    status.as_u16(),
                    Some(session_id),
                )
                .await;
            }
        });
    }

    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    // Builder::header 是 append 语义；不先 remove 会和上游 Content-Type 双发。
    response_headers.remove(axum::http::header::CONTENT_TYPE);

    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let response_body = serde_json::to_vec(&responses_response).map_err(|e| {
        log::error!("[Codex] 序列化 Responses 响应失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize responses response: {e}"))
    })?;

    builder
        .body(axum::body::Body::from(response_body))
        .map_err(|e| {
            log::error!("[Codex] 构建 Responses 响应失败: {e}");
            ProxyError::Internal(format!("Failed to build response: {e}"))
        })
}

/// 把上游 Chat Completions 的错误响应转换为 Responses API 错误形状。
///
/// 与正常响应分支配套：正常响应已经被改写成 Responses 形式，错误响应若仍保留
/// Chat 错误体（如 MiniMax 的 `{"base_resp": {"status_code": 2013}}`），Codex
/// 客户端的错误处理就无法对齐字段。这里读取上游 body、规整成
/// `{"error": {message, type, code, param}}` 并保留原始 HTTP 状态码。
async fn handle_codex_chat_error_response(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    status: axum::http::StatusCode,
) -> Result<axum::response::Response, ProxyError> {
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;

    // 非 JSON 上游错误体（Cloudflare HTML、纯文本 "Unauthorized" 等）若丢成 None，
    // 客户端就看不到原始诊断信息；包成 Value::String 走转换函数的字符串分支。
    let parsed_value: Value = match serde_json::from_slice::<Value>(&body_bytes) {
        Ok(value) => value,
        Err(_) => {
            const MAX_RAW_ERROR_BYTES: usize = 1024;
            let lossy = String::from_utf8_lossy(&body_bytes);
            let truncated = if lossy.len() > MAX_RAW_ERROR_BYTES {
                let mut end = MAX_RAW_ERROR_BYTES;
                while end > 0 && !lossy.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…(truncated)", &lossy[..end])
            } else {
                lossy.into_owned()
            };
            log::warn!("[Codex] Chat 错误响应不是合法 JSON，按文本透传: {truncated}");
            Value::String(truncated)
        }
    };

    let responses_error = transform_codex_chat::chat_error_to_response_error(Some(&parsed_value));

    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    // Builder::header 是 append 语义；不先 remove 会和上游 Content-Type 双发。
    response_headers.remove(axum::http::header::CONTENT_TYPE);

    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let body = serde_json::to_vec(&responses_error).map_err(|e| {
        log::error!("[Codex] 序列化 Responses 错误体失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize responses error: {e}"))
    })?;

    builder.body(axum::body::Body::from(body)).map_err(|e| {
        log::error!("[Codex] 构建 Responses 错误响应失败: {e}");
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

/// 把转发层（非上游响应）的失败构造成富化的 Codex 错误响应。
///
/// 与 `handle_codex_chat_error_response`（处理上游真实错误响应、复制上游头）不同，
/// 这里没有上游响应可参照，只产出一个 `application/json` 错误体。状态码走
/// `map_proxy_error_to_status`，该函数已与 `ProxyError::into_response` 对齐。
///
/// 注意：`endpoint` 经 `endpoint_with_query` 可能携带 query（如 `?beta=true`）并被
/// 原样写入错误体。当前 Codex 端点不在 query 里放凭证，故安全；若将来复用到
/// query 携带密钥的端点（如 Gemini 的 `?key=`），需先脱敏再回显。
fn build_codex_proxy_error_response(
    ctx: &RequestContext,
    endpoint: &str,
    error: &ProxyError,
) -> Result<axum::response::Response, ProxyError> {
    let status = axum::http::StatusCode::from_u16(map_proxy_error_to_status(error))
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let body = codex_proxy_error_json(&ctx.provider.name, &ctx.request_model, endpoint, error);
    let body = serde_json::to_vec(&body).map_err(|e| {
        log::error!("[Codex] 序列化代理错误体失败: {e}");
        ProxyError::Internal(format!("Failed to serialize proxy error: {e}"))
    })?;

    axum::response::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        )
        .body(axum::body::Body::from(body))
        .map_err(|e| {
            log::error!("[Codex] 构建代理错误响应失败: {e}");
            ProxyError::Internal(format!("Failed to build proxy error response: {e}"))
        })
}

fn codex_proxy_error_json(
    provider_name: &str,
    request_model: &str,
    endpoint: &str,
    error: &ProxyError,
) -> Value {
    let (mut body, upstream_status) = match error {
        ProxyError::UpstreamError { status, body } => {
            let parsed_body = body
                .as_deref()
                .map(|body| serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!(body)));
            (
                transform_codex_chat::chat_error_to_response_error(parsed_body.as_ref()),
                Some(*status),
            )
        }
        _ => (
            json!({
                "error": {
                    "message": get_error_message(error),
                    "type": "proxy_error",
                    "code": codex_proxy_error_code(error),
                    "param": Value::Null,
                }
            }),
            None,
        ),
    };

    let Some(error_obj) = body
        .get_mut("error")
        .and_then(|value| value.as_object_mut())
    else {
        return body;
    };

    let message = if upstream_status == Some(413) {
        // 413 来自上游渠道商的网关（典型是 nginx 的 client_max_body_size），不是 CC
        // Switch 本地代理的限制（本地 DefaultBodyLimit 已放到 200MB）。上游响应体往往是
        // 一整段 nginx HTML，对用户毫无价值，这里替换成明确指向上游 + 可操作的指引，
        // 避免「以为是 CC Switch 封装了 nginx / 是本地代理的锅」这种反复出现的误解。
        format!(
            concat!(
                "Upstream provider rejected the request with HTTP 413 (Payload Too Large). ",
                "The request body exceeds the upstream gateway's size limit; this is the ",
                "provider's server-side limit, not a CC Switch limit. ",
                "Provider: {provider}; model: {model}; endpoint: {endpoint}. ",
                "To recover, shrink the request: run /compact, remove large pasted logs or ",
                "inline images, or ask the provider to raise its request body limit ",
                "(e.g. nginx client_max_body_size)."
            ),
            provider = provider_name,
            model = request_model,
            endpoint = endpoint,
        )
    } else {
        let cause = error_obj
            .get("message")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| get_error_message(error));
        let status_fragment = upstream_status
            .map(|status| format!("; upstream_status: HTTP {status}"))
            .unwrap_or_default();
        format!(
            "CC Switch local proxy failed while handling Codex endpoint {endpoint}. Provider: {provider_name}; model: {request_model}{status_fragment}; cause: {cause}"
        )
    };

    error_obj.insert(
        "message".to_string(),
        Value::String(compact_error_message(&message, 1800)),
    );

    if error_obj
        .get("type")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        error_obj.insert("type".to_string(), Value::String("proxy_error".to_string()));
    }

    if error_obj.get("code").map(Value::is_null).unwrap_or(true) {
        error_obj.insert(
            "code".to_string(),
            Value::String(codex_proxy_error_code(error).to_string()),
        );
    }

    if !error_obj.contains_key("param") {
        error_obj.insert("param".to_string(), Value::Null);
    }

    error_obj.insert(
        "provider".to_string(),
        Value::String(provider_name.to_string()),
    );
    error_obj.insert(
        "model".to_string(),
        Value::String(request_model.to_string()),
    );
    // 仅用于 Codex 本地路由；不要复用到 query 可能携带凭证的端点。
    error_obj.insert("endpoint".to_string(), Value::String(endpoint.to_string()));
    if let Some(status) = upstream_status {
        error_obj.insert(
            "upstream_status".to_string(),
            Value::Number(serde_json::Number::from(status)),
        );
    }

    body
}

fn codex_proxy_error_code(error: &ProxyError) -> &'static str {
    match error {
        ProxyError::ForwardFailed(_) => "cc_switch_forward_failed",
        ProxyError::Timeout(_) | ProxyError::StreamIdleTimeout(_) => "cc_switch_timeout",
        ProxyError::NoAvailableProvider => "cc_switch_no_available_provider",
        ProxyError::AllProvidersCircuitOpen => "cc_switch_all_providers_circuit_open",
        ProxyError::NoProvidersConfigured => "cc_switch_no_providers_configured",
        ProxyError::MaxRetriesExceeded => "cc_switch_max_retries_exceeded",
        ProxyError::ProviderUnhealthy(_) => "cc_switch_provider_unhealthy",
        ProxyError::ConfigError(_) => "cc_switch_config_error",
        ProxyError::TransformError(_) => "cc_switch_transform_error",
        ProxyError::InvalidRequest(_) => "cc_switch_invalid_request",
        ProxyError::AuthError(_) => "cc_switch_auth_error",
        ProxyError::UpstreamError { .. } => "cc_switch_upstream_error",
        ProxyError::DatabaseError(_) => "cc_switch_database_error",
        ProxyError::Internal(_) => "cc_switch_internal_error",
        ProxyError::AlreadyRunning
        | ProxyError::NotRunning
        | ProxyError::BindFailed(_)
        | ProxyError::StopTimeout
        | ProxyError::StopFailed(_) => "cc_switch_proxy_error",
    }
}

fn compact_error_message(message: &str, max_chars: usize) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let truncated = normalized
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string();
    format!("{truncated}…(truncated)")
}

// ============================================================================
// Gemini API 处理器
// ============================================================================

/// 处理 Gemini API 请求（透传，包括查询参数）
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    // GET 类只读端点（/v1beta/models、/v1beta/models/<model> 等）没有请求体，
    // 不能强制 parse 为 JSON —— 否则空 body 会被拒绝。
    let body: Value = if body_bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body_bytes)
            .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?
    };

    // Gemini 的模型名称在 URI 中
    let mut ctx = RequestContext::new(&state, &body, &headers, AppType::Gemini, "Gemini", "gemini")
        .await?
        .with_model_from_uri(&uri);

    // 提取完整的路径和查询参数
    let endpoint = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path());

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &AppType::Gemini,
            method,
            endpoint,
            body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return Err(err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.provider = result.provider;
    let response = result.response;

    process_response(
        response,
        &ctx,
        &state,
        &GEMINI_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

fn should_use_claude_transform_streaming(
    requested_streaming: bool,
    upstream_is_sse: bool,
    api_format: &str,
    is_codex_oauth: bool,
) -> bool {
    requested_streaming || upstream_is_sse || (is_codex_oauth && api_format == "openai_responses")
}

/// 把 OpenAI Responses SSE 流聚合成一个完整的 Responses JSON 对象，供下游转成 Anthropic
/// 非流响应。仅在 Codex OAuth 把 `stream:false` 强制升级为 SSE 的场景下调用。
///
/// 复用 `proxy::sse` 的 `take_sse_block`/`strip_sse_field`：`take_sse_block` 同时支持
/// `\n\n` 与 `\r\n\r\n` 两种分隔符，`strip_sse_field` 兼容带/不带空格的字段写法。
fn responses_sse_to_response_value(body: &str) -> Result<Value, ProxyError> {
    let mut buffer = body.to_string();
    let mut completed_response: Option<Value> = None;
    let mut output_items = Vec::new();
    let mut output_text_deltas = Vec::new();

    while let Some(block) = take_sse_block(&mut buffer) {
        let mut event_name = "";
        let mut data_lines: Vec<&str> = Vec::new();

        for line in block.lines() {
            if let Some(evt) = strip_sse_field(line, "event") {
                event_name = evt.trim();
            } else if let Some(d) = strip_sse_field(line, "data") {
                data_lines.push(d);
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data_str = data_lines.join("\n");
        if data_str.trim() == "[DONE]" {
            continue;
        }

        let data: Value = serde_json::from_str(&data_str).map_err(|e| {
            ProxyError::TransformError(format!("Failed to parse upstream SSE event: {e}"))
        })?;

        match event_name {
            "response.output_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(|value| value.as_str()) {
                    output_text_deltas.push(delta.to_string());
                }
            }
            "response.output_item.done" => {
                if let Some(item) = data.get("item") {
                    output_items.push(item.clone());
                }
            }
            "response.completed" => {
                completed_response = Some(data.get("response").cloned().unwrap_or(data));
            }
            "response.failed" => {
                let message = data
                    .pointer("/response/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("response.failed event received");
                return Err(ProxyError::TransformError(message.to_string()));
            }
            _ => {}
        }
    }

    let mut response = completed_response.ok_or_else(|| {
        ProxyError::TransformError("No response.completed event in upstream SSE".to_string())
    })?;

    if output_items.is_empty() && !output_text_deltas.is_empty() {
        // Codex OAuth 的文本流可能只发送 output_text.delta，不发送完整 output_item.done。
        // 非流式聚合时需要把 delta 合成 Responses output message，供下游 Chat 转换器读取。
        output_items.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": output_text_deltas.join("")
            }]
        }));
    }

    if !output_items.is_empty() {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("output".to_string(), Value::Array(output_items));
        } else {
            return Err(ProxyError::TransformError(
                "response.completed payload is not an object".to_string(),
            ));
        }
    }

    Ok(response)
}

// ============================================================================
// 使用量记录（保留用于 Claude 转换逻辑）
// ============================================================================

fn log_forward_error(
    state: &ProxyState,
    ctx: &RequestContext,
    is_streaming: bool,
    error: &ProxyError,
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);
    let status_code = map_proxy_error_to_status(error);
    let error_message = get_error_message(error);
    let request_id = uuid::Uuid::new_v4().to_string();

    if let Err(e) = logger.log_error_with_context(
        request_id,
        ctx.provider.id.clone(),
        ctx.app_type_str.to_string(),
        ctx.request_model.clone(),
        status_code,
        error_message,
        ctx.latency_ms(),
        is_streaming,
        Some(ctx.session_id.clone()),
        None,
    ) {
        log::warn!("记录失败请求日志失败: {e}");
    }
}

/// 记录请求使用量
#[allow(clippy::too_many_arguments)]
async fn log_usage(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    request_model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
    session_id: Option<String>,
) {
    use super::usage::logger::UsageLogger;

    if !usage_logging_enabled(state) {
        return;
    }

    let logger = UsageLogger::new(&state.db);

    let (multiplier, pricing_model_source) =
        logger.resolve_pricing_config(provider_id, app_type).await;
    let pricing_model = if pricing_model_source == PRICING_SOURCE_REQUEST {
        request_model
    } else {
        model
    };

    let request_id = usage.dedup_request_id();

    if let Err(e) = logger.log_with_calculation(
        request_id,
        provider_id.to_string(),
        app_type.to_string(),
        model.to_string(),
        request_model.to_string(),
        pricing_model.to_string(),
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        session_id,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        codex_proxy_error_json, external_openai_api_models_response,
        external_openai_api_unsupported_response, responses_sse_to_response_value,
        should_handle_as_codex_client, should_use_claude_transform_streaming,
    };
    use crate::{
        database::Database,
        provider::Provider,
        proxy::{
            external_openai_api::{
                self, ExternalOpenAiApiBackendType, ExternalOpenAiApiProfileUpdate,
            },
            failover_switch::FailoverSwitchManager,
            provider_router::ProviderRouter,
            providers::{
                codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
            },
            server::ProxyState,
            types::{ProxyConfig, ProxyStatus},
            ProxyError,
        },
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::RwLock;

    #[test]
    fn codex_oauth_responses_force_streaming_even_if_client_sent_false() {
        assert!(should_use_claude_transform_streaming(
            false,
            false,
            "openai_responses",
            true,
        ));
    }

    #[test]
    fn upstream_sse_response_always_uses_streaming_path() {
        assert!(should_use_claude_transform_streaming(
            false,
            true,
            "openai_chat",
            false,
        ));
    }

    #[test]
    fn non_streaming_response_stays_non_streaming_for_regular_openai_responses() {
        assert!(!should_use_claude_transform_streaming(
            false,
            false,
            "openai_responses",
            false,
        ));
    }

    #[test]
    fn external_api_key_takes_precedence_over_codex_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::USER_AGENT,
            HeaderValue::from_static("codex-compatible-agent"),
        );
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ccsw_test"),
        );

        assert!(!should_handle_as_codex_client(&headers));
    }

    #[test]
    fn responses_sse_to_response_value_collects_output_items() {
        let sse = r#"event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","model":"gpt-5.4","output":[],"usage":{"input_tokens":10,"output_tokens":2}}}

"#;

        let response = responses_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "resp_1");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "hello");
    }

    #[test]
    fn responses_sse_to_response_value_handles_crlf_delimiters() {
        // 真实 HTTP SSE 按规范使用 \r\n\r\n 分隔事件；take_sse_block 必须同时处理两种分隔符，
        // 否则此路径在任何标准上游（含 Codex OAuth HTTPS 后端）下都会 TransformError。
        let sse = "event: response.output_item.done\r\n\
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi\"}]}}\r\n\
\r\n\
event: response.completed\r\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_crlf\",\"status\":\"completed\",\"model\":\"gpt-5.4\",\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\r\n\
\r\n";

        let response = responses_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "resp_crlf");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "hi");
    }

    #[test]
    fn responses_sse_to_response_value_collects_output_text_deltas() {
        // Codex OAuth 真实文本响应会以 response.output_text.delta 增量下发；
        // 非流式 OpenAI Chat 兼容层必须能把这些 delta 聚合成完整 assistant message。
        let sse = "event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"po\"}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"delta\":\"ng\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_delta\",\"status\":\"completed\",\"model\":\"gpt-5.4\",\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n";

        let response = responses_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "resp_delta");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "pong");
    }

    #[test]
    fn responses_sse_to_response_value_returns_err_on_response_failed() {
        let sse = "event: response.failed\n\
data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"upstream blew up\"}}}\n\n";

        let err = responses_sse_to_response_value(sse).unwrap_err();
        match err {
            ProxyError::TransformError(msg) => assert!(msg.contains("upstream blew up")),
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn responses_sse_to_response_value_errors_when_no_completed_event() {
        let sse = "event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\"}}\n\n";

        assert!(responses_sse_to_response_value(sse).is_err());
    }

    #[test]
    fn codex_proxy_forward_error_includes_context_and_cause() {
        let error = ProxyError::ForwardFailed("连接失败: dns lookup failed".to_string());
        let body = codex_proxy_error_json("DeepSeek", "deepseek-chat", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("CC Switch local proxy failed"));
        assert!(message.contains("DeepSeek"));
        assert!(message.contains("deepseek-chat"));
        assert!(message.contains("/responses"));
        assert!(message.contains("dns lookup failed"));
        assert_eq!(body["error"]["code"], "cc_switch_forward_failed");
        assert_eq!(body["error"]["provider"], "DeepSeek");
        assert_eq!(body["error"]["model"], "deepseek-chat");
    }

    #[test]
    fn codex_proxy_upstream_error_normalizes_nonstandard_body() {
        let error = ProxyError::UpstreamError {
            status: 502,
            body: Some(
                r#"{"base_resp":{"status_code":2013,"status_msg":"upstream gateway failed"}}"#
                    .to_string(),
            ),
        };
        let body = codex_proxy_error_json("MiniMax", "abab6.5s", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("upstream_status: HTTP 502"));
        assert!(message.contains("upstream gateway failed"));
        assert_eq!(body["error"]["code"], 2013);
        assert_eq!(body["error"]["upstream_status"], 502);
    }

    #[test]
    fn codex_proxy_413_points_to_upstream_not_local_proxy() {
        // 模拟上游渠道商 nginx 因 client_max_body_size 返回的 413 HTML 页面
        // （见 issue #666：长上下文 / 大图 / 大日志撞上游体积上限）
        let error = ProxyError::UpstreamError {
            status: 413,
            body: Some(
                "<html>\r\n<head><title>413 Request Entity Too Large</title></head>\r\n\
                 <body>\r\n<center><h1>413 Request Entity Too Large</h1></center>\r\n\
                 <hr><center>nginx/1.29.6</center>\r\n</body>\r\n</html>"
                    .to_string(),
            ),
        };
        let body = codex_proxy_error_json("HCAI", "gpt-5.5", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        // 不再误导成「本地代理失败」
        assert!(!message.contains("CC Switch local proxy failed"));
        // 明确指向上游 + 体积超限 + 可操作指引
        assert!(message.contains("413"));
        assert!(message.to_lowercase().contains("upstream"));
        assert!(message.contains("/compact"));
        // 关键：不把整段 nginx HTML 回显给用户
        assert!(!message.contains("<html>"));
        assert!(!message.contains("nginx/1.29.6"));
        // 结构化字段仍然保留，便于程序化消费 / UI 呈现
        assert_eq!(body["error"]["upstream_status"], 413);
        assert_eq!(body["error"]["provider"], "HCAI");
        assert_eq!(body["error"]["model"], "gpt-5.5");
        assert_eq!(body["error"]["endpoint"], "/responses");
    }

    #[test]
    fn external_models_response_only_exposes_profile_backend_models() {
        let db = Arc::new(Database::memory().expect("memory db"));
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "selected".to_string(),
                "Selected".to_string(),
                json!({
                    "base_url": "https://selected.example/v1",
                    "api_key": "sk-selected",
                    "models": ["visible-model"]
                }),
                None,
            ),
        )
        .expect("save selected provider");
        db.save_provider(
            "hermes",
            &Provider::with_id(
                "other".to_string(),
                "Other".to_string(),
                json!({
                    "base_url": "https://other.example/v1",
                    "api_key": "sk-other",
                    "models": ["hidden-model"]
                }),
                None,
            ),
        )
        .expect("save other provider");
        external_openai_api::regenerate_api_key(&db).expect("generate key");
        external_openai_api::update_profile(
            &db,
            ExternalOpenAiApiProfileUpdate {
                enabled: true,
                backend_type: ExternalOpenAiApiBackendType::Provider,
                app_type: Some("hermes".to_string()),
                provider_id: Some("selected".to_string()),
                route_id: None,
                default_model: Some("default-visible".to_string()),
            },
        )
        .expect("save profile");
        let profile = external_openai_api::load_profile(&db).expect("load profile");
        let state = build_state(db);

        let response = external_openai_api_models_response(&state, &profile).expect("models");
        let ids: Vec<_> = response["data"]
            .as_array()
            .expect("data array")
            .iter()
            .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
            .collect();

        assert!(ids.contains(&"visible-model"));
        assert!(ids.contains(&"default-visible"));
        assert!(!ids.contains(&"hidden-model"));
    }

    #[tokio::test]
    async fn unsupported_responses_backend_returns_openai_style_error() {
        let response = external_openai_api_unsupported_response(
            "/v1/responses is not available for this backend.",
            "backend",
        );

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let value: Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(value["error"]["type"], "invalid_request_error");
        assert_eq!(
            value["error"]["code"],
            "external_openai_api_unsupported_backend"
        );
        assert_eq!(value["error"]["param"], "backend");
    }

    fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(ProxyConfig::default())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            app_handle: None,
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        }
    }
}
