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
        deepseek::{DeepSeekAdapter, DeepSeekResponseConverter},
        ProviderAdapter,
        get_adapter, get_claude_api_format, streaming::create_anthropic_sse_stream,
        streaming_gemini::create_anthropic_sse_stream_from_gemini,
        streaming_responses::create_anthropic_sse_stream_from_responses, transform,
        transform_gemini, transform_responses,
    },
    response_processor::{
        create_logged_passthrough_stream, process_response, read_decoded_body,
        strip_entity_headers_for_rebuilt_body, strip_hop_by_hop_response_headers,
        SseUsageCollector,
    },
    server::ProxyState,
    sse::{strip_sse_field, take_sse_block},
    types::*,
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::app_config::AppType;
use crate::provider::Provider;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use bytes::Bytes;
use http_body_util::BodyExt;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

/// Cache for DeepSeek reasoning_content across requests in a conversation.
/// Keyed by response_id (the `id` field of the upstream Responses API response),
/// so reasoning from the previous turn can be looked up when Codex sends
/// `previous_response_id` in the next request (required by DeepSeek thinking mode).
static REASONING_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Secondary cache keyed by DeepSeek tool call IDs.
/// Codex does NOT send `previous_response_id` on tool-call round-trips,
/// but it DOES include `call_id` in `function_call_output` items, which
/// matches the original DeepSeek tool call `id`.  We cache reasoning by
/// each tool call ID so the fallback path can inject it without relying
/// on `previous_response_id`.
static TOOL_CALL_REASONING_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
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
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    original_body: &Value,
    is_stream: bool,
    api_format: &str,
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
    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);

    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
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
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &OPENAI_PARSER_CONFIG).await
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI）
///
/// 自动检测上游供应商，如果是 DeepSeek 则使用 DeepSeek 专用的格式转换
/// （Responses API ↔ Chat Completions），否则走标准 Codex 透传。
pub async fn handle_responses(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
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

    // Detect DeepSeek provider and route to dedicated handler
    let providers = ctx.get_providers();
    if let Some(first_provider) = providers.first() {
        if DeepSeekAdapter::is_deepseek_provider(first_provider) {
            return handle_deepseek_response(
                &state, &ctx, body, headers, first_provider,
            )
            .await;
        }
    }

    let endpoint = endpoint_with_query(&uri, "/responses");

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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG).await
}

