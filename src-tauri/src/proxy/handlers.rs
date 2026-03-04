//! 璇锋眰澶勭悊鍣?//!
//! 澶勭悊鍚勭API绔偣鐨凥TTP璇锋眰
//!
//! 閲嶆瀯鍚庣殑缁撴瀯锛?//! - 閫氱敤閫昏緫鎻愬彇鍒?`handler_context` 鍜?`response_processor` 妯″潡
//! - 鍚?handler 鍙繚鐣欑嫭鐗圭殑涓氬姟閫昏緫
//! - Claude 鐨勬牸寮忚浆鎹㈤€昏緫淇濈暀鍦ㄦ鏂囦欢锛堢敤浜?OpenRouter 鏃ф帴鍙ｅ洖閫€锛?
use super::{
    debug_capture_store,
    error_mapper::{get_error_message, map_proxy_error_to_status},
    handler_config::{
        CLAUDE_PARSER_CONFIG, CODEX_PARSER_CONFIG, GEMINI_PARSER_CONFIG, OPENAI_PARSER_CONFIG,
    },
    handler_context::RequestContext,
    providers::{get_adapter, streaming::create_anthropic_sse_stream, transform},
    response_processor::{create_logged_passthrough_stream, process_response, SseUsageCollector},
    server::ProxyState,
    types::*,
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::app_config::AppType;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use std::collections::HashSet;

// ============================================================================
// 鍋ュ悍妫€鏌ュ拰鐘舵€佹煡璇紙绠€鍗曠鐐癸級
// ============================================================================

/// 鍋ュ悍妫€鏌?
pub async fn health_check() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// 鑾峰彇鏈嶅姟鐘舵€?
pub async fn get_status(State(state): State<ProxyState>) -> Result<Json<ProxyStatus>, ProxyError> {
    let status = state.status.read().await.clone();
    Ok(Json(status))
}

// ============================================================================
// Claude API 澶勭悊鍣紙鍖呭惈鏍煎紡杞崲閫昏緫锛?// ============================================================================

/// 澶勭悊 /v1/messages 璇锋眰锛圕laude API锛?///
/// Claude 澶勭悊鍣ㄥ寘鍚嫭鐗圭殑鏍煎紡杞崲閫昏緫锛?/// - 杩囧幓鐢ㄤ簬 OpenRouter 鐨?OpenAI Chat Completions 鍏煎鎺ュ彛锛圓nthropic 鈫?OpenAI 杞崲锛?/// - 鐜板湪 OpenRouter 宸叉帹鍑?Claude Code 鍏煎鎺ュ彛锛岄粯璁や笉鍐嶅惎鐢ㄨ杞崲锛堥€昏緫淇濈暀浠ュ鍥為€€锛?
pub async fn handle_messages(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Claude, "Claude", "claude").await?;
    maybe_capture_system_prompt(
        &state,
        ctx.app_type_str,
        &ctx.session_id,
        &ctx.request_model,
        &body,
    );

    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // 杞彂璇锋眰
    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Claude,
            "/v1/messages",
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
    let response = result.response;

    // 妫€鏌ユ槸鍚﹂渶瑕佹牸寮忚浆鎹紙OpenRouter 绛変腑杞湇鍔★級
    let adapter = get_adapter(&AppType::Claude);
    let needs_transform = adapter.needs_transform(&ctx.provider);

    // Claude 鐗规湁锛氭牸寮忚浆鎹㈠鐞?
    if needs_transform {
        return handle_claude_transform(response, &ctx, &state, &body, is_stream).await;
    }

    // 閫氱敤鍝嶅簲澶勭悊锛堥€忎紶妯″紡锛?
    process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG).await
}

