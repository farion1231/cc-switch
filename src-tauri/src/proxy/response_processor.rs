//! 响应处理器模块
//!
//! 统一处理流式和非流式 API 响应

use super::{
    handler_config::UsageParserConfig,
    handler_context::{RequestContext, StreamingTimeoutConfig},
    server::ProxyState,
    usage::parser::TokenUsage,
    ProxyError,
};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Mutex;

// ============================================================================
// 公共接口
// ============================================================================

/// 检测响应是否为 SSE 流式响应
#[inline]
pub fn is_sse_response(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false)
}

/// 处理流式响应
pub async fn handle_streaming(
    response: reqwest::Response,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
) -> Response {
    let status = response.status();
    let mut builder = axum::response::Response::builder().status(status);

    // 复制响应头
    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    // 创建字节流
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(|e| std::io::Error::other(e.to_string())));

    // 创建使用量收集器
    let usage_collector = create_usage_collector(ctx, state, status.as_u16(), parser_config);

    // 获取流式超时配置
    let timeout_config = ctx.streaming_timeout_config();

    // 创建带日志和超时的透传流
    let logged_stream =
        create_logged_passthrough_stream(stream, ctx.tag, Some(usage_collector), timeout_config);

    let body = axum::body::Body::from_stream(logged_stream);
    match builder.body(body) {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("[{}] 构建流式响应失败: {e}", ctx.tag);
            ProxyError::Internal(format!("Failed to build streaming response: {e}")).into_response()
        }
    }
}