/// DeepSeek 专用响应处理器
///
/// 将 Codex 的 Responses API 请求转换为 Chat Completions 格式，
/// 转发到 DeepSeek API，再将 DeepSeek 的 Chat Completions SSE 流
/// 转换为 Responses API SSE 事件返回给 Codex。
async fn handle_deepseek_response(
    _state: &ProxyState,
    _ctx: &RequestContext,
    body: Value,
    _headers: axum::http::HeaderMap,
    provider: &Provider,
) -> Result<axum::response::Response, ProxyError> {
    use axum::http::header;
    use std::time::Duration;

    let adapter = DeepSeekAdapter::new();

    // 1. Extract base URL and auth
    let base_url = adapter.extract_base_url(provider).map_err(|e| {
        ProxyError::ForwardFailed(format!("Failed to get DeepSeek base URL: {e}"))
    })?;
    let auth = adapter.extract_auth(provider).ok_or_else(|| {
        ProxyError::ForwardFailed("No API key configured for DeepSeek provider".to_string())
    })?;
    let api_key = auth.api_key.clone();

    // 2. Inject cached reasoning_content if available.
    // DeepSeek thinking mode requires reasoning_content to be passed back
    // in ALL follow-up requests. We cache reasoning from the previous response
    // keyed by its response_id, then look it up when Codex sends
    // `previous_response_id` in the next request.
    let prev_response_id = body
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 3. Convert request body: Responses API → Chat Completions
    let mut chat_body = adapter.transform_request(body, provider)?;

    // 3b. Inject cached reasoning_content if we're continuing a conversation.
    //     Strategy: find the best assistant message to attach reasoning to.
    //     - If there are tool messages (multi-turn tool calls), attach to the
    //       last assistant BEFORE the first tool message (that's the one that
    //       generated the reasoning originally).
    //     - Otherwise, attach to the last assistant message.
    {
        if let Some(ref prev_id) = prev_response_id {
            log::info!(
                "[DeepSeek] Looking up reasoning cache for prev_response={}",
                &prev_id[..prev_id.len().min(16)]
            );
            if let Ok(cache) = REASONING_CACHE.lock() {
                let has_reasoning = cache.contains_key(prev_id);
                log::info!(
                    "[DeepSeek] Reasoning cache {} for key {}",
                    if has_reasoning { "HIT" } else { "MISS" },
                    &prev_id[..prev_id.len().min(16)]
                );
                if let Some(cached_reasoning) = cache.get(prev_id) {
                    if !cached_reasoning.is_empty() {
                        if let Some(messages) = chat_body
                            .get_mut("messages")
                            .and_then(|m| m.as_array_mut())
                        {
                            // Determine the target assistant message index:
                            // If there are tool messages, find the last assistant
                            // before the first tool message (tool-call round-trip).
                            // Otherwise, use the last assistant message.
                            let first_tool_idx = messages.iter().position(|m| {
                                m.get("role") == Some(&json!("tool"))
                            });
                            let target_idx = if let Some(tool_idx) = first_tool_idx {
                                // Find the last assistant before this tool message
                                messages[..tool_idx]
                                    .iter()
                                    .enumerate()
                                    .rev()
                                    .find(|(_, m)| m.get("role") == Some(&json!("assistant")))
                                    .map(|(i, _)| i)
                            } else {
                                // No tool messages — use the last assistant
                                messages
                                    .iter()
                                    .enumerate()
                                    .rev()
                                    .find(|(_, m)| m.get("role") == Some(&json!("assistant")))
                                    .map(|(i, _)| i)
                            };

                            if let Some(idx) = target_idx {
                                if let Some(assistant_msg) = messages.get_mut(idx) {
                                    // Only inject if not already present
                                    let already_has = assistant_msg
                                        .get("reasoning_content")
                                        .and_then(|v| v.as_str())
                                        .map_or(false, |s| !s.is_empty());
                                    if !already_has {
                                        if let Some(obj) = assistant_msg.as_object_mut() {
                                            log::info!(
                                                "[DeepSeek] Injecting cached reasoning_content ({} chars) into assistant msg [{}] (prev_response={})",
                                                cached_reasoning.len(),
                                                idx,
                                                &prev_id[..prev_id.len().min(16)]
                                            );
                                            obj.insert(
                                                "reasoning_content".to_string(),
                                                json!(cached_reasoning),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3b. Fallback reasoning injection (when previous_response_id is absent/missed).
    //     Codex does not send `previous_response_id` on tool-call round-trips, so
    //     the cache lookup above won't trigger.  Instead, we detect tool messages
    //     in the array and look up reasoning by each tool message's `tool_call_id`
    //     from TOOL_CALL_REASONING_CACHE (populated when the previous response was
    //     processed).
    {
        if let Some(messages) = chat_body
            .get_mut("messages")
            .and_then(|m| m.as_array_mut())
        {
            // Iterate through ALL assistant messages with tool_calls.
            // Each one that lacks reasoning_content needs injection from cache.
            // We match each assistant to its FOLLOWING tool messages so the
            // correct round's reasoning_content is injected.
            let total = messages.len();
            // Build a list of (assistant_idx, Vec<tool_call_ids_for_this_round>)
            let mut rounds: Vec<(usize, Vec<String>)> = Vec::new();
            let mut i = 0;
            while i < total {
                if messages[i].get("role") == Some(&json!("assistant"))
                    && messages[i].get("tool_calls").is_some()
                {
                    let assistant_idx = i;
                    i += 1;
                    // Collect tool messages that follow this assistant
                    let mut tc_ids = Vec::new();
                    while i < total
                        && messages[i].get("role") == Some(&json!("tool"))
                    {
                        if let Some(tc_id) = messages[i]
                            .get("tool_call_id")
                            .and_then(|v| v.as_str())
                        {
                            tc_ids.push(tc_id.to_string());
                        }
                        i += 1;
                    }
                    rounds.push((assistant_idx, tc_ids));
                } else {
                    i += 1;
                }
            }

            for (idx, tc_ids) in &rounds {
                if let Some(assistant_msg) = messages.get_mut(*idx) {
                    let already_has = assistant_msg
                        .get("reasoning_content")
                        .and_then(|v| v.as_str())
                        .map_or(false, |s| !s.is_empty());
                    if already_has {
                        continue;
                    }

                    // Strategy 1: Look up TOOL_CALL_REASONING_CACHE by this round's tool_call_ids
                    let mut injected = false;
                    if !tc_ids.is_empty() {
                        if let Ok(tc_cache) = TOOL_CALL_REASONING_CACHE.lock() {
                            for tc_id in tc_ids {
                                if let Some(cached_reasoning) = tc_cache.get(tc_id) {
                                    if !cached_reasoning.is_empty() {
                                        log::info!(
                                            "[DeepSeek] Fallback: injected cached reasoning_content ({} chars) via tool_call_id {} into msg[{}]",
                                            cached_reasoning.len(),
                                            &tc_id[..tc_id.len().min(12)],
                                            idx
                                        );
                                        if let Some(obj) = assistant_msg.as_object_mut() {
                                            obj.insert(
                                                "reasoning_content".to_string(),
                                                json!(cached_reasoning),
                                            );
                                        }
                                        injected = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Strategy 2: Try ALL entries from REASONING_CACHE as last resort.
                    if !injected {
                        if let Ok(cache) = REASONING_CACHE.lock() {
                            for (_key, cached_reasoning) in cache.iter() {
                                if !cached_reasoning.is_empty() {
                                    log::info!(
                                        "[DeepSeek] Last-resort: injected reasoning from REASONING_CACHE into msg[{}] ({} chars)",
                                        idx,
                                        cached_reasoning.len()
                                    );
                                    if let Some(obj) = assistant_msg.as_object_mut() {
                                        obj.insert(
                                            "reasoning_content".to_string(),
                                            json!(cached_reasoning),
                                        );
                                    }
                                    injected = true;
                                    break;
                                }
                            }
                        }
                    }

                    if !injected {
                        log::warn!(
                            "[DeepSeek] msg[{}]: assistant with tool_calls but NO cached reasoning found to inject",
                            idx
                        );
                    }
                }
            }
        }
    }

    // 3. Debug logging: verify reasoning_content state before sending to DeepSeek
    {
        if let Some(messages) = chat_body.get("messages").and_then(|m| m.as_array()) {
            for (i, msg) in messages.iter().enumerate() {
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let has_tc = msg.get("tool_calls").is_some();
                let rc = msg.get("reasoning_content").and_then(|v| v.as_str()).unwrap_or("");
                let rc_preview = if rc.is_empty() {
                    "MISSING".to_string()
                } else {
                    format!("{} chars", rc.len())
                };
                log::info!(
                    "[DeepSeek] msg[{}]: role={}, has_tool_calls={}, reasoning_content={}",
                    i, role, has_tc, rc_preview
                );
            }
        }
    }
    {
        let tc_cache_size = TOOL_CALL_REASONING_CACHE.lock().map(|c| c.len()).unwrap_or(0);
        let r_cache_size = REASONING_CACHE.lock().map(|c| c.len()).unwrap_or(0);
        log::info!(
            "[DeepSeek] Cache state: TOOL_CALL_REASONING_CACHE={} entries, REASONING_CACHE={} entries",
            tc_cache_size,
            r_cache_size
        );
    }

    // 3. Build upstream URL
    let upstream_url = adapter.build_url(&base_url, "");
    let model = chat_body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("deepseek-chat")
        .to_string();
    log::info!(
        "[DeepSeek] Forwarding Codex request to {} (model: {})",
        upstream_url,
        model
    );

    // 4. Send request to DeepSeek API
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| ProxyError::Internal(format!("Failed to create HTTP client: {e}")))?;

    let ds_response = client
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&chat_body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ProxyError::Timeout(format!("DeepSeek request timed out: {e}"))
            } else {
                ProxyError::ForwardFailed(format!("DeepSeek request failed: {e}"))
            }
        })?;

    let status = ds_response.status();
    if !status.is_success() {
        let error_text = ds_response.text().await.unwrap_or_default();
        log::error!(
            "[DeepSeek] API error {}: {}",
            status,
            &error_text[..error_text.len().min(300)]
        );
        return Err(ProxyError::UpstreamError {
            status: status.as_u16(),
            body: Some(error_text),
        });
    }

    let is_streaming = chat_body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if !is_streaming {
        // --- Non-streaming: read full JSON and convert ---
        let completion = ds_response
            .json::<Value>()
            .await
            .map_err(|e| ProxyError::Internal(format!("Failed to parse DeepSeek response: {e}")))?;

        // Build a Responses API response from the DeepSeek completion
        let response_value = build_responses_from_chat_completion(&completion, &model);

        // Save reasoning_content to cache, keyed by the response_id so Codex's
        // next request (with `previous_response_id`) can look it up.
        let resp_id = response_value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(ref rid) = resp_id {
            if let Some(reasoning) = completion
                .pointer("/choices/0/message/reasoning_content")
                .and_then(|v| v.as_str())
            {
                if !reasoning.is_empty() {
                    log::info!(
                        "[DeepSeek] Caching reasoning_content ({} chars) from non-streaming response (resp={})",
                        reasoning.len(),
                        &rid[..rid.len().min(16)]
                    );
                    REASONING_CACHE
                        .lock()
                        .unwrap()
                        .insert(rid.clone(), reasoning.to_string());
                }
            }
            // Also cache by tool call IDs for fallback injection
            // (Codex may not send previous_response_id on tool-call round-trips).
            let reasoning_str = completion
                .pointer("/choices/0/message/reasoning_content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !reasoning_str.is_empty() {
                let tc_ids: Vec<String> = completion
                    .pointer("/choices/0/message/tool_calls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|tc| tc.get("id").and_then(|v| v.as_str()))
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                if !tc_ids.is_empty() {
                    let mut tc_cache = TOOL_CALL_REASONING_CACHE.lock().unwrap();
                    for tc_id in &tc_ids {
                        tc_cache.insert(tc_id.clone(), reasoning_str.to_string());
                        // Also cache by the proxy-generated id format "fc_{tc_id}"
                        // since Codex may use that as the call_id.
                        let proxy_id = format!("fc_{tc_id}");
                        tc_cache.insert(proxy_id, reasoning_str.to_string());
                    }
                    log::info!(
                        "[DeepSeek] Also cached reasoning by {} tool call ID(s) for fallback",
                        tc_ids.len()
                    );
                }
            }
        }

        return Ok(Json(response_value).into_response());
    }

    // --- Streaming: convert Chat Completions SSE → Responses API SSE on the fly ---
    let stream = ds_response.bytes_stream();
    let model_clone = model.clone();

    let sse_stream = futures::stream::unfold(
        (stream, String::new(), DeepSeekResponseConverter::new(&model_clone), false),
        |(mut byte_stream, mut line_buf, mut converter, mut lifecycle_emitted)| async move {
            use futures::StreamExt;

            let mut output = String::new();

            // Emit lifecycle events on the very first invocation,
            // then immediately fall through to also process any buffered data.
            if !lifecycle_emitted {
                lifecycle_emitted = true;
                for evt in converter.lifecycle_events() {
                    output.push_str(&evt.to_sse_string());
                }
                // Also process whatever is already in line_buf (could be leftover from prev chunk)
            }

            // Read next chunk from DeepSeek (unless we already have pending data in the line buffer
            // from a partial SSE line — but that's rare; normally we always read a new chunk)
            match byte_stream.next().await {
                Some(Ok(bytes)) => {
                    let chunk_str = String::from_utf8_lossy(&bytes);
                    line_buf.push_str(&chunk_str);

                    // Process all complete lines from the buffer
                    loop {
                        if let Some(newline_pos) = line_buf.find('\n') {
                            let line = line_buf[..newline_pos].to_string();
                            line_buf = line_buf[newline_pos + 1..].to_string();
                            let trimmed = line.trim();
                            if trimmed.is_empty() || trimmed.starts_with(':') {
                                continue;
                            }
                            if let Some(data) = trimmed.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    continue;
                                }
                                if let Ok(chat_chunk) = serde_json::from_str::<Value>(data) {
                                    for evt in converter.process_chunk(&chat_chunk) {
                                        output.push_str(&evt.to_sse_string());
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }

                    if output.is_empty() {
                        Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::new()), (byte_stream, line_buf, converter, true)))
                    } else {
                        Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::from(output)), (byte_stream, line_buf, converter, true)))
                    }
                }
                Some(Err(e)) => {
                    log::error!("[DeepSeek] Stream error: {e}");
                    // Save partial reasoning before emitting the error
                    let reasoning = converter.reasoning_content().to_string();
                    let resp_id = converter.response_id().to_string();
                    if !reasoning.is_empty() && !resp_id.is_empty() {
                        REASONING_CACHE.lock().unwrap().insert(resp_id, reasoning);
                    }
                    let failed = converter.failed_event(&e.to_string());
                    Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::from(failed.to_sse_string())), (byte_stream, line_buf, converter, true)))
                }
                None => {
                    // Stream ended — finalize and emit response.completed.
                    // Save reasoning_content to cache BEFORE finalizing so the
                    // cache is populated by the time Codex reads the completed event.
                    let reasoning = converter.reasoning_content().to_string();
                    let resp_id = converter.response_id().to_string();
                    if !reasoning.is_empty() && !resp_id.is_empty() {
                        log::info!(
                            "[DeepSeek] Caching reasoning_content ({} chars) from streaming response (resp_id={})",
                            reasoning.len(),
                            &resp_id[..resp_id.len().min(16)]
                        );
                        REASONING_CACHE.lock().unwrap().insert(resp_id, reasoning.clone());
                    }
                    // Also cache by each tool call ID, since Codex may not send
                    // `previous_response_id` on the follow-up (tool-call round-trip)
                    // but will include the tool call's `call_id` in function_call_output.
                    if !reasoning.is_empty() {
                        let tc_ids = converter.tool_call_ids();
                        if !tc_ids.is_empty() {
                            let mut tc_cache = TOOL_CALL_REASONING_CACHE.lock().unwrap();
                            for tc_id in &tc_ids {
                                tc_cache.insert(tc_id.clone(), reasoning.clone());
                            }
                            log::info!(
                                "[DeepSeek] Also cached reasoning by {} tool call ID(s) for fallback lookup",
                                tc_ids.len()
                            );
                        }
                    }
                    let final_events = converter.finalize();
                    if final_events.is_empty() && output.is_empty() {
                        None
                    } else {
                        for evt in final_events {
                            output.push_str(&evt.to_sse_string());
                        }
                        if output.contains("response.completed") {
                            log::info!(
                                "[DeepSeek] Stream completed, total text length: {}",
                                converter.output_text().len()
                            );
                        }
                        Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::from(output)), (byte_stream, line_buf, converter, true)))
                    }
                }
            }
        }
    );

    let body = axum::body::Body::from_stream(sse_stream);
    let headers = [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")];
    log::info!("[DeepSeek] Streaming response started for model: {model}");
    Ok((headers, body).into_response())
}

/// Build a Responses API JSON response from a Chat Completions response.
fn build_responses_from_chat_completion(completion: &Value, model: &str) -> Value {
    let msg = completion
        .pointer("/choices/0/message")
        .or_else(|| completion.pointer("/choices/0/delta"));

    let content = msg
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let finish_reason = completion
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop");

    let tool_calls = msg.and_then(|m| m.get("tool_calls")).and_then(|v| v.as_array());

    // Reasoning content (DeepSeek thinking mode)
    let reasoning = msg
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut output: Vec<Value> = Vec::new();

    // Reasoning output (for thinking mode round-trip)
    if !reasoning.is_empty() {
        output.push(json!({
            "type": "reasoning",
            "reasoning_content": reasoning,
        }));
    }

    // Text output — include reasoning_content directly on the message item
    // so Codex preserves it when reconstructing conversation history.
    if !content.is_empty() {
        let mut msg = json!({
            "type": "message",
            "id": format!("msg_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]),
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": content,
                "annotations": []
            }],
            "status": "completed"
        });
        if !reasoning.is_empty() {
            msg["reasoning_content"] = json!(reasoning);
        }
        output.push(msg);
    }

    // Tool call outputs
    if let Some(tcs) = tool_calls {
        for tc in tcs {
            let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let func = tc.get("function");
            let tc_name = func.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let tc_args = func.and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("");
            output.push(json!({
                "type": "function_call",
                "id": format!("fc_{tc_id}"),
                "call_id": tc_id,
                "name": tc_name,
                "arguments": tc_args,
                "status": "completed"
            }));
        }
    }

    let mut resp = json!({
        "id": format!("resp_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]),
        "object": "response",
        "model": model,
        "status": if finish_reason == "error" { "failed" } else { "completed" },
        "created_at": chrono::Utc::now().timestamp(),
        "output": output,
    });

    if let Some(usage) = completion.get("usage") {
        resp["usage"] = json!({
            "input_tokens": usage.get("prompt_tokens"),
            "output_tokens": usage.get("completion_tokens"),
            "total_tokens": usage.get("total_tokens"),
        });
    }

    resp
}

/// Convert a full Chat Completions SSE body string into Responses API SSE string.
/// 处理 /v1/responses/compact 请求（OpenAI Responses Compact API - Codex CLI 透传）
pub async fn handle_responses_compact(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, req_body) = request.into_parts();
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

    // Detect DeepSeek provider and route to dedicated handler
    let providers = ctx.get_providers();
    if let Some(first_provider) = providers.first() {
        if DeepSeekAdapter::is_deepseek_provider(first_provider) {
            return handle_deepseek_response(
                &state, &ctx, body, headers, first_provider,
            )
            .await;
        }
    }

    let endpoint = endpoint_with_query(&uri, "/responses/compact");

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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG).await
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
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = req_body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

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

    ctx.provider = result.provider;
    let response = result.response;

    process_response(response, &ctx, &state, &GEMINI_PARSER_CONFIG).await
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
        None,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::{responses_sse_to_response_value, should_use_claude_transform_streaming};
    use crate::proxy::ProxyError;

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
}
