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
    handler_config::{
        CLAUDE_PARSER_CONFIG, CODEX_PARSER_CONFIG, GEMINI_PARSER_CONFIG, OPENAI_PARSER_CONFIG,
    },
    handler_context::RequestContext,
    providers::{
        get_adapter, get_claude_api_format, get_codex_api_format,
        streaming::create_anthropic_sse_stream,
        streaming_responses::create_anthropic_sse_stream_from_responses, transform,
        transform_compat, transform_responses,
    },
    response_processor::{create_logged_passthrough_stream, process_response, SseUsageCollector},
    server::ProxyState,
    types::*,
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::app_config::AppType;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use bytes::Bytes;
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
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Claude, "Claude", "claude").await?;

    let endpoint = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str())
        .unwrap_or(uri.path());

    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // 转发请求
    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Claude,
            endpoint,
            body.clone(),
            headers,
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

    ctx.provider = result.provider;
    let api_format = result
        .claude_api_format
        .as_deref()
        .unwrap_or_else(|| get_claude_api_format(&ctx.provider))
        .to_string();
    let response = result.response;

    // 检查是否需要格式转换（OpenRouter 等中转服务）
    let adapter = get_adapter(&AppType::Claude);
    let needs_transform = adapter.needs_transform(&ctx.provider);

    // Claude 特有：格式转换处理
    if needs_transform {
        return handle_claude_transform(response, &ctx, &state, &body, is_stream, &api_format)
            .await;
    }

    // 通用响应处理（透传模式）
    process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG).await
}