/// 处理非流式响应
pub async fn handle_non_streaming(
    response: reqwest::Response,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
) -> Result<Response, ProxyError> {
    let response_headers = response.headers().clone();
    let status = response.status();

    // 读取响应体
    let body_bytes = response.bytes().await.map_err(|e| {
        log::error!("[{}] 读取响应失败: {e}", ctx.tag);
        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
    })?;

    // 解析并记录使用量
    if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
        // 解析使用量
        if let Some(usage) = (parser_config.response_parser)(&json_value) {
            // 优先使用 usage 中解析出的模型名称，其次使用响应中的 model 字段，最后回退到请求模型
            let model = if let Some(ref m) = usage.model {
                m.clone()
            } else if let Some(m) = json_value.get("model").and_then(|m| m.as_str()) {
                m.to_string()
            } else {
                ctx.request_model.clone()
            };

            spawn_log_usage(state, ctx, usage, &model, status.as_u16(), false);
        } else {
            let model = json_value
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or(&ctx.request_model)
                .to_string();
            spawn_log_usage(
                state,
                ctx,
                TokenUsage::default(),
                &model,
                status.as_u16(),
                false,
            );
            log::debug!(
                "[{}] 未能解析 usage 信息，跳过记录",
                parser_config.app_type_str
            );
        }
    } else {
        log::debug!(
            "[{}] <<< 响应 (非 JSON): {} bytes",
            ctx.tag,
            body_bytes.len()
        );
        spawn_log_usage(
            state,
            ctx,
            TokenUsage::default(),
            &ctx.request_model,
            status.as_u16(),
            false,
        );
    }

    // 构建响应
    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }

    let body = axum::body::Body::from(body_bytes);
    builder.body(body).map_err(|e| {
        log::error!("[{}] 构建响应失败: {e}", ctx.tag);
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

/// 通用响应处理入口
///
/// 根据响应类型自动选择流式或非流式处理
pub async fn process_response(
    response: reqwest::Response,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        Ok(handle_streaming(response, ctx, state, parser_config).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config).await
    }
}

// ============================================================================
// SSE 使用量收集器
// ============================================================================

type UsageCallbackWithTiming = Arc<dyn Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static>;

/// SSE 使用量收集器
#[derive(Clone)]
pub struct SseUsageCollector {
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
    /// 创建新的使用量收集器
    pub fn new(
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

    /// 推送 SSE 事件
    pub async fn push(&self, event: Value) {
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

    /// 完成收集并触发回调
    pub async fn finish(&self) {
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

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 创建使用量收集器
fn create_usage_collector(
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    parser_config: &UsageParserConfig,
) -> SseUsageCollector {
    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let request_model = ctx.request_model.clone();
    let app_type_str = parser_config.app_type_str;
    let tag = ctx.tag;
    let start_time = ctx.start_time;
    let stream_parser = parser_config.stream_parser;
    let model_extractor = parser_config.model_extractor;
    let session_id = ctx.session_id.clone();

    SseUsageCollector::new(start_time, move |events, first_token_ms| {
        if let Some(usage) = stream_parser(&events) {
            let model = model_extractor(&events, &request_model);
            let latency_ms = start_time.elapsed().as_millis() as u64;

            let state = state.clone();
            let provider_id = provider_id.clone();
            let session_id = session_id.clone();

            tokio::spawn(async move {
                log_usage_internal(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    usage,
                    latency_ms,
                    first_token_ms,
                    true, // is_streaming
                    status_code,
                    Some(session_id),
                )
                .await;
            });
        } else {
            let model = model_extractor(&events, &request_model);
            let latency_ms = start_time.elapsed().as_millis() as u64;
            let state = state.clone();
            let provider_id = provider_id.clone();
            let session_id = session_id.clone();

            tokio::spawn(async move {
                log_usage_internal(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    TokenUsage::default(),
                    latency_ms,
                    first_token_ms,
                    true, // is_streaming
                    status_code,
                    Some(session_id),
                )
                .await;
            });
            log::debug!("[{tag}] 流式响应缺少 usage 统计，跳过消费记录");
        }
    })
}

/// 异步记录使用量
fn spawn_log_usage(
    state: &ProxyState,
    ctx: &RequestContext,
    usage: TokenUsage,
    model: &str,
    status_code: u16,
    is_streaming: bool,
) {
    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let app_type_str = ctx.app_type_str.to_string();
    let model = model.to_string();
    let latency_ms = ctx.latency_ms();
    let session_id = ctx.session_id.clone();

    tokio::spawn(async move {
        log_usage_internal(
            &state,
            &provider_id,
            &app_type_str,
            &model,
            usage,
            latency_ms,
            None,
            is_streaming,
            status_code,
            Some(session_id),
        )
        .await;
    });
}

/// 内部使用量记录函数
#[allow(clippy::too_many_arguments)]
async fn log_usage_internal(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
    session_id: Option<String>,
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);

    // 获取 provider 的 cost_multiplier
    let multiplier = match state.db.get_provider_by_id(provider_id, app_type) {
        Ok(Some(p)) => {
            if let Some(meta) = p.meta {
                if let Some(cm) = meta.cost_multiplier {
                    Decimal::from_str(&cm).unwrap_or_else(|e| {
                        log::warn!(
                            "cost_multiplier 解析失败 (provider_id={provider_id}): {cm} - {e}"
                        );
                        Decimal::from(1)
                    })
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

    log::debug!(
        "[{app_type}] 记录请求日志: id={request_id}, provider={provider_id}, model={model}, streaming={is_streaming}, status={status_code}, latency_ms={latency_ms}, first_token_ms={first_token_ms:?}, session={}, input={}, output={}, cache_read={}, cache_creation={}",
        session_id.as_deref().unwrap_or("none"),
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_creation_tokens
    );

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
        session_id,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}

/// 创建带日志记录和超时控制的透传流
pub fn create_logged_passthrough_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    tag: &'static str,
    usage_collector: Option<SseUsageCollector>,
    timeout_config: StreamingTimeoutConfig,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut collector = usage_collector;
        let mut is_first_chunk = true;

        // 超时配置
        let first_byte_timeout = if timeout_config.first_byte_timeout > 0 {
            Some(Duration::from_secs(timeout_config.first_byte_timeout))
        } else {
            None
        };
        let idle_timeout = if timeout_config.idle_timeout > 0 {
            Some(Duration::from_secs(timeout_config.idle_timeout))
        } else {
            None
        };

        tokio::pin!(stream);

        loop {
            // 选择超时时间：首字节超时或静默期超时
            let timeout_duration = if is_first_chunk {
                first_byte_timeout
            } else {
                idle_timeout
            };

            let chunk_result = match timeout_duration {
                Some(duration) => {
                    match tokio::time::timeout(duration, stream.next()).await {
                        Ok(Some(chunk)) => Some(chunk),
                        Ok(None) => None, // 流结束
                        Err(_) => {
                            // 超时
                            let timeout_type = if is_first_chunk { "首字节" } else { "静默期" };
                            log::error!("[{tag}] 流式响应{}超时 ({}秒)", timeout_type, duration.as_secs());
                            yield Err(std::io::Error::other(format!("流式响应{timeout_type}超时")));
                            break;
                        }
                    }
                }
                None => stream.next().await, // 无超时限制
            };

            match chunk_result {
                Some(Ok(bytes)) => {
                    is_first_chunk = false;
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
                                            log::debug!(
                                                "[{}] <<< SSE 事件: {}",
                                                tag,
                                                data.chars().take(100).collect::<String>()
                                            );
                                        } else {
                                            log::debug!("[{tag}] <<< SSE 数据: {}", data.chars().take(100).collect::<String>());
                                        }
                                    } else {
                                        log::debug!("[{tag}] <<< SSE: [DONE]");
                                    }
                                }
                            }
                        }
                    }

                    yield Ok(bytes);
                }
                Some(Err(e)) => {
                    log::error!("[{tag}] 流错误: {e}");
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
                None => {
                    // 流正常结束
                    break;
                }
            }
        }

        if let Some(c) = collector.take() {
            c.finish().await;
        }
    }
}