/// Claude 鏍煎紡杞崲澶勭悊锛堢嫭鏈夐€昏緫锛?///
/// 澶勭悊 OpenRouter 鏃?OpenAI 鍏煎鎺ュ彛鐨勫洖閫€鏂规锛堝綋鍓嶉粯璁や笉鍚敤锛?
async fn handle_claude_transform(
    response: reqwest::Response,
    ctx: &RequestContext,
    state: &ProxyState,
    _original_body: &Value,
    is_stream: bool,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if is_stream {
        // 娴佸紡鍝嶅簲杞崲 (OpenAI SSE 鈫?Anthropic SSE)
        let stream = response.bytes_stream();
        let sse_stream = create_anthropic_sse_stream(stream);

        // 鍒涘缓浣跨敤閲忔敹闆嗗櫒
        let usage_collector = {
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = ctx.request_model.clone();
            let session_id = ctx.session_id.clone();
            let status_code = status.as_u16();
            let start_time = ctx.start_time;
            let app_type_for_debug = ctx.app_type_str.to_string();

            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                let response_preview = serde_json::to_string_pretty(&Value::Array(events.clone()))
                    .unwrap_or_else(|_| Value::Array(events.clone()).to_string());
                spawn_append_debug_response_file(
                    app_type_for_debug.clone(),
                    session_id.clone(),
                    provider_id.clone(),
                    model.clone(),
                    true,
                    status_code,
                    response_preview,
                );

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
                    log::debug!("[Claude] OpenRouter streaming response missing usage, skip cost logging");
                }
            })
        };

        // 鑾峰彇娴佸紡瓒呮椂閰嶇疆
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

    // 闈炴祦寮忓搷搴旇浆鎹?(OpenAI 鈫?Anthropic)
    let response_headers = response.headers().clone();

    let body_bytes = response.bytes().await.map_err(|e| {
        log::error!("[Claude] 璇诲彇鍝嶅簲浣撳け璐? {e}");
        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
    })?;

    let body_str = String::from_utf8_lossy(&body_bytes);

    let openai_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        log::error!("[Claude] 瑙ｆ瀽 OpenAI 鍝嶅簲澶辫触: {e}, body: {body_str}");
        ProxyError::TransformError(format!("Failed to parse OpenAI response: {e}"))
    })?;

    let anthropic_response = transform::openai_to_anthropic(openai_response).map_err(|e| {
        log::error!("[Claude] 杞崲鍝嶅簲澶辫触: {e}");
        e
    })?;

    let debug_response_preview = serde_json::to_string_pretty(&anthropic_response)
        .unwrap_or_else(|_| anthropic_response.to_string());
    let debug_response_model = anthropic_response
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();
    spawn_append_debug_response_file(
        ctx.app_type_str.to_string(),
        ctx.session_id.clone(),
        ctx.provider.id.clone(),
        debug_response_model,
        false,
        status.as_u16(),
        debug_response_preview,
    );

    // 璁板綍浣跨敤閲?
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

    // 鏋勫缓鍝嶅簲
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
        log::error!("[Claude] 搴忓垪鍖栧搷搴斿け璐? {e}");
        ProxyError::TransformError(format!("Failed to serialize response: {e}"))
    })?;

    let body = axum::body::Body::from(response_body);
    builder.body(body).map_err(|e| {
        log::error!("[Claude] 鏋勫缓鍝嶅簲澶辫触: {e}");
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

// ============================================================================
// Codex API 澶勭悊鍣?// ============================================================================

/// 澶勭悊 /v1/chat/completions 璇锋眰锛圤penAI Chat Completions API - Codex CLI锛?
pub async fn handle_chat_completions(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    maybe_capture_system_prompt(
        &state,
        ctx.app_type_str,
        &ctx.session_id,
        &ctx.request_model,
        &body,
    );

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            "/chat/completions",
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

/// 澶勭悊 /v1/responses 璇锋眰锛圤penAI Responses API - Codex CLI 閫忎紶锛?
pub async fn handle_responses(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let mut ctx =
        RequestContext::new(&state, &body, &headers, AppType::Codex, "Codex", "codex").await?;
    maybe_capture_system_prompt(
        &state,
        ctx.app_type_str,
        &ctx.session_id,
        &ctx.request_model,
        &body,
    );

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            "/responses",
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

    process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG).await
}

// ============================================================================
// Gemini API 澶勭悊鍣?// ============================================================================

/// 澶勭悊 Gemini API 璇锋眰锛堥€忎紶锛屽寘鎷煡璇㈠弬鏁帮級
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    // Gemini 鐨勬ā鍨嬪悕绉板湪 URI 涓?
    let mut ctx = RequestContext::new(&state, &body, &headers, AppType::Gemini, "Gemini", "gemini")
        .await?
        .with_model_from_uri(&uri);
    maybe_capture_system_prompt(
        &state,
        ctx.app_type_str,
        &ctx.session_id,
        &ctx.request_model,
        &body,
    );

    // 鎻愬彇瀹屾暣鐨勮矾寰勫拰鏌ヨ鍙傛暟
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

const MAX_CAPTURED_SYSTEM_PROMPT_LEN: usize = 1_000_000;
const MAX_CAPTURED_MESSAGES_LEN: usize = 1_000_000;
const MAX_CAPTURED_USER_INPUTS_LEN: usize = 1_000_000;
const MAX_CAPTURED_DEBUG_DIRECTIVES_LEN: usize = 1_000_000;
const MAX_CAPTURED_DEBUG_BODY_LEN: usize = 4_000_000;
const RECENT_MESSAGES_LIMIT: usize = 6;

fn maybe_capture_system_prompt(
    _state: &ProxyState,
    app_type: &str,
    session_id: &str,
    model: &str,
    body: &Value,
) {
    if !crate::settings::get_settings().capture_system_prompt {
        return;
    }

    let Some(mut prompt) = extract_system_prompt(body) else {
        return;
    };

    let mut user_inputs = extract_user_inputs(body).unwrap_or_default();
    let mut messages = extract_recent_messages(body).unwrap_or_default();
    let mut directives = extract_agent_directives(body).unwrap_or_default();
    let body_for_debug = body.clone();

    if prompt.len() > MAX_CAPTURED_SYSTEM_PROMPT_LEN {
        truncate_utf8(&mut prompt, MAX_CAPTURED_SYSTEM_PROMPT_LEN);
        log::warn!(
            "[{app_type}] Captured system prompt is too large and was truncated for session {}",
            session_id
        );
    }

    if messages.len() > MAX_CAPTURED_MESSAGES_LEN {
        truncate_utf8(&mut messages, MAX_CAPTURED_MESSAGES_LEN);
        log::warn!(
            "[{app_type}] Captured message context is too large and was truncated for session {}",
            session_id
        );
    }

    if user_inputs.len() > MAX_CAPTURED_USER_INPUTS_LEN {
        truncate_utf8(&mut user_inputs, MAX_CAPTURED_USER_INPUTS_LEN);
        log::warn!(
            "[{app_type}] Captured user input context is too large and was truncated for session {}",
            session_id
        );
    }

    if directives.len() > MAX_CAPTURED_DEBUG_DIRECTIVES_LEN {
        truncate_utf8(&mut directives, MAX_CAPTURED_DEBUG_DIRECTIVES_LEN);
        log::warn!(
            "[{app_type}] Captured directive context is too large and was truncated for session {}",
            session_id
        );
    }

    let app_type = app_type.to_string();
    let session_id = session_id.to_string();
    let model = model.to_string();

    tauri::async_runtime::spawn(async move {
        let app_type_for_log = app_type.clone();
        let session_id_for_log = session_id.clone();

        let save_result = tokio::task::spawn_blocking(move || {
            if let Err(err) = append_debug_capture_file(
                &app_type,
                &session_id,
                &model,
                &prompt,
                &user_inputs,
                &messages,
                &directives,
                &body_for_debug,
            ) {
                log::warn!(
                    "[{app_type}] Failed to append debug capture file for session {}: {}",
                    session_id,
                    err
                );
            }
        })
        .await;

        match save_result {
            Ok(()) => {}
            Err(err) => {
                log::warn!(
                    "[{app_type_for_log}] Failed to join capture persistence task for session {}: {}",
                    session_id_for_log,
                    err
                );
            }
        }
    });
}

fn append_debug_capture_file(
    app_type: &str,
    session_id: &str,
    model: &str,
    system_prompt: &str,
    user_inputs: &str,
    messages: &str,
    directives: &str,
    body: &Value,
) -> Result<(), std::io::Error> {
    let mut directives_text = directives.to_string();
    if directives_text.len() > MAX_CAPTURED_DEBUG_DIRECTIVES_LEN {
        truncate_utf8(&mut directives_text, MAX_CAPTURED_DEBUG_DIRECTIVES_LEN);
    }

    let mut raw_body = serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string());
    if raw_body.len() > MAX_CAPTURED_DEBUG_BODY_LEN {
        truncate_utf8(&mut raw_body, MAX_CAPTURED_DEBUG_BODY_LEN);
    }

    let session_file = debug_capture_store::capture_session_path(app_type, session_id);
    let index_file = debug_capture_store::capture_index_path();

    let entry = format!(
        "\n===== CC SWITCH INTERCEPT DEBUG =====\n\
timestamp: {}\n\
direction: REQUEST\n\
app_type: {}\n\
session_id: {}\n\
model: {}\n\
log_file: {}\n\
index_file: {}\n\
\n\
[system_prompt]\n{}\n\
\n\
[user_to_agent]\n{}\n\
\n\
[agent_directives]\n{}\n\
\n\
[recent_messages]\n{}\n\
\n\
[raw_body]\n{}\n\
===== END =====\n",
        chrono::Utc::now().to_rfc3339(),
        app_type,
        session_id,
        model,
        session_file.display(),
        index_file.display(),
        system_prompt,
        user_inputs,
        directives_text,
        messages,
        raw_body
    );

    debug_capture_store::append_session_debug_entry(app_type, session_id, model, "REQUEST", &entry)
}

fn append_debug_response_file(
    app_type: &str,
    session_id: &str,
    provider_id: &str,
    model: &str,
    is_streaming: bool,
    status_code: u16,
    response_preview: &str,
) -> Result<(), std::io::Error> {
    let mut preview = response_preview.to_string();
    if preview.len() > MAX_CAPTURED_DEBUG_BODY_LEN {
        truncate_utf8(&mut preview, MAX_CAPTURED_DEBUG_BODY_LEN);
    }

    let session_file = debug_capture_store::capture_session_path(app_type, session_id);
    let index_file = debug_capture_store::capture_index_path();

    let entry = format!(
        "\n===== CC SWITCH INTERCEPT DEBUG =====\n\
timestamp: {}\n\
direction: RESPONSE\n\
app_type: {}\n\
session_id: {}\n\
provider_id: {}\n\
model: {}\n\
streaming: {}\n\
status_code: {}\n\
log_file: {}\n\
index_file: {}\n\
\n\
[response]\n{}\n\
===== END =====\n",
        chrono::Utc::now().to_rfc3339(),
        app_type,
        session_id,
        provider_id,
        model,
        is_streaming,
        status_code,
        session_file.display(),
        index_file.display(),
        preview
    );

    debug_capture_store::append_session_debug_entry(app_type, session_id, model, "RESPONSE", &entry)
}

fn spawn_append_debug_response_file(
    app_type: String,
    session_id: String,
    provider_id: String,
    model: String,
    is_streaming: bool,
    status_code: u16,
    response_preview: String,
) {
    tauri::async_runtime::spawn(async move {
        let app_type_for_log = app_type.clone();
        let session_id_for_log = session_id.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            append_debug_response_file(
                &app_type,
                &session_id,
                &provider_id,
                &model,
                is_streaming,
                status_code,
                &response_preview,
            )
        })
        .await;

        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                log::warn!(
                    "[{app_type_for_log}] Failed to append response debug file for session {}: {}",
                    session_id_for_log,
                    err
                );
            }
            Err(err) => {
                log::warn!(
                    "[{app_type_for_log}] Failed to join response debug file task for session {}: {}",
                    session_id_for_log,
                    err
                );
            }
        }
    });
}

