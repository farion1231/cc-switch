//! 请求处理器
//!
//! 处理各种API端点的HTTP请求

use super::{
    forwarder::RequestForwarder,
    providers::{get_adapter, transform, ProviderType},
    server::ProxyState,
    session::ProxySession,
    types::*,
    usage::{logger::UsageLogger, parser::TokenUsage},
    ProxyError,
};
use crate::app_config::AppType;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;

/// 记录请求使用量（带 ProxySession 支持）
#[allow(dead_code, clippy::too_many_arguments)]
async fn log_usage_with_session(
    state: &ProxyState,
    session: &ProxySession,
    provider_id: &str,
    app_type: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    status_code: u16,
    provider_type: Option<&ProviderType>,
) {
    let logger = UsageLogger::new(&state.db);

    // 获取 provider 的 cost_multiplier
    let multiplier = match state.db.get_provider_by_id(provider_id, app_type) {
        Ok(Some(p)) => {
            if let Some(meta) = p.meta {
                if let Some(cm) = meta.cost_multiplier {
                    Decimal::from_str(&cm).unwrap_or(Decimal::from(1))
                } else {
                    Decimal::from(1)
                }
            } else {
                Decimal::from(1)
            }
        }
        _ => Decimal::from(1),
    };

    let model = session
        .model
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let provider_type_str = provider_type.map(|pt| pt.as_str().to_string());

    if let Err(e) = logger.log_with_calculation(
        session.session_id.clone(),
        provider_id.to_string(),
        app_type.to_string(),
        model,
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        Some(session.session_id.clone()),
        provider_type_str,
        session.is_streaming,
    ) {
        log::warn!("记录使用量失败: {e}");
    }
}

/// 记录请求使用量（兼容旧接口）
#[allow(clippy::too_many_arguments)]
async fn log_usage(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
) {
    let logger = UsageLogger::new(&state.db);

    // 获取 provider 的 cost_multiplier
    let multiplier = match state.db.get_provider_by_id(provider_id, app_type) {
        Ok(Some(p)) => {
            if let Some(meta) = p.meta {
                if let Some(cm) = meta.cost_multiplier {
                    Decimal::from_str(&cm).unwrap_or(Decimal::from(1))
                } else {
                    Decimal::from(1)
                }
            } else {
                Decimal::from(1)
            }
        }
        _ => Decimal::from(1),
    };

    let request_id = uuid::Uuid::new_v4().to_string();

    if let Err(e) = logger.log_with_calculation(
        request_id,
        provider_id.to_string(),
        app_type.to_string(),
        model.to_string(),
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        None,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("记录使用量失败: {e}");
    }
}

type UsageCallbackWithTiming = Arc<dyn Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static>;

#[derive(Clone)]
struct SseUsageCollector {
    inner: Arc<SseUsageCollectorInner>,
}

struct SseUsageCollectorInner {
    events: Mutex<Vec<Value>>,
    first_event_time: Mutex<Option<std::time::Instant>>,
    start_time: std::time::Instant,
    on_complete: UsageCallbackWithTiming,
    finished: AtomicBool,
}

impl SseUsageCollector {
    fn new(
        start_time: std::time::Instant,
        callback: impl Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static,
    ) -> Self {
        let on_complete: UsageCallbackWithTiming = Arc::new(callback);
        Self {
            inner: Arc::new(SseUsageCollectorInner {
                events: Mutex::new(Vec::new()),
                first_event_time: Mutex::new(None),
                start_time,
                on_complete,
                finished: AtomicBool::new(false),
            }),
        }
    }

    async fn push(&self, event: Value) {
        // 记录首个事件时间
        {
            let mut first_time = self.inner.first_event_time.lock().await;
            if first_time.is_none() {
                *first_time = Some(std::time::Instant::now());
            }
        }
        let mut events = self.inner.events.lock().await;
        events.push(event);
    }

    async fn finish(&self) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }

        let events = {
            let mut guard = self.inner.events.lock().await;
            std::mem::take(&mut *guard)
        };

        let first_token_ms = {
            let first_time = self.inner.first_event_time.lock().await;
            first_time.map(|t| (t - self.inner.start_time).as_millis() as u64)
        };

        (self.inner.on_complete)(events, first_token_ms);
    }
}