/// Claude 格式转换处理（独有逻辑）
///
/// 支持 OpenAI Chat Completions 和 Responses API 两种格式的转换
async fn handle_claude_transform(
    response: reqwest::Response,
    ctx: &RequestContext,
    state: &ProxyState,
    _original_body: &Value,
    is_stream: bool,
    api_format: &str,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if is_stream {
        // 根据 api_format 选择流式转换器
        let stream = response.bytes_stream();
        let sse_stream: Box<
            dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin,
        > = if api_format == "openai_responses" {
            Box::new(Box::pin(create_anthropic_sse_stream_from_responses(stream)))
        } else {
            Box::new(Box::pin(create_anthropic_sse_stream(stream)))
        };

        // 创建使用量收集器
        let usage_collector = {
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = ctx.request_model.clone();
            let status_code = status.as_u16();
            let start_time = ctx.start_time;

            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                    let latency_ms = start_time.elapsed().as_millis() as u64;
                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    let model = model.clone();

                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "claude",
                            &model,
                            &model,
                            usage,
                            latency_ms,
                            first_token_ms,
                            true,
                            status_code,
                        )
                        .await;
                    });
                } else {
                    log::debug!("[Claude] OpenRouter 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };

        // 获取流式超时配置
        let timeout_config = ctx.streaming_timeout_config();

        let logged_stream = create_logged_passthrough_stream(
            sse_stream,
            "Claude/OpenRouter",
            Some(usage_collector),
            timeout_config,
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
        headers.insert(
            "Connection",
            axum::http::HeaderValue::from_static("keep-alive"),
        );

        let body = axum::body::Body::from_stream(logged_stream);
        return Ok((headers, body).into_response());
    }

    // 非流式响应转换 (OpenAI/Responses → Anthropic)
    let response_headers = response.headers().clone();

    let body_bytes = response.bytes().await.map_err(|e| {
        log::error!("[Claude] 读取响应体失败: {e}");
        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
    })?;

    let body_str = String::from_utf8_lossy(&body_bytes);

    let upstream_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        log::error!("[Claude] 解析上游响应失败: {e}, body: {body_str}");
        ProxyError::TransformError(format!("Failed to parse upstream response: {e}"))
    })?;

    // 根据 api_format 选择非流式转换器
    let anthropic_response = if api_format == "openai_responses" {
        transform_responses::responses_to_anthropic(upstream_response)
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
                )
                .await;
            }
        });
    }

    // 构建响应
    let mut builder = axum::response::Response::builder().status(status);

    for (key, value) in response_headers.iter() {
        if key.as_str().to_lowercase() != "content-length"
            && key.as_str().to_lowercase() != "transfer-encoding"
        {
            builder = builder.header(key, value);
        }
    }

    builder = builder.header("content-type", "application/json");

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
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    let endpoint = endpoint_with_query(&uri, "/chat/completions");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            &endpoint,
            body,
            headers,
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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &OPENAI_PARSER_CONFIG).await
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI）
///
/// 当供应商的 `meta.api_format == "openai_chat"` 时，将 Responses API 请求转换为
/// Chat Completions 格式，并将响应转换回 Responses API 格式返回给客户端。
pub async fn handle_responses(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    let endpoint = endpoint_with_query(&uri, "/responses");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Check api_format to determine transform mode
    let codex_format = get_codex_api_format(&ctx.provider);

    // Route based on api_format
    let (forward_endpoint, forward_body) = if codex_format == "anthropic" {
        let mut transformed = transform_compat::responses_to_anthropic_messages(body.clone())?;
        if let Some(upstream_model) = ctx
            .provider
            .settings_config
            .get("upstream_model")
            .and_then(|v| v.as_str())
        {
            transformed["model"] = json!(upstream_model);
        }
        ("/v1/messages", transformed)
    } else if codex_format == "openai_chat" {
        let mut transformed = transform_compat::responses_to_chat_completions(body.clone())?;
        if is_stream {
            transformed["stream"] = json!(false);
        }
        if let Some(upstream_model) = ctx
            .provider
            .settings_config
            .get("upstream_model")
            .and_then(|v| v.as_str())
        {
            transformed["model"] = json!(upstream_model);
        }
        ("/chat/completions", transformed)
    } else {
        ("/responses", body)
    };

    // Extract tools before forward_body is moved (needed for streaming converter)
    let anthropic_tools: Vec<serde_json::Value> = if codex_format == "anthropic" {
        forward_body
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            forward_endpoint,
            forward_body,
            headers,
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

    ctx.provider = result.provider;
    let response = result.response;

    if codex_format == "anthropic" {
        if is_stream {
            // Real-time streaming: Anthropic SSE → Responses API SSE
            let stream = response.bytes_stream();
            let converted =
                super::providers::streaming_compat::create_responses_sse_stream_from_anthropic(
                    stream,
                    anthropic_tools,
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
            headers.insert(
                "Connection",
                axum::http::HeaderValue::from_static("keep-alive"),
            );

            let body = axum::body::Body::from_stream(converted);
            return Ok((headers, body).into_response());
        }
        return handle_codex_anthropic_transform(response, false).await;
    }
    if codex_format == "openai_chat" {
        return handle_codex_transform(response, is_stream).await;
    }

    process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG).await
}

/// 处理 /v1/responses/compact 请求（OpenAI Responses Compact API - Codex CLI）
///
/// 与 `handle_responses` 相同的转换逻辑，但针对 `/responses/compact` 端点。
pub async fn handle_responses_compact(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    let endpoint = endpoint_with_query(&uri, "/responses/compact");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Check api_format to determine transform mode
    let codex_format = get_codex_api_format(&ctx.provider);

    // Route based on api_format
    let (forward_endpoint, forward_body) = if codex_format == "anthropic" {
        let mut transformed = transform_compat::responses_to_anthropic_messages(body.clone())?;
        if let Some(upstream_model) = ctx
            .provider
            .settings_config
            .get("upstream_model")
            .and_then(|v| v.as_str())
        {
            transformed["model"] = json!(upstream_model);
        }
        // Keep stream flag as-is — use real Anthropic streaming
        ("/v1/messages", transformed)
    } else if codex_format == "openai_chat" {
        let mut transformed = transform_compat::responses_to_chat_completions(body.clone())?;
        if is_stream {
            transformed["stream"] = json!(false);
        }
        if let Some(upstream_model) = ctx
            .provider
            .settings_config
            .get("upstream_model")
            .and_then(|v| v.as_str())
        {
            transformed["model"] = json!(upstream_model);
        }
        ("/chat/completions", transformed)
    } else {
        ("/responses/compact", body)
    };

    // Extract tools before forward_body is moved
    let anthropic_tools_compact: Vec<serde_json::Value> = if codex_format == "anthropic" {
        forward_body
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            forward_endpoint,
            forward_body,
            headers,
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

    ctx.provider = result.provider;
    let response = result.response;

    if codex_format == "anthropic" {
        if is_stream {
            let stream = response.bytes_stream();
            let converted =
                super::providers::streaming_compat::create_responses_sse_stream_from_anthropic(
                    stream,
                    anthropic_tools_compact,
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
            headers.insert(
                "Connection",
                axum::http::HeaderValue::from_static("keep-alive"),
            );

            let body = axum::body::Body::from_stream(converted);
            return Ok((headers, body).into_response());
        }
        return handle_codex_anthropic_transform(response, false).await;
    }
    if codex_format == "openai_chat" {
        return handle_codex_transform(response, is_stream).await;
    }

    process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG).await
}

/// Codex 格式转换处理（openai_chat 模式）
///
/// 读取 Chat Completions 完整响应，转换为 Responses API 格式。
/// 当客户端请求流式时，模拟 SSE 事件流输出。
async fn handle_codex_transform(
    response: reqwest::Response,
    is_stream: bool,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();
    let response_headers = response.headers().clone();

    let body_bytes = response.bytes().await.map_err(|e| {
        log::error!("[Codex] Failed to read response body: {e}");
        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
    })?;

    // If upstream returned an error, pass it through unchanged
    if !status.is_success() {
        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            let k = key.as_str().to_lowercase();
            if k != "content-length" && k != "transfer-encoding" {
                builder = builder.header(key, value);
            }
        }
        builder = builder.header("content-type", "application/json");
        let body = axum::body::Body::from(body_bytes.to_vec());
        return builder
            .body(body)
            .map_err(|e| ProxyError::Internal(format!("{e}")));
    }

    let upstream_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        let body_str = String::from_utf8_lossy(&body_bytes);
        log::error!("[Codex] Failed to parse upstream response: {e}, body: {body_str}");
        ProxyError::TransformError(format!("Failed to parse upstream response: {e}"))
    })?;

    let responses_api_response = transform_compat::chat_completions_to_responses(upstream_response)
        .map_err(|e| {
            log::error!("[Codex] Transform failed: {e}");
            e
        })?;

    if is_stream {
        // Simulate streaming by emitting SSE events from the complete response
        let events = build_responses_sse_events(&responses_api_response);
        let event_bytes: Vec<u8> = events.into_iter().flat_map(|e| e.into_bytes()).collect();

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );
        headers.insert(
            "Connection",
            axum::http::HeaderValue::from_static("keep-alive"),
        );

        let body = axum::body::Body::from(event_bytes);
        return Ok((headers, body).into_response());
    }

    // Non-streaming: return JSON response
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        let k = key.as_str().to_lowercase();
        if k != "content-length" && k != "transfer-encoding" {
            builder = builder.header(key, value);
        }
    }
    builder = builder.header("content-type", "application/json");

    let response_body = serde_json::to_vec(&responses_api_response)
        .map_err(|e| ProxyError::TransformError(format!("Failed to serialize response: {e}")))?;
    let body = axum::body::Body::from(response_body);
    builder
        .body(body)
        .map_err(|e| ProxyError::Internal(format!("{e}")))
}