fn extract_recent_messages(body: &Value) -> Option<String> {
    let mut rendered = Vec::new();

    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        append_recent_messages(messages, &mut rendered);
    }

    if let Some(contents) = body.get("contents").and_then(Value::as_array) {
        append_recent_messages(contents, &mut rendered);
    }

    if let Some(input) = body.get("input") {
        match input {
            Value::Array(items) => append_recent_messages(items, &mut rendered),
            Value::Object(_) => {
                if let Some(line) = render_message_line(input) {
                    rendered.push(line);
                }
            }
            Value::String(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    rendered.push(format!("input: {text}"));
                }
            }
            _ => {}
        }
    }

    if rendered.is_empty() {
        None
    } else {
        Some(rendered.join("\n"))
    }
}

fn extract_user_inputs(body: &Value) -> Option<String> {
    let mut rendered = Vec::new();

    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !role.eq_ignore_ascii_case("user") {
                continue;
            }
            if message
                .get("content")
                .map(is_agent_tool_result_content)
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(line) = render_user_input_line(message) {
                rendered.push(line);
            }
        }
    }

    if let Some(contents) = body.get("contents").and_then(Value::as_array) {
        for content in contents {
            let role = content
                .get("role")
                .and_then(Value::as_str)
                .or_else(|| content.get("type").and_then(Value::as_str))
                .unwrap_or_default();
            if !role.eq_ignore_ascii_case("user") {
                continue;
            }
            if let Some(line) = render_user_input_line(content) {
                rendered.push(line);
            }
        }
    }

    if let Some(input) = body.get("input") {
        match input {
            Value::String(text) => {
                let text = text.trim();
                if !text.is_empty() {
                    rendered.push(format!("user: {text}"));
                }
            }
            Value::Array(items) => {
                for item in items {
                    let role = item
                        .get("role")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("type").and_then(Value::as_str))
                        .unwrap_or_default();
                    if !role.eq_ignore_ascii_case("user") {
                        continue;
                    }
                    if item
                        .get("content")
                        .map(is_agent_tool_result_content)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    if let Some(line) = render_user_input_line(item) {
                        rendered.push(line);
                    }
                }
            }
            Value::Object(_) => {
                if let Some(line) = render_user_input_line(input) {
                    rendered.push(line);
                }
            }
            _ => {}
        }
    }

    if rendered.is_empty() {
        None
    } else {
        Some(rendered.join("\n"))
    }
}

