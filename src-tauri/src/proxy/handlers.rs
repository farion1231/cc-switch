//! 请求处理器
//!
//! 处理各种API端点的HTTP请求

use super::{
    forwarder::RequestForwarder,
    providers::{get_adapter, transform},
    server::ProxyState,
    types::*,
    ProxyError,
};
use crate::app_config::AppType;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

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

/// 处理 /v1/messages 请求（Claude API）
pub async fn handle_messages(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();

    // 选择目标 Provider
    let router = super::router::ProviderRouter::new(state.db.clone());
    let failed_ids = Vec::new();
    let provider = router
        .select_provider(&AppType::Claude, &failed_ids)
        .await?;

    // 检查是否需要转换（OpenRouter）
    let adapter = get_adapter(&AppType::Claude);
    let needs_transform = adapter.needs_transform(&provider);

    // 检查是否是流式请求
    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    log::info!(
        "[Claude] Provider: {}, needs_transform: {}, is_stream: {}",
        provider.name,
        needs_transform,
        is_stream
    );

    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Claude, "/v1/messages", body, headers)
        .await?;

    let status = response.status();
    log::info!("[Claude] 上游响应状态: {status}");

    // 如果需要转换且是非流式请求，转换响应
    if needs_transform && !is_stream {
        log::info!("[Claude] 开始转换响应 (OpenAI → Anthropic)");

        let response_headers = response.headers().clone();

        // 读取响应体
        let body_bytes = response.bytes().await.map_err(|e| {
            log::error!("[Claude] 读取响应体失败: {e}");
            ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
        })?;

        let body_str = String::from_utf8_lossy(&body_bytes);
        log::info!("[Claude] OpenAI 响应长度: {} bytes", body_bytes.len());
        log::debug!("[Claude] OpenAI 原始响应: {body_str}");

        // 解析并转换
        let openai_response: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
            log::error!("[Claude] 解析 OpenAI 响应失败: {e}, body: {body_str}");
            ProxyError::TransformError(format!("Failed to parse OpenAI response: {e}"))
        })?;

        log::info!("[Claude] 解析 OpenAI 响应成功");

        let anthropic_response = transform::openai_to_anthropic(openai_response).map_err(|e| {
            log::error!("[Claude] 转换响应失败: {e}");
            e
        })?;

        log::info!("[Claude] 转换响应成功");
        log::info!(
            "[Claude] Anthropic 响应: {}",
            serde_json::to_string(&anthropic_response).unwrap_or_default()
        );

        // 构建响应
        let mut builder = axum::response::Response::builder().status(status);

        // 复制响应头（排除 content-length，因为内容已改变）
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

        log::info!(
            "[Claude] 返回转换后的响应, 长度: {} bytes",
            response_body.len()
        );

        let body = axum::body::Body::from(response_body);
        return Ok(builder.body(body).unwrap());
    }

    // 流式请求需要特殊处理
    if needs_transform && is_stream {
        log::warn!("[Claude] OpenRouter 流式请求暂不支持完整转换，透传响应");
    }

    // 透传响应（直连 Anthropic 或流式请求）
    log::info!("[Claude] 透传响应");
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}

/// 处理 Gemini API 请求（透传，包括查询参数）
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    // 提取完整的路径和查询参数
    let endpoint = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path());

    log::debug!("Gemini request endpoint (with query): {endpoint}");

    let response = forwarder
        .forward_with_retry(&AppType::Gemini, endpoint, body, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI 透传）
pub async fn handle_responses(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Codex, "/v1/responses", body, headers)
        .await?;

    // 透传响应（包括流式和非流式）
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}