/// Codex 格式转换处理（anthropic 模式）
///
/// 读取 Anthropic Messages API 完整响应，转换为 Responses API 格式。
/// 当客户端请求流式时，模拟 SSE 事件流输出。
async fn handle_codex_anthropic_transform(
    response: reqwest::Response,
    is_stream: bool,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();
    let response_headers = response.headers().clone();

    let body_bytes = response.bytes().await.map_err(|e| {
        log::error!("[Codex/Anthropic] Failed to read response body: {e}");
        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
    })?;

    // If upstream returned an error, pass it through unchanged
    if !status.is_success() {
        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            let k = key.as_str().to_lowercase();
            if k != "content-length" && k != "transfer-encoding" {
                builder = builder.header(key, value);
            }
        }
        builder = builder.header("content-type", "application/json");
        let body = axum::body::Body::from(body_bytes.to_vec());
        return builder
            .body(body)
            .map_err(|e| ProxyError::Internal(format!("{e}")));
    }

    let upstream_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        let body_str = String::from_utf8_lossy(&body_bytes);
        log::error!("[Codex/Anthropic] Failed to parse upstream response: {e}, body: {body_str}");
        ProxyError::TransformError(format!("Failed to parse upstream response: {e}"))
    })?;

    let responses_api_response =
        transform_compat::anthropic_messages_to_responses(upstream_response).map_err(|e| {
            log::error!("[Codex/Anthropic] Transform failed: {e}");
            e
        })?;

    if is_stream {
        // Simulate streaming by emitting SSE events from the complete response
        let events = build_responses_sse_events(&responses_api_response);
        let event_bytes: Vec<u8> = events.into_iter().flat_map(|e| e.into_bytes()).collect();

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );
        headers.insert(
            "Connection",
            axum::http::HeaderValue::from_static("keep-alive"),
        );

        let body = axum::body::Body::from(event_bytes);
        return Ok((headers, body).into_response());
    }

    // Non-streaming: return JSON response
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        let k = key.as_str().to_lowercase();
        if k != "content-length" && k != "transfer-encoding" {
            builder = builder.header(key, value);
        }
    }
    builder = builder.header("content-type", "application/json");

    let response_body = serde_json::to_vec(&responses_api_response)
        .map_err(|e| ProxyError::TransformError(format!("Failed to serialize response: {e}")))?;
    let body = axum::body::Body::from(response_body);
    builder
        .body(body)
        .map_err(|e| ProxyError::Internal(format!("{e}")))
}

