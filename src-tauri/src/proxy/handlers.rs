//! 请求处理器
//!
//! 处理各种API端点的HTTP请求
//!
//! 重构后的结构：
//! - 通用逻辑提取到 `handler_context` 和 `response_processor` 模块
//! - 各 handler 只保留独特的业务逻辑
//! - Claude 的格式转换逻辑保留在此文件（用于 OpenRouter 旧接口回退）

use super::{
    content_encoding::{decompress_body, get_content_encoding, is_supported_content_encoding},
    error_mapper::{get_error_message, map_proxy_error_to_status},
    forwarder::ActiveConnectionGuard,
    handler_config::{
        claude_stream_usage_event_filter, codex_stream_usage_event_filter, CLAUDE_PARSER_CONFIG,
        CODEX_PARSER_CONFIG, GEMINI_PARSER_CONFIG, OPENAI_PARSER_CONFIG,
    },
    handler_context::RequestContext,
    providers::{
        codex_chat_common::extract_reasoning_field_text,
        codex_chat_history::record_responses_sse_stream,
        get_adapter, get_claude_api_format,
        streaming::create_anthropic_sse_stream,
        streaming_codex_anthropic::{
            create_responses_sse_stream_from_anthropic_with_context,
            responses_sse_events_from_anthropic_message,
        },
        streaming_codex_chat::create_responses_sse_stream_from_chat_with_context,
        streaming_gemini::create_anthropic_sse_stream_from_gemini,
        streaming_responses::create_anthropic_sse_stream_from_responses,
        transform, transform_codex_anthropic, transform_codex_chat, transform_gemini,
        transform_responses,
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
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
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
pub async fn handle_models() -> Result<Json<Value>, ProxyError> {
    let generated_path = crate::codex_config::get_codex_model_catalog_path();
    let active_catalog_path = match crate::codex_config::read_codex_config_text() {
        Ok(config_text) => {
            crate::codex_config::resolve_cc_switch_catalog_path(&config_text, &generated_path)
        }
        Err(_) => None,
    };

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
    Ok(Json(catalog))
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
    ctx.outbound_model = result.outbound_model.take();
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
struct ClaudeUsageLog {
    model: String,
    request_model: String,
    outbound_model: String,
    app_type: &'static str,
    provider_id: String,
    session_id: String,
    usage: TokenUsage,
    latency_ms: u64,
    status_code: u16,
    is_streaming: bool,
}

fn prepare_claude_usage_log(
    ctx: &RequestContext,
    response: &Value,
    status_code: u16,
    is_streaming: bool,
) -> Option<ClaudeUsageLog> {
    let usage =
        TokenUsage::from_claude_response(response).filter(TokenUsage::has_billable_tokens)?;

    let model = response
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| ctx.outbound_model.clone())
        .unwrap_or_else(|| ctx.request_model.clone());

    Some(ClaudeUsageLog {
        model,
        request_model: ctx.request_model.clone(),
        outbound_model: ctx
            .outbound_model
            .clone()
            .unwrap_or_else(|| ctx.request_model.clone()),
        app_type: ctx.app_type_str,
        provider_id: ctx.provider.id.clone(),
        session_id: ctx.session_id.clone(),
        usage,
        latency_ms: ctx.latency_ms(),
        status_code,
        is_streaming,
    })
}

async fn write_claude_usage_log(state: &ProxyState, log: ClaudeUsageLog) {
    log_usage(
        state,
        &log.provider_id,
        log.app_type,
        &log.model,
        &log.request_model,
        &log.outbound_model,
        log.usage,
        log.latency_ms,
        None,
        log.is_streaming,
        log.status_code,
        Some(log.session_id),
    )
    .await;
}

fn spawn_claude_usage_log(
    state: &ProxyState,
    ctx: &RequestContext,
    response: &Value,
    status_code: u16,
    is_streaming: bool,
) {
    if !usage_logging_enabled(state) {
        return;
    }
    let Some(log) = prepare_claude_usage_log(ctx, response, status_code, is_streaming) else {
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        write_claude_usage_log(&state, log).await;
    });
}

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
            // 上游/转换层未回显模型时，优先用映射后的出站模型兜底（路由接管真值），
            // 其次才是客户端请求别名。空字符串视为缺失（转换器对无回显上游会合成 ""）。
            let fallback_model = ctx
                .outbound_model
                .clone()
                .unwrap_or_else(|| ctx.request_model.clone());
            let status_code = status.as_u16();
            let start_time = ctx.start_time;
            let session_id = ctx.session_id.clone();
            // 用 ctx 的 app_type：Claude Desktop 网关也走此转换路径，硬编码
            // "claude" 会把 claude-desktop 的行错记到 claude 名下
            let app_type_str = ctx.app_type_str;

            Some(SseUsageCollector::new(
                start_time,
                Some(claude_stream_usage_event_filter),
                move |events, first_token_ms| {
                    if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                        let model = usage
                            .model
                            .clone()
                            .filter(|m| !m.is_empty())
                            .unwrap_or_else(|| fallback_model.clone());
                        let latency_ms = start_time.elapsed().as_millis() as u64;
                        let state = state.clone();
                        let provider_id = provider_id.clone();
                        let session_id = session_id.clone();
                        let request_model = request_model.clone();
                        let outbound_model = fallback_model.clone();

                        tokio::spawn(async move {
                            log_usage(
                                &state,
                                &provider_id,
                                app_type_str,
                                &model,
                                &request_model,
                                &outbound_model,
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
        match serde_json::from_slice(&body_bytes) {
            Ok(value) => value,
            // 兜底嗅探（#2234）：部分网关对 stream:false 强制返回 SSE 体，却把
            // Content-Type 标成 application/json 等，is_sse() 的 header 检查失效。
            // 此时按 SSE 聚合成单个 JSON 再走既有非流转换器，客户端仍收到
            // Anthropic JSON，非流语义不变。gemini_native 暂无聚合器，落诊断错误。
            Err(_) if body_looks_like_sse(&body_str) && api_format != "gemini_native" => {
                log::warn!(
                    "[Claude] 上游对非流请求返回未标记的 SSE 体（api_format={api_format}），按 SSE 聚合兜底"
                );
                let aggregated = if api_format == "openai_responses" {
                    responses_sse_to_response_value(&body_str)
                } else {
                    chat_sse_to_response_value(&body_str)
                };
                // 聚合也失败时：保留全量 body 服务端日志，并给客户端错误附带同款
                // 现场诊断（content-type/body 摘要），否则命中嗅探臂的用户只拿到
                // 裸聚合错误、丢失非嗅探臂已有的诊断增强（C7）
                aggregated.map_err(|e| {
                    log::error!("[Claude] SSE 聚合兜底失败: {e}, body: {body_str}");
                    aggregate_fallback_error(e, &response_headers, &body_str)
                })?
            }
            Err(e) => {
                log::error!("[Claude] 解析上游响应失败: {e}, body: {body_str}");
                return Err(upstream_body_parse_error(
                    "Failed to parse upstream response",
                    &e,
                    &response_headers,
                    &body_str,
                ));
            }
        }
    };

    // Preserve raw Responses usage so a post-upstream conversion failure still
    // records the tokens already consumed by the successful upstream request.
    let raw_usage_response = (api_format == "openai_responses").then(|| {
        json!({
            "id": upstream_response.get("id").cloned().unwrap_or(Value::Null),
            "model": upstream_response.get("model").cloned().unwrap_or(Value::Null),
            "usage": transform_responses::build_anthropic_usage_from_responses(
                upstream_response.get("usage")
            )
        })
    });

    // 根据 api_format 选择非流式转换器
    let transform_result = if api_format == "openai_responses" {
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
    };
    let anthropic_response = match transform_result {
        Ok(response) => response,
        Err(error) => {
            log::error!("[Claude] 转换响应失败: {error}");
            if usage_logging_enabled(state) {
                if let Some(log) = raw_usage_response.as_ref().and_then(|response| {
                    prepare_claude_usage_log(ctx, response, status.as_u16(), false)
                }) {
                    // The upstream request already succeeded and consumed tokens. Persist
                    // usage before returning the terminal transform error to the client.
                    write_claude_usage_log(state, log).await;
                }
            }
            return Err(error);
        }
    };

    // 记录使用量
    // 全 0 usage 不落账（对齐 Codex 流式收集器的 skip）：SSE 聚合兜底救回的流
    // 在上游缺 stream_options.include_usage 时没有 usage，写入只会产生无意义空行
    spawn_claude_usage_log(state, ctx, &anthropic_response, status.as_u16(), false);

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

/// Codex 客户端（尤其 Desktop 登录态）可能对请求体启用 zstd 压缩，使得后续
/// `serde_json::from_slice` 直接解析失败。这里在解析前解压，并剥掉已失真的实体头
/// （content-encoding / content-length / transfer-encoding）——转发层会基于解压后的
/// 明文 JSON 重新生成正确的头。
fn decode_codex_request_body(
    headers: &mut axum::http::HeaderMap,
    body_bytes: Bytes,
) -> Result<Bytes, ProxyError> {
    let Some(encoding) = get_content_encoding(headers) else {
        return Ok(body_bytes);
    };

    if !is_supported_content_encoding(&encoding) {
        return Err(ProxyError::InvalidRequest(format!(
            "Unsupported request content-encoding: {encoding}"
        )));
    }

    log::debug!("[Codex] 解压请求体: content-encoding={encoding}");
    let decompressed = match decompress_body(&encoding, &body_bytes) {
        Ok(Some(decompressed)) => decompressed,
        // is_supported_content_encoding 已确保编码受支持，正常不会返回 None；
        // 防御性兜底：宁可报错，也不能把压缩字节当 JSON 透传下去。
        Ok(None) => {
            return Err(ProxyError::InvalidRequest(format!(
                "Unsupported request content-encoding: {encoding}"
            )));
        }
        Err(e) => {
            log::warn!("[Codex] 请求体解压失败 ({encoding}): {e}");
            return Err(ProxyError::InvalidRequest(format!(
                "Failed to decompress request body ({encoding}): {e}"
            )));
        }
    };

    headers.remove(axum::http::header::CONTENT_ENCODING);
    headers.remove(axum::http::header::CONTENT_LENGTH);
    headers.remove(axum::http::header::TRANSFER_ENCODING);

    Ok(Bytes::from(decompressed))
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
    let mut headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body_bytes = decode_codex_request_body(&mut headers, body_bytes)?;
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    let endpoint = endpoint_with_query(&uri, "/chat/completions");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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
    ctx.outbound_model = result.outbound_model.take();
    ctx.provider = result.provider;
    let response = result.response;

    process_response(
        response,
        &ctx,
        &state,
        &OPENAI_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI 透传）
pub async fn handle_responses(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_responses_for_app(state, request, AppType::Codex, "Codex", "codex").await
}

pub async fn handle_grokbuild_responses(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_responses_for_app(
        state,
        request,
        AppType::GrokBuild,
        "Grok Build",
        "grokbuild",
    )
    .await
}

async fn handle_responses_for_app(
    state: ProxyState,
    request: axum::extract::Request,
    app_type: AppType,
    tag: &'static str,
    app_type_str: &'static str,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let mut headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body_bytes = decode_codex_request_body(&mut headers, body_bytes)?;
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, app_type.clone(), tag, app_type_str).await?;
    let endpoint = endpoint_with_query(&uri, "/responses");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &app_type,
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
    ctx.outbound_model = result.outbound_model.take();
    ctx.provider = result.provider;
    let response = result.response;

    if super::providers::should_convert_codex_responses_to_anthropic(&ctx.provider, &endpoint) {
        return handle_codex_anthropic_to_responses_transform(
            response,
            &ctx,
            &state,
            is_stream,
            connection_guard,
            codex_tool_context,
        )
        .await;
    }

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
    handle_responses_compact_for_app(state, request, AppType::Codex, "Codex", "codex").await
}

pub async fn handle_grokbuild_responses_compact(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_responses_compact_for_app(
        state,
        request,
        AppType::GrokBuild,
        "Grok Build",
        "grokbuild",
    )
    .await
}

async fn handle_responses_compact_for_app(
    state: ProxyState,
    request: axum::extract::Request,
    app_type: AppType,
    tag: &'static str,
    app_type_str: &'static str,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let mut headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body_bytes = decode_codex_request_body(&mut headers, body_bytes)?;
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, app_type.clone(), tag, app_type_str).await?;
    let endpoint = endpoint_with_query(&uri, "/responses/compact");

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let codex_tool_context = transform_codex_chat::build_codex_tool_context_from_request(&body);

    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &app_type,
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
    ctx.outbound_model = result.outbound_model.take();
    ctx.provider = result.provider;
    let response = result.response;

    if super::providers::should_convert_codex_responses_to_anthropic(&ctx.provider, &endpoint) {
        return handle_codex_anthropic_to_responses_transform(
            response,
            &ctx,
            &state,
            is_stream,
            connection_guard,
            codex_tool_context,
        )
        .await;
    }

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
            // 接管/模型覆写场景的归因兜底：出站真值优先于客户端请求别名
            let fallback_model = ctx
                .outbound_model
                .clone()
                .unwrap_or_else(|| ctx.request_model.clone());
            let app_type_str = ctx.app_type_str;
            let start_time = ctx.start_time;
            let session_id = ctx.session_id.clone();

            Some(SseUsageCollector::new(
                start_time,
                Some(codex_stream_usage_event_filter),
                move |events, first_token_ms| {
                    let usage =
                        TokenUsage::from_codex_stream_events_auto(&events).unwrap_or_default();
                    // 上游遵守 OpenAI 语义省略 usage 时，Chat→Responses 转换器会合成一个
                    // 全 0 的 response.completed，from_codex_response 对 input/output 字段
                    // 存在（哪怕=0）即返回 Some。缺 nonzero 闸门会让全 0 usage 也被写入：
                    // message_id=None → dedup_request_id 退化为随机 UUID，无法去重，每笔
                    // 请求插入一条无意义空行、虚增请求数。对齐 Claude transform handler 的 skip。
                    if !usage.has_billable_tokens() {
                        log::debug!("[Codex] 流式响应 usage 全 0 或缺失，跳过消费记录");
                        return;
                    }
                    let model = usage
                        .model
                        .clone()
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| fallback_model.clone());
                    let latency_ms = start_time.elapsed().as_millis() as u64;

                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    let request_model = request_model.clone();
                    let outbound_model = fallback_model.clone();
                    let session_id = session_id.clone();

                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            app_type_str,
                            &model,
                            &request_model,
                            &outbound_model,
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
    let chat_response: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        // 与 Claude 侧 handle_claude_transform 对称的兜底嗅探（#2234）：
        // 上游对 stream:false 返回未标记 Content-Type 的 SSE 体时按 SSE 聚合。
        Err(_) if body_looks_like_sse(&body_str) => {
            log::warn!("[Codex] 上游对非流请求返回未标记的 SSE 体，按 Chat SSE 聚合兜底");
            // 聚合也失败时：保留全量 body 服务端日志，并给客户端错误附带现场诊断（C7）
            chat_sse_to_response_value(&body_str).map_err(|e| {
                log::error!("[Codex] SSE 聚合兜底失败: {e}, body: {body_str}");
                aggregate_fallback_error(e, &response_headers, &body_str)
            })?
        }
        Err(e) => {
            log::error!("[Codex] 解析 Chat 上游响应失败: {e}, body: {body_str}");
            return Err(upstream_body_parse_error(
                "Failed to parse upstream chat response",
                &e,
                &response_headers,
                &body_str,
            ));
        }
    };
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

    // 上游非流式 Chat 省略 usage 时，chat_usage_to_responses_usage 会合成全 0 usage
    // (transform_codex_chat.rs:1581)，from_codex_response 对 input/output 字段存在(哪怕=0)
    // 即返回 Some。用 has_billable_tokens 闸门跳过全 0，避免空行虚增请求数——与流式分支
    // 及 Claude transform handler 的 skip 行为对齐。
    if let Some(usage) = TokenUsage::from_codex_response_auto(&responses_response)
        .filter(TokenUsage::has_billable_tokens)
    {
        let model = responses_response
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .or_else(|| ctx.outbound_model.clone())
            .unwrap_or_else(|| ctx.request_model.clone());
        let request_model = ctx.request_model.clone();
        let outbound_model = ctx
            .outbound_model
            .clone()
            .unwrap_or_else(|| ctx.request_model.clone());
        let app_type_str = ctx.app_type_str;
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let session_id = ctx.session_id.clone();
            let latency_ms = ctx.latency_ms();
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    &request_model,
                    &outbound_model,
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

/// Response-transform handler for the Codex (Responses) ↔ Anthropic Messages gateway.
///
/// Parallel to `handle_codex_chat_to_responses_transform`: the upstream speaks
/// Anthropic Messages, and this converts the response back into the Responses form
/// Codex expects (both streaming and non-streaming). Error bodies reuse
/// `handle_codex_chat_error_response` (whose extraction logic also works for
/// Anthropic's `{"error":{type,message}}`). It does not involve codex_chat_history
/// (tool ids round-trip natively through Anthropic).
async fn handle_codex_anthropic_to_responses_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    is_stream: bool,
    connection_guard: Option<ActiveConnectionGuard>,
    codex_tool_context: transform_codex_chat::CodexToolContext,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if !status.is_success() {
        return handle_codex_chat_error_response(response, ctx, status).await;
    }

    // Preserve live streaming when the gateway marks SSE correctly or omits an
    // explicit JSON media type. Explicit JSON is buffered below so 2xx error
    // envelopes and gateways that ignore stream:true can be converted faithfully.
    if response.is_sse() || (is_stream && !response.is_json()) {
        let stream = response.bytes_stream();
        let sse_stream =
            create_responses_sse_stream_from_anthropic_with_context(stream, codex_tool_context);
        return build_codex_anthropic_sse_response(
            sse_stream,
            ctx,
            state,
            status,
            connection_guard,
        );
    }

    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    let body_str = String::from_utf8_lossy(&body_bytes);
    let anthropic_response: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        // Fallback sniffing symmetric to the chat / claude side (#2234): when the
        // upstream returns an Anthropic SSE body with an unmarked Content-Type,
        // aggregate it back into a message before continuing the conversion.
        Err(_) if body_looks_like_sse(&body_str) => {
            log::warn!("[Codex] Upstream returned an unmarked Anthropic SSE body, falling back to aggregation");
            transform_codex_anthropic::anthropic_sse_to_message_value(&body_str).map_err(|e| {
                log::error!("[Codex] Failed to aggregate Anthropic SSE body: {e}");
                e
            })?
        }
        Err(e) => {
            log::error!(
                "[Codex] Failed to parse Anthropic upstream response: {e}, body: {body_str}"
            );
            return Err(upstream_body_parse_error(
                "Failed to parse upstream anthropic response",
                &e,
                &response_headers,
                &body_str,
            ));
        }
    };

    if is_stream {
        let events =
            responses_sse_events_from_anthropic_message(&anthropic_response, codex_tool_context);
        let sse_stream = futures::stream::iter(events.into_iter().map(Ok::<Bytes, std::io::Error>));
        return build_codex_anthropic_sse_response(
            sse_stream,
            ctx,
            state,
            status,
            connection_guard,
        );
    }

    let _connection_guard = connection_guard;
    let responses_response =
        transform_codex_anthropic::anthropic_response_to_responses_with_context(
            anthropic_response,
            &codex_tool_context,
        )
        .map_err(|e| {
            log::error!("[Codex] Failed to convert Anthropic response to Responses: {e}");
            e
        })?;

    if let Some(usage) = TokenUsage::from_codex_response_auto(&responses_response)
        .filter(TokenUsage::has_billable_tokens)
    {
        let model = responses_response
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .or_else(|| ctx.outbound_model.clone())
            .unwrap_or_else(|| ctx.request_model.clone());
        let request_model = ctx.request_model.clone();
        let outbound_model = ctx
            .outbound_model
            .clone()
            .unwrap_or_else(|| ctx.request_model.clone());
        let app_type_str = ctx.app_type_str;
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let session_id = ctx.session_id.clone();
            let latency_ms = ctx.latency_ms();
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    &request_model,
                    &outbound_model,
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

    let response_body = serde_json::to_vec(&responses_response).map_err(|e| {
        log::error!("[Codex] Failed to serialize Responses response: {e}");
        ProxyError::TransformError(format!("Failed to serialize responses response: {e}"))
    })?;

    builder
        .body(axum::body::Body::from(response_body))
        .map_err(|e| {
            log::error!("[Codex] Failed to build Responses response: {e}");
            ProxyError::Internal(format!("Failed to build response: {e}"))
        })
}

fn build_codex_anthropic_sse_response(
    sse_stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    ctx: &RequestContext,
    state: &ProxyState,
    status: StatusCode,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<axum::response::Response, ProxyError> {
    let usage_collector = if usage_logging_enabled(state) {
        let state = state.clone();
        let provider_id = ctx.provider.id.clone();
        let request_model = ctx.request_model.clone();
        let fallback_model = ctx
            .outbound_model
            .clone()
            .unwrap_or_else(|| ctx.request_model.clone());
        let app_type_str = ctx.app_type_str;
        let start_time = ctx.start_time;
        let session_id = ctx.session_id.clone();

        Some(SseUsageCollector::new(
            start_time,
            Some(codex_stream_usage_event_filter),
            move |events, first_token_ms| {
                let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap_or_default();
                if !usage.has_billable_tokens() {
                    log::debug!("[Codex] Anthropic streaming response usage is all-zero or missing, skipping usage recording");
                    return;
                }
                let model = usage
                    .model
                    .clone()
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| fallback_model.clone());
                let latency_ms = start_time.elapsed().as_millis() as u64;

                let state = state.clone();
                let provider_id = provider_id.clone();
                let request_model = request_model.clone();
                let outbound_model = fallback_model.clone();
                let session_id = session_id.clone();

                tokio::spawn(async move {
                    log_usage(
                        &state,
                        &provider_id,
                        app_type_str,
                        &model,
                        &request_model,
                        &outbound_model,
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
    Ok((headers, body).into_response())
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
    ctx.outbound_model = result.outbound_model.take();
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
    let mut buffer = body.trim_start_matches('\u{feff}').to_string();
    let mut completed_response: Option<Value> = None;
    let mut output_items = Vec::new();

    // strict=false 用于残余尾块：截断的半截 JSON 忽略而非报错，避免破坏
    // 已聚合好的完整响应（codex_oauth 聚合路径也复用本函数）
    let mut process_block = |block: &str, strict: bool| -> Result<(), ProxyError> {
        // 残余尾块（strict=false）在已拿到 completed 后整体跳过——codex_oauth 聚合
        // 路径也复用本函数，已完成后再执行残余里的完整 response.failed/杂事件会把
        // 成功响应翻成 422（C8）。
        if !strict && completed_response.is_some() {
            return Ok(());
        }
        let mut event_name = "";
        let mut data_lines: Vec<&str> = Vec::new();

        for line in block.lines() {
            let line = line.trim_start();
            if let Some(evt) = strip_sse_field(line, "event") {
                event_name = evt.trim();
            } else if let Some(d) = strip_sse_field(line, "data") {
                data_lines.push(d);
            }
        }

        if data_lines.is_empty() {
            return Ok(());
        }

        let data_str = data_lines.join("\n");
        if data_str.trim() == "[DONE]" {
            return Ok(());
        }

        let data: Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(_) if !strict => return Ok(()),
            Err(e) => {
                return Err(ProxyError::TransformError(format!(
                    "Failed to parse upstream SSE event: {e}"
                )))
            }
        };

        match event_name {
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
        Ok(())
    };

    while let Some(block) = take_sse_block(&mut buffer) {
        process_block(&block, true)?;
    }
    // 最后一个事件后可能没有空行分隔（错标 SSE 兜底/非规范上游常见）：
    // 残余 buffer 当最后一块处理，否则尾部的 response.completed 会被丢掉。
    // 已完成时的跳过判定在闭包内（C8）。
    process_block(&buffer, false)?;

    let mut response = completed_response.ok_or_else(|| {
        ProxyError::TransformError("No response.completed event in upstream SSE".to_string())
    })?;

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

/// 判断响应体是否"看起来像" SSE 文本（#2234 兜底嗅探）。
///
/// 仅在 JSON 解析已失败后调用：合法 JSON 不可能以这些前缀开头，误判面为零。
/// 覆盖 SSE 规范的全部四种字段行；包含 ":" 是因为 OpenRouter 等会在流前发
/// `: PROCESSING` 注释行。
fn body_looks_like_sse(body: &str) -> bool {
    let trimmed = body.trim_start_matches('\u{feff}').trim_start();
    ["data:", "event:", "id:", "retry:", ":"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

/// 构造带现场诊断的上游解析错误：附 content-type / content-encoding 与 body
/// 前缀摘要，让客户端收到的报错自带根因判别（"data:"=错标 SSE、"<"=HTML
/// 拦截页、� 乱码=未解压二进制），不再依赖向用户索要服务端日志。
fn upstream_body_parse_error(
    prefix: &str,
    err: &serde_json::Error,
    headers: &axum::http::HeaderMap,
    body: &str,
) -> ProxyError {
    ProxyError::TransformError(format!(
        "{prefix}: {err} {}",
        body_diagnostics_suffix(headers, body)
    ))
}

/// SSE 聚合兜底失败时，给聚合器内部错误附加同款现场诊断（content-type/
/// content-encoding/body 摘要），使命中 #2234 嗅探臂的客户端也拿到根因线索，
/// 而非仅 "No chat completion choices in upstream SSE" 这类无 header/body 的裸消息。
fn aggregate_fallback_error(
    err: ProxyError,
    headers: &axum::http::HeaderMap,
    body: &str,
) -> ProxyError {
    let base = match &err {
        ProxyError::TransformError(m) => m.clone(),
        other => other.to_string(),
    };
    ProxyError::TransformError(format!("{base} {}", body_diagnostics_suffix(headers, body)))
}

/// 现场诊断后缀：content-type、content-encoding 与 body 前 120 字符摘要。
fn body_diagnostics_suffix(headers: &axum::http::HeaderMap, body: &str) -> String {
    let header_str = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("<none>")
    };
    format!(
        "(content-type: {}; content-encoding: {}; body[..120]: '{}')",
        header_str("content-type"),
        header_str("content-encoding"),
        body_snippet(body, 120),
    )
}

/// 从 SSE chunk 的 error 字段提取可报告的错误消息。占位形状（空对象、空消息、
/// false、空字符串等，常见于 OpenAI 兼容网关每 chunk 附带的 error 字段）返回
/// None——不应据此判定整条流失败（否则会把成功流误杀成 422，C12/C2234 目标人群）。
fn error_event_message(error: &Value) -> Option<String> {
    if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
        return (!msg.is_empty()).then(|| msg.to_string());
    }
    if let Some(s) = error.as_str() {
        return (!s.is_empty()).then(|| s.to_string());
    }
    None
}

/// 取 body 前 `max_chars` 个字符的单行摘要：\r 丢弃、\n 折叠为字面 \n、
/// 其余控制字符替换为 �，超长加省略号。
fn body_snippet(body: &str, max_chars: usize) -> String {
    let mut snippet = String::new();
    for c in body.chars().take(max_chars) {
        match c {
            '\n' => snippet.push_str("\\n"),
            '\r' => {}
            c if c.is_control() => snippet.push('\u{FFFD}'),
            c => snippet.push(c),
        }
    }
    if body.chars().nth(max_chars).is_some() {
        snippet.push('…');
    }
    snippet
}

/// 解析单个 SSE 块的 event 名与 data 负载（多行 data 按规范以 \n 连接）。
/// 行首允许前导空白后再匹配字段名——与 body_looks_like_sse 的 trim 宽容度对齐，
/// 否则缩进的 `  data:` 行被嗅探接受却在此静默丢失（C4）。返回 None 表示无 data 行。
fn sse_block_parts(block: &str) -> Option<(String, String)> {
    let mut event_name = String::new();
    let mut data_lines: Vec<&str> = Vec::new();
    for line in block.lines() {
        let line = line.trim_start();
        if let Some(evt) = strip_sse_field(line, "event") {
            event_name = evt.trim().to_string();
        } else if let Some(d) = strip_sse_field(line, "data") {
            data_lines.push(d);
        }
    }
    (!data_lines.is_empty()).then(|| (event_name, data_lines.join("\n")))
}

/// 把 Chat Completions 流式 SSE 聚合为单个 chat.completion JSON（#2234 兜底）。
///
/// 专供非流式分支使用：上游对 stream:false 返回了 SSE 体但 Content-Type 没标
/// text/event-stream，header 检查（is_sse）失效。聚合后喂给既有非流转换器
/// （Claude 侧 openai_to_anthropic、Codex 侧 chat_completion_to_response_with_context），
/// 客户端拿到的仍是合法 JSON，非流语义不变。
/// 增量合并语义与 providers/streaming.rs 对齐：tool_calls 按 delta.index 定位，
/// id/name 出现即覆盖、arguments 字符串拼接；reasoning 各形态（reasoning_content /
/// reasoning / reasoning_details）经 codex_chat_common 公共提取器并入同一累加器；
/// finish_reason 首个非 null 即锁定（kimi-k2.6 会在 tool_use 后再发带
/// finish_reason 的尾块，见 streaming.rs）。
fn chat_sse_to_response_value(body: &str) -> Result<Value, ProxyError> {
    // 剥 BOM：嗅探器接受 BOM 开头，但 strip_sse_field 按行首精确匹配，
    // 不剥会让首个 data 行静默丢失
    let mut buffer = body.trim_start_matches('\u{feff}').to_string();

    let mut id = Value::Null;
    let mut created = Value::Null;
    let mut model = Value::Null;
    let mut content = String::new();
    let mut reasoning_content = String::new();
    // tool_calls 以 BTreeMap 按 index 聚合：上游可控的 index（u64）不会 densify
    // 数组——旧的 `while len() <= index { push }` 写法遇到 index=4e9 会 OOM 整个
    // 进程（C1）。BTreeMap 既免去无界分配，又天然保持 index 有序输出。
    let mut tool_calls: std::collections::BTreeMap<usize, Value> =
        std::collections::BTreeMap::new();
    let mut finish_reason = Value::Null;
    let mut usage = Value::Null;
    let mut saw_choice = false;
    let mut saw_done = false;

    // strict=false 用于残余尾块：截断的半截 JSON 忽略而非报错，与
    // responses_sse_to_response_value 的残余处理对称（C2），否则一个被掐断的
    // 尾块会把已聚合完整的响应误杀成 422。
    let mut process_event =
        |event_name: &str, data_str: &str, strict: bool| -> Result<(), ProxyError> {
            let trimmed = data_str.trim();
            if trimmed == "[DONE]" {
                saw_done = true;
                return Ok(());
            }
            if trimmed.is_empty() {
                return Ok(());
            }
            let chunk: Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) if !strict => return Ok(()),
                Err(e) => {
                    return Err(ProxyError::TransformError(format!(
                        "Failed to parse upstream SSE chunk: {e}"
                    )))
                }
            };

            // `event: error` 事件：错误由事件名标记，data 体未必有 error 键（直接是
            // 错误对象）。即便此前已聚合完整 choice 也要据此判失败，否则会把网关的
            // 配额/限流错误伪装成成功（C18）。
            if event_name.eq_ignore_ascii_case("error") {
                let message = chunk
                    .get("error")
                    .and_then(error_event_message)
                    .or_else(|| error_event_message(&chunk))
                    .unwrap_or_else(|| "upstream error event in SSE stream".to_string());
                return Err(ProxyError::TransformError(message));
            }
            // 网关把错误作为普通 data chunk 下发（{"error":{...}}）：仅在 error 含
            // 可报告消息时判失败。空对象 / 空消息 / null / false 等占位形状（部分
            // OpenAI 兼容网关每 chunk 都带）不能据此误杀成功流（C12）。
            if let Some(message) = chunk
                .get("error")
                .filter(|e| !e.is_null())
                .and_then(error_event_message)
            {
                return Err(ProxyError::TransformError(message));
            }

            // 首个"有意义"的值锁定 envelope。Azure 的 content-filter 前置块带
            // ""/0 占位（streaming.rs 有同款空串守卫），不能让占位值冻结字段
            for (slot, key) in [
                (&mut id, "id"),
                (&mut created, "created"),
                (&mut model, "model"),
            ] {
                if slot.is_null() {
                    if let Some(v) = chunk.get(key).filter(|v| envelope_value_meaningful(v)) {
                        *slot = v.clone();
                    }
                }
            }
            // OpenAI 语义：usage 只在最终 chunk 非 null
            if let Some(u) = chunk.get("usage").filter(|u| !u.is_null()) {
                usage = u.clone();
            }

            // 代理上下文只存在单选择（n=1），仅聚合 index==0 的 choice
            let Some(choice) = chunk
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|ch| ch.get("index").and_then(|i| i.as_u64()).unwrap_or(0) == 0)
                })
            else {
                return Ok(());
            };

            // "见过响应"的证据必须是 choice payload：metadata/usage-only chunk +
            // [DONE] 的流（全程无 choice）若也算数，会绕过下方两道守卫、
            // 包装出空内容假成功
            saw_choice = true;

            // finish_reason 首个非 null 即锁定（对齐 streaming.rs 的 first-wins：
            // 多 finish_reason 上游的尾块 "stop" 不能覆盖先到的 "tool_calls"）
            if finish_reason.is_null() {
                if let Some(fr) = choice.get("finish_reason").filter(|v| !v.is_null()) {
                    finish_reason = fr.clone();
                }
            }
            // payload 选择：正常增量走 delta；但假流式中转会把完整 chat.completion
            // 包成单事件（message 而非 delta），有的还附带空 delta:{}。delta 为空对象
            // 且存在 message 时改用 message 快照（覆盖此前累计的增量，防混合形态双计），
            // 否则内容被静默丢弃、完成性守卫又被其 finish_reason 击穿 → 空内容假成功（C3）。
            let delta_nonempty = choice
                .get("delta")
                .and_then(|d| d.as_object())
                .is_some_and(|o| !o.is_empty());
            let (payload, is_full_message) = if delta_nonempty {
                (choice.get("delta").unwrap(), false)
            } else if let Some(message) = choice.get("message") {
                (message, true)
            } else if let Some(delta) = choice.get("delta") {
                // 空 delta 且无 message：正常的纯 finish_reason 收尾块
                (delta, false)
            } else {
                return Ok(());
            };
            if is_full_message {
                content.clear();
                reasoning_content.clear();
                tool_calls.clear();
            }
            match payload.get("content") {
                Some(Value::String(text)) => content.push_str(text),
                Some(Value::Array(parts)) => {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            content.push_str(text);
                        } else if let Some(refusal) = part.get("refusal").and_then(|r| r.as_str()) {
                            content.push_str(refusal);
                        }
                    }
                }
                _ => {}
            }
            // refusal：OpenAI 官方拒绝形态（delta.refusal / message.refusal 字符串）。
            // 两个下游转换器都把 refusal 当可见内容，漏读会让拒绝响应变空消息假成功（C15）。
            if let Some(refusal) = payload.get("refusal").and_then(|r| r.as_str()) {
                content.push_str(refusal);
            }
            // reasoning 字段穷举提取直接复用 codex_chat_common（reasoning_content >
            // reasoning 字符串/对象 > reasoning_details），避免第三份手写实现漏档：
            // MiMo/OpenRouter 等只发 reasoning_details 的 provider 否则会丢思考内容
            if let Some(text) = extract_reasoning_field_text(payload) {
                reasoning_content.push_str(&text);
            }
            if let Some(deltas) = payload.get("tool_calls").and_then(|t| t.as_array()) {
                for (pos, tc) in deltas.iter().enumerate() {
                    merge_tool_call_delta(&mut tool_calls, tc, pos);
                }
            } else if let Some(fc) = payload.get("function_call").filter(|v| !v.is_null()) {
                // legacy function_call（2023 弃用但仍有中转回传）→ 当单个 tool_call。
                // 两个下游转换器都支持 function_call，漏读会让 finish_reason
                // "function_call"→stop_reason "tool_use" 却零工具块、卡死 agent 循环（C17）。
                let synthetic = json!({
                    "index": 0,
                    "id": fc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": "function",
                    "function": fc,
                });
                merge_tool_call_delta(&mut tool_calls, &synthetic, 0);
            }
            Ok(())
        };

    while let Some(block) = take_sse_block(&mut buffer) {
        if let Some((event, data)) = sse_block_parts(&block) {
            process_event(&event, &data, true)?;
        }
    }
    // 最后一个事件后可能没有空行分隔（半截流/非规范上游）：残余 buffer 当最后一块
    // 处理，strict=false 容忍被掐断的尾块（C2）。
    if let Some((event, data)) = sse_block_parts(&buffer) {
        process_event(&event, &data, false)?;
    }

    if !saw_choice {
        return Err(ProxyError::TransformError(
            "No chat completion choices in upstream SSE".to_string(),
        ));
    }
    // 完成性守卫：close-delimited 响应的中途截断在字节层不可检测，缺少
    // finish_reason 与 [DONE] 两个完成证据时按截断处理，避免把半截内容
    // 包装成"看起来成功"的响应静默返回（比 422 更难诊断的失败形态）。
    if finish_reason.is_null() && !saw_done {
        return Err(ProxyError::TransformError(
            "Upstream SSE stream appears truncated (no finish_reason or [DONE] marker)".to_string(),
        ));
    }

    // tool_calls 终结化：全空壳（index 空洞或未收到任何字段）直接丢弃（避免幽灵
    // tool_use）；缺 id/name 的按原始 index 回填合成值（对齐 streaming.rs 的
    // tool_call_{idx}/unknown_tool）——空 id 会破坏 Claude 的 tool_use_id ↔
    // tool_result 回程
    let tool_calls: Vec<Value> = tool_calls
        .into_iter()
        .filter(|(_, tc)| {
            tc["id"].as_str().is_some_and(|s| !s.is_empty())
                || tc["function"]["name"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
                || tc["function"]["arguments"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
        })
        .map(|(index, mut tc)| {
            if tc["id"].as_str().is_none_or(str::is_empty) {
                tc["id"] = json!(format!("tool_call_{index}"));
            }
            if tc["function"]["name"].as_str().is_none_or(str::is_empty) {
                tc["function"]["name"] = json!("unknown_tool");
            }
            tc
        })
        .collect();

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), json!("assistant"));
    message.insert("content".to_string(), json!(content));
    if !reasoning_content.is_empty() {
        message.insert("reasoning_content".to_string(), json!(reasoning_content));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    // 上游未回传有效 id 时合成 UUID：留 null/"" 会让下游 dedup_request_id 退化为
    // 常量 "session:" 全局碰撞，INSERT OR REPLACE 静默覆盖前序 usage 行、少计成本（C9）。
    let id = if envelope_value_meaningful(&id) {
        id
    } else {
        json!(uuid::Uuid::new_v4().to_string())
    };

    let mut response = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
    });
    if !usage.is_null() {
        response["usage"] = usage;
    }
    Ok(response)
}

/// envelope 字段是否"有意义"：过滤 null、空串与数值 0（含浮点 0.0——Azure
/// content-filter 前置块的占位值），避免占位值抢先冻结 id/model/created。
fn envelope_value_meaningful(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
        _ => true,
    }
}

/// 合并单条 tool_calls 增量到按 index 聚合的 BTreeMap：OpenAI 流式把 id/name 放
/// 首个增量、arguments 分片下发，按 delta.index 定位目标；缺 index 时退到所在数组
/// 中的位置（message 形态的完整 tool_calls 常不带 index，按 0 会互相覆盖）。
fn merge_tool_call_delta(
    tool_calls: &mut std::collections::BTreeMap<usize, Value>,
    delta: &Value,
    fallback_index: usize,
) {
    let index = delta
        .get("index")
        .and_then(|i| i.as_u64())
        .map(|i| i as usize)
        .unwrap_or(fallback_index);
    let target = tool_calls.entry(index).or_insert_with(|| {
        json!({
            "id": "",
            "type": "function",
            "function": {"name": "", "arguments": ""}
        })
    });
    if let Some(v) = delta
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        target["id"] = json!(v);
    }
    if let Some(func) = delta.get("function") {
        if let Some(name) = func
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            target["function"]["name"] = json!(name);
        }
        // arguments：string 直接拼接；object/array 序列化后拼接——非流 message
        // 快照常把 arguments 作对象回传（OpenAI 兼容偏差），只认 string 会丢参数
        // 致工具空输入执行（C16）
        match func.get("arguments") {
            Some(Value::String(args)) => {
                if let Some(existing) = target["function"]["arguments"].as_str() {
                    target["function"]["arguments"] = json!(format!("{existing}{args}"));
                }
            }
            Some(v @ (Value::Object(_) | Value::Array(_))) => {
                let serialized = serde_json::to_string(v).unwrap_or_default();
                if let Some(existing) = target["function"]["arguments"].as_str() {
                    target["function"]["arguments"] = json!(format!("{existing}{serialized}"));
                }
            }
            _ => {}
        }
    }
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
///
/// `outbound_model` 是「按请求计价」模式的锚点：实际发往上游的模型
/// （路由接管映射后的真值，无映射时等于 request_model）。
#[allow(clippy::too_many_arguments)]
async fn log_usage(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    request_model: &str,
    outbound_model: &str,
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
        outbound_model
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
        body_looks_like_sse, body_snippet, chat_sse_to_response_value, codex_proxy_error_json,
        responses_sse_to_response_value, should_use_claude_transform_streaming, transform,
        upstream_body_parse_error,
    };
    use crate::proxy::ProxyError;

    #[test]
    fn body_looks_like_sse_detects_unlabeled_sse_prefixes() {
        assert!(body_looks_like_sse("data: {\"id\":\"1\"}\n\n"));
        assert!(body_looks_like_sse("event: message\ndata: {}\n\n"));
        // SSE 规范的另两种字段行也可能打头
        assert!(body_looks_like_sse("id: 1\ndata: {}\n\n"));
        assert!(body_looks_like_sse("retry: 3000\ndata: {}\n\n"));
        // OpenRouter 会在流前发注释行
        assert!(body_looks_like_sse(
            ": OPENROUTER PROCESSING\n\ndata: {}\n\n"
        ));
        // BOM + 前导空白
        assert!(body_looks_like_sse("\u{feff}\n  data: {}\n\n"));
        // HTML 拦截页与普通文本不应误判为 SSE
        assert!(!body_looks_like_sse("<html><body>blocked</body></html>"));
        assert!(!body_looks_like_sse("Bad Gateway"));
        assert!(!body_looks_like_sse(""));
    }

    #[test]
    fn upstream_body_parse_error_carries_field_diagnostics() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("content-type", "text/html".parse().unwrap());
        headers.insert("content-encoding", "gzip".parse().unwrap());
        let parse_err = serde_json::from_str::<serde_json::Value>("<html>").unwrap_err();

        let err = upstream_body_parse_error(
            "Failed to parse upstream response",
            &parse_err,
            &headers,
            "<html>\nblocked</html>",
        );

        match err {
            ProxyError::TransformError(msg) => {
                assert!(msg.contains("content-type: text/html"), "{msg}");
                assert!(msg.contains("content-encoding: gzip"), "{msg}");
                assert!(msg.contains("<html>\\nblocked</html>"), "{msg}");
            }
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn upstream_body_parse_error_marks_missing_headers() {
        let headers = axum::http::HeaderMap::new();
        let parse_err = serde_json::from_str::<serde_json::Value>("data:").unwrap_err();

        let err = upstream_body_parse_error("x", &parse_err, &headers, "data: oops");

        match err {
            ProxyError::TransformError(msg) => {
                assert!(msg.contains("content-type: <none>"), "{msg}");
                assert!(msg.contains("content-encoding: <none>"), "{msg}");
            }
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_collects_reasoning_alias() {
        // OpenRouter/Kimi 用 reasoning（字符串），部分网关用对象形态
        let sse = "data: {\"id\":\"c1\",\"model\":\"kimi-k2.6\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"think\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":{\"content\":\"ing\"},\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(
            response["choices"][0]["message"]["reasoning_content"],
            "thinking"
        );
        assert_eq!(response["choices"][0]["message"]["content"], "ok");
    }

    #[test]
    fn chat_sse_to_response_value_collects_reasoning_details() {
        // MiMo/OpenRouter 等只发 reasoning_details（数组形态）的 provider，
        // 经公共提取器兜底，不能丢思考内容
        let sse = "data: {\"id\":\"c1\",\"model\":\"mimo\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"think\"}]},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"ing\"}],\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(
            response["choices"][0]["message"]["reasoning_content"],
            "thinking"
        );
        assert_eq!(response["choices"][0]["message"]["content"], "ok");
    }

    #[test]
    fn responses_sse_to_response_value_handles_missing_trailing_blank_line() {
        // 错标 SSE 兜底/非规范上游：最后的 response.completed 后没有空行分隔
        let sse = "event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tail\",\"status\":\"completed\",\"model\":\"gpt-5.4\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n";

        let response = responses_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "resp_tail");
    }

    #[test]
    fn responses_sse_to_response_value_ignores_truncated_trailing_block() {
        // 截断的残余尾块不能破坏已聚合好的完整响应（codex_oauth 路径复用本函数）
        let sse = "event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ok\",\"status\":\"completed\",\"model\":\"gpt-5.4\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\
\n\
event: response.extra\n\
data: {\"type\":\"resp";

        let response = responses_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "resp_ok");
    }

    #[test]
    fn chat_sse_to_response_value_skips_azure_placeholder_envelope() {
        // Azure content-filter 前置块带 ""/0 占位，不能冻结 envelope 字段
        let sse = "data: {\"id\":\"\",\"model\":\"\",\"created\":0,\"object\":\"\",\"choices\":[],\"prompt_filter_results\":[]}\n\n\
data: {\"id\":\"chatcmpl-real\",\"model\":\"gpt-5.4\",\"created\":42,\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "chatcmpl-real");
        assert_eq!(response["model"], "gpt-5.4");
        assert_eq!(response["created"], 42);
    }

    #[test]
    fn chat_sse_to_response_value_tolerates_null_error_field() {
        // one-api 系网关每个 chunk 都带 "error": null，不能误判为上游错误
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"error\":null,\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn chat_sse_to_response_value_first_finish_reason_wins() {
        // kimi-k2.6 等会在 tool_use 后再发带 finish_reason 的尾块，
        // 尾块 "stop" 不能覆盖先到的 "tool_calls"（对齐 streaming.rs first-wins）
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn chat_sse_to_response_value_unwraps_message_shaped_fake_stream() {
        // 假流式中转把完整 chat.completion 包成单个 SSE 事件（message 而非 delta）
        let sse = "data: {\"id\":\"c1\",\"object\":\"chat.completion\",\"model\":\"m\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"full answer\"},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "full answer");
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn chat_sse_to_response_value_message_snapshot_overrides_deltas() {
        // 混合形态：先发增量再发完整 message 快照时，快照覆盖增量（防双计）
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"full\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "full");
    }

    #[test]
    fn chat_sse_to_response_value_backfills_sparse_tool_call_ids() {
        // index 空洞的空壳被丢弃；缺 id 的按原始 index 回填 tool_call_{idx}
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"name\":\"f2\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        let tool_calls = response["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 1, "index 0 的空壳应被丢弃");
        assert_eq!(tool_calls[0]["id"], "tool_call_1");
        assert_eq!(tool_calls[0]["function"]["name"], "f2");
    }

    #[test]
    fn chat_sse_to_response_value_strips_bom_before_parsing() {
        // 嗅探器接受 BOM，块解析也必须剥掉它，否则首个 data 行静默丢失
        let sse = "\u{feff}data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn body_snippet_sanitizes_controls_and_truncates() {
        assert_eq!(
            body_snippet("<html>\r\nblocked\u{0}</html>", 120),
            "<html>\\nblocked\u{FFFD}</html>"
        );
        let long = "a".repeat(200);
        let snippet = body_snippet(&long, 120);
        assert_eq!(snippet.chars().count(), 121); // 120 个字符 + 省略号
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn chat_sse_to_response_value_aggregates_text_finish_reason_and_usage() {
        let sse = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12}}\n\n\
data: [DONE]\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["id"], "chatcmpl-1");
        assert_eq!(response["object"], "chat.completion");
        assert_eq!(response["model"], "gpt-5.4");
        assert_eq!(response["choices"][0]["message"]["role"], "assistant");
        assert_eq!(response["choices"][0]["message"]["content"], "Hello");
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
        assert_eq!(response["usage"]["prompt_tokens"], 10);
    }

    #[test]
    fn chat_sse_to_response_value_merges_tool_call_argument_fragments() {
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"SF\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: [DONE]\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        let tool_call = &response["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tool_call["id"], "call_1");
        assert_eq!(tool_call["function"]["name"], "get_weather");
        assert_eq!(tool_call["function"]["arguments"], "{\"city\":\"SF\"}");
        assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn chat_sse_to_response_value_collects_reasoning_content() {
        let sse = "data: {\"id\":\"c1\",\"model\":\"deepseek-r2\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"ing\",\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(
            response["choices"][0]["message"]["reasoning_content"],
            "thinking"
        );
        assert_eq!(response["choices"][0]["message"]["content"], "ok");
    }

    #[test]
    fn chat_sse_to_response_value_handles_missing_trailing_blank_line() {
        // 非规范上游/半截流：最后一个事件后没有空行分隔
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn chat_sse_to_response_value_handles_crlf_delimiters() {
        // 真实 HTTP SSE 按规范使用 \r\n\r\n 分隔事件
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\r\n\
\r\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\
\r\n\
data: [DONE]\r\n\
\r\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "hi");
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn chat_sse_to_response_value_propagates_upstream_error_event() {
        let sse = "data: {\"error\":{\"message\":\"rate limited by gateway\",\"code\":429}}\n\n";

        let err = chat_sse_to_response_value(sse).unwrap_err();
        match err {
            ProxyError::TransformError(msg) => assert!(msg.contains("rate limited by gateway")),
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_rejects_truncated_stream() {
        // 只有内容增量、无 finish_reason 也无 [DONE]：close-delimited 截断不可
        // 在字节层检测，必须按截断报错而非静默返回半截内容
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";

        let err = chat_sse_to_response_value(sse).unwrap_err();
        match err {
            ProxyError::TransformError(msg) => assert!(msg.contains("truncated")),
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_accepts_done_marker_without_finish_reason() {
        // 非规范上游可能不发 finish_reason 但正常收尾 [DONE]：视为完成
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n\
data: [DONE]\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();

        assert_eq!(response["choices"][0]["message"]["content"], "hi");
        assert_eq!(
            response["choices"][0]["finish_reason"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn chat_sse_to_response_value_rejects_stream_without_chunks() {
        let err = chat_sse_to_response_value(": keepalive\n\ndata: [DONE]\n\n").unwrap_err();
        match err {
            ProxyError::TransformError(msg) => {
                assert!(msg.contains("No chat completion choices"))
            }
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_rejects_choiceless_stream_despite_done() {
        // metadata/usage-only chunk + [DONE]、全程无 choice payload：
        // 不能凭 [DONE] 包装成空内容假成功（saw_choice 必须以 choice 为证据）
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":0,\"total_tokens\":1}}\n\n\
data: [DONE]\n\n";

        let err = chat_sse_to_response_value(sse).unwrap_err();
        match err {
            ProxyError::TransformError(msg) => {
                assert!(msg.contains("No chat completion choices"), "{msg}")
            }
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_huge_tool_call_index_does_not_oom() {
        // C1：上游可控的巨大 index 不得 densify 数组（旧实现会 OOM 整个进程）；
        // BTreeMap 只占一个槽，且原始 index 用于回填合成 id
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":4000000000,\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        let tool_calls = response["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "tool_call_4000000000");
        assert_eq!(tool_calls[0]["function"]["name"], "f");
    }

    #[test]
    fn chat_sse_to_response_value_empty_delta_falls_back_to_message_snapshot() {
        // C3：同一 choice 同时带空 delta:{} 与完整 message 快照——不能因 delta 键
        // 存在就短路到空 delta、丢掉 message 内容（finish_reason 还会击穿守卫）
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"message\":{\"role\":\"assistant\",\"content\":\"full answer\"},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "full answer");
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn chat_sse_to_response_value_empty_delta_scaffold_does_not_wipe_real_content() {
        // C3 反向陷阱：每个 chunk 都带真内容 delta + 空 message 壳时，不能让空
        // message 触发 clear 抹掉累计内容（delta 非空则优先 delta，不走快照覆盖）
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"message\":{},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"},\"message\":{},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "hi there");
    }

    #[test]
    fn chat_sse_to_response_value_object_form_tool_arguments_preserved() {
        // C16：message 快照里 arguments 作对象回传时序列化保留，不能丢成空输入
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"tool_calls\":[{\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":{\"city\":\"SF\"}}}]},\"finish_reason\":\"tool_calls\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        let args = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(parsed["city"], "SF");
    }

    #[test]
    fn chat_sse_to_response_value_collects_refusal() {
        // C15：delta.refusal 字符串并入可见内容，避免拒绝响应变空消息假成功
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"refusal\":\"I can't help with that.\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "I can't help with that."
        );
    }

    #[test]
    fn chat_sse_to_response_value_maps_legacy_function_call() {
        // C17：legacy function_call → 单个 tool_call，避免 finish_reason
        // function_call 映射成 tool_use 却零工具块卡死 agent
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":null,\"function_call\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\\\"SF\\\"}\"}},\"finish_reason\":\"function_call\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        let tc = &response["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "get_weather");
        assert_eq!(tc["function"]["arguments"], "{\"city\":\"SF\"}");
    }

    #[test]
    fn chat_sse_to_response_value_event_error_fails_even_after_complete_choice() {
        // C18：event:error（data 无 error 键）即便跟在完整 choice 后也判失败，
        // 不能伪装成成功
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"stop\"}]}\n\n\
event: error\n\
data: {\"message\":\"insufficient_user_quota\",\"code\":429}\n\n";

        let err = chat_sse_to_response_value(sse).unwrap_err();
        match err {
            ProxyError::TransformError(msg) => {
                assert!(msg.contains("insufficient_user_quota"), "{msg}")
            }
            other => panic!("expected TransformError, got {other:?}"),
        }
    }

    #[test]
    fn chat_sse_to_response_value_tolerates_empty_error_placeholder() {
        // C12：error 为空对象 / 空消息等占位形状不得误杀成功流
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"error\":{},\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn chat_sse_to_response_value_tolerates_truncated_residual_after_complete() {
        // C2：完整 finish_reason 块后尾块被掐断（半截 JSON），不能误杀已完整的聚合
        let sse = "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n\
data: {\"usage\":{\"prompt_to";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn chat_sse_to_response_value_float_zero_does_not_freeze_envelope() {
        // C14：浮点 0.0 占位的 created 不得冻结 envelope，真值应能覆盖
        let sse = "data: {\"id\":\"\",\"model\":\"\",\"created\":0.0,\"choices\":[]}\n\n\
data: {\"id\":\"chatcmpl-real\",\"model\":\"m\",\"created\":42,\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["created"], 42);
        assert_eq!(response["id"], "chatcmpl-real");
    }

    #[test]
    fn chat_sse_to_response_value_synthesizes_id_when_absent() {
        // C9：上游无 id 时合成非空唯一 id，避免下游 dedup 退化成常量碰撞覆盖
        let sse = "data: {\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let r1 = chat_sse_to_response_value(sse).unwrap();
        let r2 = chat_sse_to_response_value(sse).unwrap();
        let id1 = r1["id"].as_str().unwrap();
        let id2 = r2["id"].as_str().unwrap();
        assert!(!id1.is_empty());
        assert_ne!(id1, id2, "两次无 id 聚合应产出不同 id 以避免 dedup 碰撞");
    }

    #[test]
    fn chat_sse_to_response_value_accepts_indented_data_lines() {
        // C4：行首缩进的 data 行（嗅探器宽容接受）也应能被聚合，不静默丢失
        let sse = "  data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n";

        let response = chat_sse_to_response_value(sse).unwrap();
        assert_eq!(response["choices"][0]["message"]["content"], "hi");
    }

    #[test]
    fn responses_sse_completed_then_trailing_failed_keeps_success() {
        // C8：已拿到 response.completed 后，残余里的完整 response.failed 不得翻车
        // （codex_oauth 聚合路径复用本函数，此前该尾块被忽略=成功）
        let sse = "event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ok\",\"status\":\"completed\",\"model\":\"gpt-5.4\",\"output\":[]}}\n\n\
event: response.failed\n\
data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"boom\"}}}\n";

        let response = responses_sse_to_response_value(sse).unwrap();
        assert_eq!(response["id"], "resp_ok");
    }

    #[test]
    fn aggregated_chat_sse_round_trips_through_openai_to_anthropic() {
        // 全链路：错标 Content-Type 的 SSE 体 → 聚合 → 既有非流转换器 → Anthropic JSON
        let sse = "data: {\"id\":\"chatcmpl-9\",\"created\":1,\"model\":\"gpt-5.4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"chatcmpl-9\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":1,\"total_tokens\":5}}\n\n\
data: [DONE]\n\n";

        let aggregated = chat_sse_to_response_value(sse).unwrap();
        let anthropic = transform::openai_to_anthropic(aggregated).unwrap();

        assert_eq!(anthropic["model"], "gpt-5.4");
        assert_eq!(anthropic["content"][0]["type"], "text");
        assert_eq!(anthropic["content"][0]["text"], "Hi");
        assert_eq!(anthropic["stop_reason"], "end_turn");
    }

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
}