/// 创建带日志记录的透传流
fn create_logged_passthrough_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    tag: &'static str,
    usage_collector: Option<SseUsageCollector>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut collector = usage_collector;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // 尝试解析并记录完整的 SSE 事件
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_text = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if !event_text.trim().is_empty() {
                            // 提取 data 部分并尝试解析为 JSON
                            for line in event_text.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data.trim() != "[DONE]" {
                                        if let Ok(json_value) = serde_json::from_str::<Value>(data) {
                                            if let Some(c) = &collector {
                                                c.push(json_value.clone()).await;
                                            }
                                            log::info!(
                                                "[{}] <<< SSE 事件:\n{}",
                                                tag,
                                                serde_json::to_string_pretty(&json_value).unwrap_or_else(|_| data.to_string())
                                            );
                                        } else {
                                            log::info!("[{tag}] <<< SSE 数据: {data}");
                                        }
                                    } else {
                                        log::info!("[{tag}] <<< SSE: [DONE]");
                                    }
                                }
                            }
                        }
                    }

                    yield Ok(bytes);
                }
                Err(e) => {
                    log::error!("[{tag}] 流错误: {e}");
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
            }
        }

        log::info!("[{}] ====== 流结束 ======", tag);

        if let Some(c) = collector.take() {
            c.finish().await;
        }
    }
}

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
    let start_time = std::time::Instant::now();

    let config = state.config.read().await.clone();
    let request_model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

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
        state.current_providers.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Claude, "/v1/messages", body, headers)
        .await?;

    let status = response.status();
    log::info!("[Claude] 上游响应状态: {status}");

    // 如果需要转换
    if needs_transform {
        if is_stream {
            // 流式响应转换
            log::info!("[Claude] 开始流式响应转换 (OpenAI SSE → Anthropic SSE)");

            let stream = response.bytes_stream();
            let sse_stream = super::providers::streaming::create_anthropic_sse_stream(stream);

            let usage_collector = {
                let state = state.clone();
                let provider_id = provider.id.clone();
                let model = request_model.clone();
                let status_code = status.as_u16();
                let start_time_clone = start_time;
                SseUsageCollector::new(start_time, move |events, first_token_ms| {
                    if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                        let latency_ms = start_time_clone.elapsed().as_millis() as u64;
                        let state = state.clone();
                        let provider_id = provider_id.clone();
                        let model = model.clone();
                        tokio::spawn(async move {
                            log_usage(
                                &state,
                                &provider_id,
                                "claude",
                                &model,
                                usage,
                                latency_ms,
                                first_token_ms,
                                true, // is_streaming
                                status_code,
                            )
                            .await;
                        });
                    } else {
                        log::debug!("[Claude] OpenRouter 流式响应缺少 usage 统计，跳过消费记录");
                    }
                })
            };

            let logged_stream = create_logged_passthrough_stream(
                sse_stream,
                "Claude/OpenRouter",
                Some(usage_collector),
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
            log::info!("[Claude] ====== 请求结束 (流式转换) ======");
            return Ok((headers, body).into_response());
        } else {
            // 非流式响应转换
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
            log::info!(
                "[Claude] <<< OpenAI 响应 JSON:\n{}",
                serde_json::to_string_pretty(&openai_response).unwrap_or_default()
            );

            let anthropic_response =
                transform::openai_to_anthropic(openai_response).map_err(|e| {
                    log::error!("[Claude] 转换响应失败: {e}");
                    e
                })?;

            log::info!("[Claude] 转换响应成功");
            log::info!(
                "[Claude] <<< Anthropic 响应 JSON:\n{}",
                serde_json::to_string_pretty(&anthropic_response).unwrap_or_default()
            );

            // 记录使用量
            if let Some(usage) = TokenUsage::from_claude_response(&anthropic_response) {
                let model = anthropic_response
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                let latency_ms = start_time.elapsed().as_millis() as u64;

                tokio::spawn({
                    let state = state.clone();
                    let provider_id = provider.id.clone();
                    let model = model.to_string();
                    async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "claude",
                            &model,
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

            log::info!("[Claude] ====== 请求结束 ======");

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
    }

    // 透传响应（直连 Anthropic）
    log::info!("[Claude] 透传响应模式");

    // 检查是否流式响应
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_sse = content_type.contains("text/event-stream");

    if is_sse {
        // 流式透传：使用包装流记录 SSE 事件
        log::info!("[Claude] 流式透传响应 (SSE)");
        let mut builder = axum::response::Response::builder().status(status);

        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }

        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())));
        let usage_collector = {
            let state = state.clone();
            let provider_id = provider.id.clone();
            let model = request_model.clone();
            let status_code = status.as_u16();
            let start_time_clone = start_time;
            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                    let latency_ms = start_time_clone.elapsed().as_millis() as u64;
                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    let model = model.clone();
                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "claude",
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
                    log::debug!("[Claude] 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };
        let logged_stream =
            create_logged_passthrough_stream(stream, "Claude", Some(usage_collector));

        let body = axum::body::Body::from_stream(logged_stream);
        log::info!("[Claude] ====== 请求结束 (流式) ======");
        Ok(builder.body(body).unwrap())
    } else {
        // 非流式透传：读取完整响应并记录
        let response_headers = response.headers().clone();
        let status = response.status();

        let body_bytes = response.bytes().await.map_err(|e| {
            log::error!("[Claude] 读取透传响应失败: {e}");
            ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
        })?;

        // 记录响应 JSON
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            log::info!(
                "[Claude] <<< Anthropic 透传响应 JSON:\n{}",
                serde_json::to_string_pretty(&json_value).unwrap_or_default()
            );

            // 记录使用量
            if let Some(usage) = TokenUsage::from_claude_response(&json_value) {
                let model = json_value
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                let latency_ms = start_time.elapsed().as_millis() as u64;

                tokio::spawn({
                    let state = state.clone();
                    let provider_id = provider.id.clone();
                    let model = model.to_string();
                    async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "claude",
                            &model,
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
        } else {
            log::info!(
                "[Claude] <<< 透传响应 (非 JSON): {} bytes",
                body_bytes.len()
            );
        }
        log::info!("[Claude] ====== 请求结束 ======");

        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            builder = builder.header(key, value);
        }

        let body = axum::body::Body::from(body_bytes);
        Ok(builder.body(body).unwrap())
    }
}

/// 处理 Gemini API 请求（透传，包括查询参数）
pub async fn handle_gemini(
    State(state): State<ProxyState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let start_time = std::time::Instant::now();

    let config = state.config.read().await.clone();

    // 选择目标 Provider
    let router = super::router::ProviderRouter::new(state.db.clone());
    let failed_ids = Vec::new();
    let provider = router
        .select_provider(&AppType::Gemini, &failed_ids)
        .await?;

    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
        state.current_providers.clone(),
    );

    // 提取完整的路径和查询参数
    let endpoint = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path());
    let gemini_model = endpoint
        .split('/')
        .find(|s| s.starts_with("models/"))
        .and_then(|s| s.strip_prefix("models/"))
        .map(|s| s.split(':').next().unwrap_or(s))
        .unwrap_or("unknown")
        .to_string();

    log::info!("[Gemini] 请求端点: {endpoint}");

    let response = forwarder
        .forward_with_retry(&AppType::Gemini, endpoint, body, headers)
        .await?;

    let status = response.status();
    log::info!("[Gemini] 上游响应状态: {status}");

    // 检查是否流式响应
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_sse = content_type.contains("text/event-stream");

    if is_sse {
        // 流式透传
        log::info!("[Gemini] 流式透传响应 (SSE)");
        let mut builder = axum::response::Response::builder().status(status);

        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }

        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())));
        let usage_collector = {
            let state = state.clone();
            let provider_id = provider.id.clone();
            let fallback_model = gemini_model.clone();
            let status_code = status.as_u16();
            let start_time_clone = start_time;
            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_gemini_stream_chunks(&events) {
                    // 优先使用响应中的实际模型名称，否则使用从 URI 提取的模型名称
                    let model = usage
                        .model
                        .clone()
                        .unwrap_or_else(|| fallback_model.clone());
                    let latency_ms = start_time_clone.elapsed().as_millis() as u64;
                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "gemini",
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
                    log::debug!("[Gemini] 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };
        let logged_stream =
            create_logged_passthrough_stream(stream, "Gemini", Some(usage_collector));

        let body = axum::body::Body::from_stream(logged_stream);
        Ok(builder.body(body).unwrap())
    } else {
        // 非流式透传
        let response_headers = response.headers().clone();
        let status = response.status();

        let body_bytes = response.bytes().await.map_err(|e| {
            log::error!("[Gemini] 读取响应失败: {e}");
            ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
        })?;

        // 记录响应 JSON
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            log::info!(
                "[Gemini] <<< 响应 JSON:\n{}",
                serde_json::to_string_pretty(&json_value).unwrap_or_default()
            );

            // 记录使用量
            if let Some(usage) = TokenUsage::from_gemini_response(&json_value) {
                // 优先使用响应中的实际模型名称，否则使用从 URI 提取的模型名称
                let model = usage.model.clone().unwrap_or_else(|| gemini_model.clone());
                let latency_ms = start_time.elapsed().as_millis() as u64;
                tokio::spawn({
                    let state = state.clone();
                    let provider_id = provider.id.clone();
                    async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "gemini",
                            &model,
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
        } else {
            log::info!("[Gemini] <<< 响应 (非 JSON): {} bytes", body_bytes.len());
        }
        log::info!("[Gemini] ====== 请求结束 ======");

        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            builder = builder.header(key, value);
        }

        let body = axum::body::Body::from(body_bytes);
        Ok(builder.body(body).unwrap())
    }
}

/// 处理 /v1/responses 请求（OpenAI Responses API - Codex CLI 透传）
pub async fn handle_responses(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let start_time = std::time::Instant::now();

    let config = state.config.read().await.clone();
    let request_model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    // 选择目标 Provider
    let router = super::router::ProviderRouter::new(state.db.clone());
    let failed_ids = Vec::new();
    let provider = router.select_provider(&AppType::Codex, &failed_ids).await?;

    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
        state.current_providers.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Codex, "/v1/responses", body, headers)
        .await?;

    let status = response.status();
    log::info!("[Codex] 上游响应状态: {status}");

    // 检查是否流式响应
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_sse = content_type.contains("text/event-stream");

    if is_sse {
        // 流式透传
        log::info!("[Codex] 流式透传响应 (SSE)");
        let mut builder = axum::response::Response::builder().status(status);

        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }

        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())));
        let usage_collector = {
            let state = state.clone();
            let provider_id = provider.id.clone();
            let request_model = request_model.clone();
            let status_code = status.as_u16();
            let start_time_clone = start_time;
            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_codex_stream_events(&events) {
                    // 尝试从事件中提取模型，回退到请求模型
                    let model = events
                        .iter()
                        .find_map(|e| {
                            if e.get("type")?.as_str()? == "response.completed" {
                                e.get("response")?.get("model")?.as_str()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(&request_model)
                        .to_string();
                    let latency_ms = start_time_clone.elapsed().as_millis() as u64;

                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "codex",
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
                    log::debug!("[Codex] 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };
        let logged_stream =
            create_logged_passthrough_stream(stream, "Codex", Some(usage_collector));

        let body = axum::body::Body::from_stream(logged_stream);
        Ok(builder.body(body).unwrap())
    } else {
        // 非流式透传
        let response_headers = response.headers().clone();
        let status = response.status();

        let body_bytes = response.bytes().await.map_err(|e| {
            log::error!("[Codex] 读取响应失败: {e}");
            ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
        })?;

        // 记录响应 JSON
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            log::info!(
                "[Codex] <<< 响应 JSON:\n{}",
                serde_json::to_string_pretty(&json_value).unwrap_or_default()
            );

            // 记录使用量
            if let Some(usage) = TokenUsage::from_codex_response(&json_value) {
                let model = json_value
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                let latency_ms = start_time.elapsed().as_millis() as u64;

                log::info!(
                    "[Codex] 解析到 usage: input={}, output={}",
                    usage.input_tokens,
                    usage.output_tokens
                );

                tokio::spawn({
                    let state = state.clone();
                    let provider_id = provider.id.clone();
                    let model = model.to_string();
                    async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "codex",
                            &model,
                            usage,
                            latency_ms,
                            None,
                            false,
                            status.as_u16(),
                        )
                        .await;
                    }
                });
            } else {
                log::warn!("[Codex] 未能解析 usage 信息，跳过记录");
            }
        } else {
            log::info!("[Codex] <<< 响应 (非 JSON): {} bytes", body_bytes.len());
        }
        log::info!("[Codex] ====== 请求结束 ======");

        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            builder = builder.header(key, value);
        }

        let body = axum::body::Body::from(body_bytes);
        Ok(builder.body(body).unwrap())
    }
}