/// Build SSE events from a complete Responses API response object.
///
/// Emits the standard event sequence that Codex CLI expects:
/// `response.created` → output item events → `response.completed`
fn build_responses_sse_events(response: &Value) -> Vec<String> {
    let mut events = Vec::new();
    let id = response
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp_unknown");

    // response.created
    events.push(format!(
        "event: response.created\ndata: {}\n\n",
        serde_json::to_string(&json!({
            "id": id,
            "object": "response",
            "status": "in_progress",
            "model": response.get("model").cloned().unwrap_or(json!("unknown")),
            "output": [],
            "usage": null
        }))
        .unwrap_or_default()
    ));

    // Emit output items
    if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
        for (idx, item) in output.iter().enumerate() {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    // response.output_item.added
                    events.push(format!(
                        "event: response.output_item.added\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "item": {
                                "type": "message",
                                "id": item.get("id").cloned().unwrap_or(json!("")),
                                "role": "assistant",
                                "status": "in_progress",
                                "content": []
                            }
                        }))
                        .unwrap_or_default()
                    ));

                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for (cidx, part) in content.iter().enumerate() {
                            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");

                            // response.content_part.added
                            events.push(format!(
                                "event: response.content_part.added\ndata: {}\n\n",
                                serde_json::to_string(&json!({
                                    "output_index": idx,
                                    "content_index": cidx,
                                    "part": {"type": "output_text", "text": "", "annotations": []}
                                }))
                                .unwrap_or_default()
                            ));

                            // response.output_text.delta (send full text as single delta)
                            if !text.is_empty() {
                                events.push(format!(
                                    "event: response.output_text.delta\ndata: {}\n\n",
                                    serde_json::to_string(&json!({
                                        "output_index": idx,
                                        "content_index": cidx,
                                        "delta": text
                                    }))
                                    .unwrap_or_default()
                                ));
                            }

                            // response.content_part.done
                            events.push(format!(
                                "event: response.content_part.done\ndata: {}\n\n",
                                serde_json::to_string(&json!({
                                    "output_index": idx,
                                    "content_index": cidx,
                                    "part": part
                                }))
                                .unwrap_or_default()
                            ));
                        }
                    }

                    // response.output_item.done
                    events.push(format!(
                        "event: response.output_item.done\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "item": item
                        }))
                        .unwrap_or_default()
                    ));
                }
                "function_call" => {
                    // response.output_item.added
                    events.push(format!(
                        "event: response.output_item.added\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "item": {
                                "type": "function_call",
                                "id": item.get("id").cloned().unwrap_or(json!("")),
                                "call_id": item.get("call_id").cloned().unwrap_or(json!("")),
                                "name": item.get("name").cloned().unwrap_or(json!("")),
                                "arguments": "",
                                "status": "in_progress"
                            }
                        }))
                        .unwrap_or_default()
                    ));

                    // function_call arguments delta
                    let args = item
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    events.push(format!(
                        "event: response.function_call_arguments.delta\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "delta": args
                        }))
                        .unwrap_or_default()
                    ));

                    // response.function_call_arguments.done
                    events.push(format!(
                        "event: response.function_call_arguments.done\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "arguments": args
                        }))
                        .unwrap_or_default()
                    ));

                    // response.output_item.done
                    events.push(format!(
                        "event: response.output_item.done\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "output_index": idx,
                            "item": item
                        }))
                        .unwrap_or_default()
                    ));
                }
                _ => {}
            }
        }
    }

    // response.completed
    events.push(format!(
        "event: response.completed\ndata: {}\n\n",
        serde_json::to_string(response).unwrap_or_default()
    ));

    events
}

// ============================================================================
// Gemini API 处理器
// ============================================================================

/// 处理 Gemini API 请求（透传，包括查询参数）
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
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
    let result = match forwarder
        .forward_with_retry(
            &AppType::Gemini,
            endpoint,
            body,
            headers,
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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &GEMINI_PARSER_CONFIG).await
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
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);

    let (multiplier, pricing_model_source) =
        logger.resolve_pricing_config(provider_id, app_type).await;
    let pricing_model = if pricing_model_source == "request" {
        request_model
    } else {
        model
    };

    let request_id = uuid::Uuid::new_v4().to_string();

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
        None,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}