fn is_agent_tool_result_content(value: &Value) -> bool {
    contains_content_type(
        value,
        &["tool_result", "function_call_output", "custom_tool_call_output"],
    )
}

fn contains_content_type(value: &Value, expected_types: &[&str]) -> bool {
    match value {
        Value::Array(items) => items
            .iter()
            .any(|item| contains_content_type(item, expected_types)),
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .map(|actual| {
                    expected_types
                        .iter()
                        .any(|expected| actual.eq_ignore_ascii_case(expected))
                })
                .unwrap_or(false)
            {
                return true;
            }

            for key in ["content", "parts", "input", "output", "value", "result"] {
                if let Some(child) = map.get(key) {
                    if contains_content_type(child, expected_types) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

fn render_user_input_line(value: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    if let Some(content) = value.get("content") {
        collect_text_fragments(content, &mut fragments);
    } else if let Some(parts) = value.get("parts") {
        collect_text_fragments(parts, &mut fragments);
    } else if let Some(text) = value.as_str() {
        if !text.trim().is_empty() {
            fragments.push(text.to_string());
        }
    }

    let combined = fragments
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");

    if combined.is_empty() {
        None
    } else {
        Some(format!("user: {combined}"))
    }
}

fn extract_agent_directives(body: &Value) -> Option<String> {
    let mut sections = Vec::new();
    let mut directive_fragments = Vec::new();

    if let Some(instructions) = body.get("instructions") {
        collect_text_fragments(instructions, &mut directive_fragments);
    }
    if let Some(system_instruction) = body
        .get("system_instruction")
        .or_else(|| body.get("systemInstruction"))
    {
        collect_text_fragments(system_instruction, &mut directive_fragments);
    }
    if let Some(developer) = body.get("developer") {
        collect_text_fragments(developer, &mut directive_fragments);
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if role == "system" || role == "developer" {
                if let Some(content) = message.get("content") {
                    collect_text_fragments(content, &mut directive_fragments);
                } else {
                    collect_text_fragments(message, &mut directive_fragments);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut unique_fragments = Vec::new();
    for fragment in directive_fragments {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            unique_fragments.push(trimmed.to_string());
        }
    }
    if !unique_fragments.is_empty() {
        sections.push(format!("[directive_text]\n{}", unique_fragments.join("\n\n")));
    }

    for key in [
        "tools",
        "tool_choice",
        "response_format",
        "reasoning",
        "temperature",
        "top_p",
        "max_tokens",
        "max_output_tokens",
        "stop",
        "metadata",
    ] {
        if let Some(value) = body.get(key) {
            let rendered =
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            sections.push(format!("[{key}]\n{rendered}"));
        }
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn append_recent_messages(items: &[Value], rendered: &mut Vec<String>) {
    let start = items.len().saturating_sub(RECENT_MESSAGES_LIMIT);
    for item in &items[start..] {
        if let Some(line) = render_message_line(item) {
            rendered.push(line);
        }
    }
}

fn render_message_line(value: &Value) -> Option<String> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("message");

    let mut fragments = Vec::new();
    if let Some(content) = value.get("content") {
        collect_text_fragments(content, &mut fragments);
    } else if let Some(parts) = value.get("parts") {
        collect_text_fragments(parts, &mut fragments);
    } else {
        collect_text_fragments(value, &mut fragments);
    }

    let combined = fragments
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");

    if combined.is_empty() {
        None
    } else {
        Some(format!("{role}: {combined}"))
    }
}

fn extract_system_prompt(body: &Value) -> Option<String> {
    let mut fragments = Vec::new();

    if let Some(system) = body.get("system") {
        collect_text_fragments(system, &mut fragments);
    }

    if let Some(instructions) = body.get("instructions") {
        collect_text_fragments(instructions, &mut fragments);
    }

    if let Some(system_instruction) = body
        .get("system_instruction")
        .or_else(|| body.get("systemInstruction"))
    {
        collect_text_fragments(system_instruction, &mut fragments);
    }

    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            collect_system_from_message(message, &mut fragments);
        }
    }

    if let Some(contents) = body.get("contents").and_then(Value::as_array) {
        for content in contents {
            collect_system_from_message(content, &mut fragments);
        }
    }

    if let Some(input) = body.get("input") {
        match input {
            Value::Array(items) => {
                for item in items {
                    collect_system_from_message(item, &mut fragments);
                }
            }
            Value::Object(_) => collect_system_from_message(input, &mut fragments),
            _ => {}
        }
    }

    if fragments.is_empty() {
        return None;
    }

    let mut seen = HashSet::new();
    let mut unique_fragments = Vec::new();
    for fragment in fragments {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            unique_fragments.push(trimmed.to_string());
        }
    }

    if unique_fragments.is_empty() {
        None
    } else {
        Some(unique_fragments.join("\n\n"))
    }
}

fn collect_system_from_message(value: &Value, fragments: &mut Vec<String>) {
    if !is_system_role(value) {
        return;
    }

    if let Some(content) = value.get("content") {
        collect_text_fragments(content, fragments);
    } else {
        collect_text_fragments(value, fragments);
    }
}

fn is_system_role(value: &Value) -> bool {
    value
        .get("role")
        .and_then(Value::as_str)
        .map(|role| role.eq_ignore_ascii_case("system"))
        .unwrap_or(false)
}

fn collect_text_fragments(value: &Value, fragments: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                fragments.push(text.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_fragments(item, fragments);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    fragments.push(text.to_string());
                }
            }
            if let Some(text) = map.get("input_text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    fragments.push(text.to_string());
                }
            }

            for key in [
                "content",
                "parts",
                "value",
                "instruction",
                "instructions",
                "prompt",
            ] {
                if let Some(child) = map.get(key) {
                    collect_text_fragments(child, fragments);
                }
            }
        }
        _ => {}
    }
}

