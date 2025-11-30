//! 请求处理器
//!
//! 处理各种API端点的HTTP请求

use super::{forwarder::RequestForwarder, server::ProxyState, types::*, ProxyError};
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
    let _provider = router
        .select_provider(&AppType::Claude, &failed_ids)
        .await?;

    // 直接透传 Claude 请求
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Claude, "/v1/messages", body, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    // 复制响应头
    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());

    Ok(builder.body(body).unwrap())
}

/// 处理 /v1/messages/count_tokens 请求（透传）
pub async fn handle_count_tokens(
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
        .forward_with_retry(&AppType::Claude, "/v1/messages/count_tokens", body, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}

/// 处理 Gemini API 请求（透传）
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    axum::extract::Path(path): axum::extract::Path<String>,
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

    let endpoint = format!("/{path}");
    let response = forwarder
        .forward_with_retry(&AppType::Gemini, &endpoint, body, headers)
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

/// 获取单个 Response（GET /v1/responses/:response_id 透传）
pub async fn handle_get_response(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let endpoint = format!("/v1/responses/{response_id}");
    let response = forwarder
        .forward_get_request(&AppType::Codex, &endpoint, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}

/// 删除 Response（DELETE /v1/responses/:response_id 透传）
pub async fn handle_delete_response(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let endpoint = format!("/v1/responses/{response_id}");
    let response = forwarder
        .forward_delete_request(&AppType::Codex, &endpoint, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}

/// 获取 Response 的输入项（GET /v1/responses/:response_id/input_items 透传）
pub async fn handle_get_response_input_items(
    State(state): State<ProxyState>,
    axum::extract::Path(response_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ProxyError> {
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
    );

    let endpoint = format!("/v1/responses/{response_id}/input_items");
    let response = forwarder
        .forward_get_request(&AppType::Codex, &endpoint, headers)
        .await?;

    // 透传响应
    let mut builder = axum::response::Response::builder().status(response.status());

    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from_stream(response.bytes_stream());
    Ok(builder.body(body).unwrap())
}