/// 处理 /v1/chat/completions 请求（OpenAI Chat Completions API - Codex CLI）
pub async fn handle_chat_completions(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ProxyError> {
    let start_time = std::time::Instant::now();
    log::info!("[Codex] ====== /v1/chat/completions 请求开始 ======");

    let config = state.config.read().await.clone();
    let request_model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    log::info!("[Codex] 请求模型: {request_model}, 流式: {is_stream}");

    // 选择目标 Provider
    let router = super::router::ProviderRouter::new(state.db.clone());
    let failed_ids = Vec::new();
    let provider = router.select_provider(&AppType::Codex, &failed_ids).await?;

    log::info!("[Codex] 选择 Provider: {}", provider.id);

    let forwarder = RequestForwarder::new(
        state.db.clone(),
        config.request_timeout,
        config.max_retries,
        state.status.clone(),
        state.current_providers.clone(),
    );

    let response = forwarder
        .forward_with_retry(&AppType::Codex, "/v1/chat/completions", body, headers)
        .await?;

    let status = response.status();
    log::info!("[Codex] 上游响应状态: {status}");

    // 检查是否流式响应
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_sse = content_type.contains("text/event-stream");

    if is_sse {
        // 流式透传
        log::info!("[Codex] 流式透传响应 (SSE)");
        let mut builder = axum::response::Response::builder().status(status);

        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }

        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())));

        let usage_collector = {
            let state = state.clone();
            let provider_id = provider.id.clone();
            let request_model = request_model.clone();
            let status_code = status.as_u16();
            let start_time_clone = start_time;
            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_openai_stream_events(&events) {
                    let model = events
                        .iter()
                        .find_map(|e| e.get("model")?.as_str())
                        .unwrap_or(&request_model)
                        .to_string();
                    let latency_ms = start_time_clone.elapsed().as_millis() as u64;

                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "codex",
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
                    log::debug!("[Codex] 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };
        let logged_stream =
            create_logged_passthrough_stream(stream, "Codex", Some(usage_collector));

        let body = axum::body::Body::from_stream(logged_stream);
        Ok(builder.body(body).unwrap())
    } else {
        // 非流式透传
        let response_headers = response.headers().clone();
        let status = response.status();

        let body_bytes = response.bytes().await.map_err(|e| {
            log::error!("[Codex] 读取响应失败: {e}");
            ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
        })?;

        // 记录响应 JSON
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            log::info!(
                "[Codex] <<< 响应 JSON:\n{}",
                serde_json::to_string_pretty(&json_value).unwrap_or_default()
            );

            // 记录使用量 (OpenAI 格式: prompt_tokens, completion_tokens)
            if let Some(usage) = TokenUsage::from_openai_response(&json_value) {
                let model = json_value
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                let latency_ms = start_time.elapsed().as_millis() as u64;

                log::info!(
                    "[Codex] 解析到 usage: input={}, output={}",
                    usage.input_tokens,
                    usage.output_tokens
                );

                tokio::spawn({
                    let state = state.clone();
                    let provider_id = provider.id.clone();
                    let model = model.to_string();
                    async move {
                        log_usage(
                            &state,
                            &provider_id,
                            "codex",
                            &model,
                            usage,
                            latency_ms,
                            None,
                            false,
                            status.as_u16(),
                        )
                        .await;
                    }
                });
            } else {
                log::warn!("[Codex] 未能解析 usage 信息，跳过记录");
            }
        } else {
            log::info!("[Codex] <<< 响应 (非 JSON): {} bytes", body_bytes.len());
        }
        log::info!("[Codex] ====== 请求结束 ======");

        let mut builder = axum::response::Response::builder().status(status);
        for (key, value) in response_headers.iter() {
            builder = builder.header(key, value);
        }

        let body = axum::body::Body::from(body_bytes);
        Ok(builder.body(body).unwrap())
    }
}