fn truncate_utf8(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }

    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

// ============================================================================
// 浣跨敤閲忚褰曪紙淇濈暀鐢ㄤ簬 Claude 杞崲閫昏緫锛?// ============================================================================

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
        log::warn!("璁板綍澶辫触璇锋眰鏃ュ織澶辫触: {e}");
    }
}

/// 璁板綍璇锋眰浣跨敤閲?
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
        log::warn!("[USG-001] 璁板綍浣跨敤閲忓け璐? {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore]
    fn manual_write_interceptor_debug_file() {
        let enable = std::env::var("CC_SWITCH_WRITE_DEBUG_FILE").unwrap_or_default();
        assert_eq!(
            enable, "1",
            "set CC_SWITCH_WRITE_DEBUG_FILE=1 to run this manual debug file test"
        );

        let session_id = std::env::var("CC_SWITCH_SEED_SESSION_ID")
            .unwrap_or_else(|_| "manual-debug-session".to_string());

        let body = json!({
            "model": "demo/manual-model",
            "instructions": "You are an agent that must follow strict system constraints.",
            "messages": [
                {"role": "system", "content": "System policy block"},
                {"role": "developer", "content": "Developer says: use tool-call-first strategy"},
                {"role": "user", "content": "please continue debug task"}
            ],
            "tools": [
                {"type": "function", "function": {"name": "web_search", "description": "search web"}}
            ],
            "tool_choice": "auto"
        });

        let directives = extract_agent_directives(&body).unwrap_or_default();
        append_debug_capture_file(
            "claude",
            &session_id,
            "demo/manual-model",
            "MANUAL_DEBUG_SYSTEM_PROMPT",
            "user: please continue debug task",
            "user: please continue debug task",
            &directives,
            &body,
        )
        .expect("write debug capture file");

        let path = debug_capture_store::capture_session_path("claude", &session_id);
        let file_content =
            std::fs::read_to_string(&path).expect("read debug capture file from session path");
        assert!(file_content.contains("CC SWITCH INTERCEPT DEBUG"));
        assert!(file_content.contains("MANUAL_DEBUG_SYSTEM_PROMPT"));
        assert!(file_content.contains("[user_to_agent]"));

        let index_path = debug_capture_store::capture_index_path();
        let index_content =
            std::fs::read_to_string(&index_path).expect("read debug capture index file");
        assert!(index_content.contains(&session_id));
        assert!(index_content.contains(path.file_name().and_then(|n| n.to_str()).unwrap_or("")));

        println!("debug_capture_file_path={}", path.display());
        println!("debug_capture_index_path={}", index_path.display());
        println!("debug_session_id={session_id}");
    }
}



