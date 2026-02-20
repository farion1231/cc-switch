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
use futures::stream::{Stream, StreamExt, TryStreamExt};
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::{
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
    log::debug!(
        "[{}] 已接收上游流式响应: status={}, headers={}",
        ctx.tag,
        status.as_u16(),
        format_headers(response.headers())
    );
    let mut builder = axum::response::Response::builder().status(status);

    // 复制响应头
    for (key, value) in response.headers() {
        builder = builder.header(key, value);
    }

    // Create usage collector before processing stream
    let usage_collector: SseUsageCollector = create_usage_collector(
        ctx,
        state,
        status.as_u16(),
        parser_config,
        ctx.request_body.clone(),
        None, // response_body will be obtained after stream processing
    );

    // 获取流式超时配置
    let timeout_config = ctx.streaming_timeout_config();

    // Use actual streaming processing and convert error types
    let stream = response.bytes_stream().map_err(|e: reqwest::Error| std::io::Error::other(e.to_string()));

    // 创建带日志和超时的透传流
    let logged_stream =
        create_logged_passthrough_stream(stream, ctx.tag, Some(usage_collector), timeout_config);

    let body = axum::body::Body::from_stream(logged_stream);
    match builder.body(body) {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("[{}] 构建流式响应失败: {}", ctx.tag, e);
            ProxyError::Internal(format!("Failed to build streaming response: {}", e)).into_response()
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
    log::debug!(
        "[{}] 已接收上游响应体: status={}, bytes={}, headers={}",
        ctx.tag,
        status.as_u16(),
        body_bytes.len(),
        format_headers(&response_headers)
    );

    log::debug!(
        "[{}] 上游响应体内容: {}",
        ctx.tag,
        String::from_utf8_lossy(&body_bytes)
    );

    // 解析并记录使用量
    let response_body = String::from_utf8_lossy(&body_bytes).to_string();
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

            spawn_log_usage(
                state,
                ctx,
                usage,
                &model,
                &ctx.request_model,
                status.as_u16(),
                false,
                ctx.request_body.clone(),
                Some(response_body),
            );
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
                &ctx.request_model,
                status.as_u16(),
                false,
                ctx.request_body.clone(),
                Some(response_body),
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
            &ctx.request_model,
            status.as_u16(),
            false,
            ctx.request_body.clone(),
            Some(response_body),
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

// Callback type: extracted data, first_token_ms, latency_ms, response_body, combined_output
type UsageCallbackWithTiming = Arc<dyn Fn(ExtractedStreamData, Option<u64>, u64, Option<String>, Option<String>) + Send + Sync + 'static>;

/// Extracted data from SSE stream - replaces storing raw events
#[derive(Default, Clone)]
pub struct ExtractedStreamData {
    /// Combined text from all delta events
    pub text: String,
    /// Message ID from response
    pub message_id: Option<String>,
    /// Stop reason (completion, length, etc.)
    pub stop_reason: Option<String>,
    /// Creation timestamp
    pub created: Option<i64>,
    /// Model name from response
    pub model: Option<String>,
    /// Input tokens (from message_start)
    pub input_tokens: u32,
    /// Output tokens (from message_delta)
    pub output_tokens: u32,
    /// Cache read tokens
    pub cache_read_tokens: u32,
    /// Cache creation tokens
    pub cache_creation_tokens: u32,
}

/// SSE 使用量收集器
#[derive(Clone)]
pub struct SseUsageCollector {
    inner: Arc<SseUsageCollectorInner>,
}

struct SseUsageCollectorInner {
    data: Mutex<ExtractedStreamData>,
    first_token_ms: Mutex<Option<u64>>,  // First token time in ms
    start_time: std::time::Instant,
    on_complete: UsageCallbackWithTiming,
    finished: AtomicBool,
    response_body: Mutex<Option<String>>,  // Complete streaming response body
}

#[allow(dead_code)]
impl SseUsageCollector {
    /// Extract metadata from SSE events
    fn extract_metadata_from_events(events: &[Value]) -> (Option<String>, Option<String>, Option<i64>) {
        let mut message_id: Option<String> = None;
        let mut stop_reason: Option<String> = None;
        let mut created: Option<i64> = None;

        for event in events {
            let event_type = event.get("type").and_then(|t| t.as_str());

            match event_type {
                // Claude format
                Some("message_start") => {
                    if let Some(msg) = event.get("message") {
                        if message_id.is_none() {
                            message_id = msg.get("id").and_then(|v| v.as_str()).map(String::from);
                        }
                        if created.is_none() {
                            created = msg.get("created").and_then(|v| v.as_i64());
                        }
                    }
                }
                Some("message_delta") => {
                    if let Some(delta) = event.get("delta") {
                        if stop_reason.is_none() {
                            stop_reason = delta.get("stop_reason").and_then(|v| v.as_str()).map(String::from);
                        }
                    }
                }
                // OpenAI format
                _ => {
                    if message_id.is_none() {
                        message_id = event.get("id").and_then(|v| v.as_str()).map(String::from);
                    }
                    if created.is_none() {
                        created = event.get("created").and_then(|v| v.as_i64());
                    }
                    if stop_reason.is_none() {
                        if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                            if let Some(choice) = choices.first() {
                                stop_reason = choice.get("finish_reason").and_then(|v| v.as_str()).map(String::from);
                            }
                        }
                    }
                }
            }
        }

        (message_id, stop_reason, created)
    }

    /// Extract text content from SSE events
    fn extract_text_from_events(events: &[Value]) -> String {
        let mut combined_text = String::new();

        for event in events {
            // 1. Extract delta.text content (Claude format: content_block_delta)
            if let Some(delta) = event.get("delta") {
                // text_delta format
                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                    combined_text.push_str(text);
                }
                // thinking_delta format (MiniMax etc)
                if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                    combined_text.push_str(thinking);
                }
                // input_json_delta format (tool use)
                if let Some(partial_json) = delta.get("partial_json").and_then(|t| t.as_str()) {
                    combined_text.push_str(partial_json);
                }
                // signature_delta format - skip, don't extract signature
                if delta.get("signature").is_some() {
                    // Skip signature
                }
                // Also check if delta itself is a string
                if let Some(text) = delta.as_str() {
                    combined_text.push_str(text);
                }
            }

            // 2. Extract choices[0].delta.content (OpenAI format)
            if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|t| t.as_str()) {
                            combined_text.push_str(content);
                        }
                    }
                }
            }

            // 3. Root-level text field
            if let Some(text) = event.get("text").and_then(|t| t.as_str()) {
                combined_text.push_str(text);
            }

            // 4. Text in content array
            if let Some(content) = event.get("content").and_then(|c| c.as_array()) {
                for item in content {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        combined_text.push_str(text);
                    }
                    // thinking content
                    if let Some(thinking) = item.get("thinking").and_then(|t| t.as_str()) {
                        combined_text.push_str(thinking);
                    }
                }
            }

            // 5. Extract content field directly from event (some formats)
            if let Some(content) = event.get("content").and_then(|c| c.as_str()) {
                combined_text.push_str(content);
            }

            // 6. Check message.content array (Claude message format)
            if let Some(message) = event.get("message") {
                if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                    for item in content {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            combined_text.push_str(text);
                        }
                        if let Some(thinking) = item.get("thinking").and_then(|t| t.as_str()) {
                            combined_text.push_str(thinking);
                        }
                    }
                }
            }
        }

        combined_text
    }

    /// Extract text content from SSE events and build final JSON response body
    pub fn build_final_response_body(events: &[Value]) -> Option<String> {
        let text = Self::extract_text_from_events(events);
        if text.is_empty() {
            return None;
        }

        // Build final JSON response body
        let final_response = serde_json::json!({
            "text": text,
            "extracted": true,
            "events_count": events.len()
        });

        serde_json::to_string(&final_response).ok()
    }

    /// 创建新的使用量收集器
    pub fn new(
        start_time: std::time::Instant,
        callback: impl Fn(ExtractedStreamData, Option<u64>, u64, Option<String>, Option<String>) + Send + Sync + 'static,
    ) -> Self {
        let on_complete: UsageCallbackWithTiming = Arc::new(callback);
        Self {
            inner: Arc::new(SseUsageCollectorInner {
                data: Mutex::new(ExtractedStreamData::default()),
                first_token_ms: Mutex::new(None),
                start_time,
                on_complete,
                finished: AtomicBool::new(false),
                response_body: Mutex::new(None),
            }),
        }
    }

    /// Set complete streaming response body
    pub async fn set_response_body(&self, body: String) {
        let mut response_body = self.inner.response_body.lock().await;
        *response_body = Some(body);
    }

    /// Push SSE event - extracts and stores data incrementally
    pub async fn push(&self, event: Value) {
        let elapsed_ms = self.inner.start_time.elapsed().as_millis() as u64;

        // Extract and store data from event
        let mut data = self.inner.data.lock().await;

        // Record first token time (event with content)
        // Check for: content_block_delta, thinking_delta, or OpenAI format (choices[0].delta.content)
        let event_type = event.get("type").and_then(|t| t.as_str());
        let is_content_event = event_type == Some("content_block_delta")
            || event_type == Some("thinking_delta")
            // OpenAI format: no type field, but has choices[0].delta.content
            || (event_type.is_none()
                && event
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .is_some());
        if is_content_event {
            let mut first_time = self.inner.first_token_ms.lock().await;
            if first_time.is_none() {
                *first_time = Some(elapsed_ms);
            }
        }

        // Extract text delta
        if let Some(delta) = event.get("delta") {
            // text_delta format
            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                data.text.push_str(text);
            }
            // thinking_delta format (MiniMax etc)
            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                data.text.push_str(thinking);
            }
            // partial_json_delta format (tool use)
            if let Some(partial_json) = delta.get("partial_json").and_then(|t| t.as_str()) {
                data.text.push_str(partial_json);
            }
        }

        // Extract metadata based on event type
        match event_type {
            // Claude format
            Some("message_start") => {
                if data.message_id.is_none() {
                    data.message_id = event.get("message").and_then(|m| m.get("id")).and_then(|v| v.as_str()).map(String::from);
                }
                if data.created.is_none() {
                    data.created = event.get("message").and_then(|m| m.get("created")).and_then(|v| v.as_i64());
                }
                // Extract model from message_start
                if data.model.is_none() {
                    data.model = event.get("message").and_then(|m| m.get("model")).and_then(|v| v.as_str()).map(String::from);
                }
                // Extract usage from message_start (Claude native format)
                if let Some(usage) = event.get("message").and_then(|m| m.get("usage")) {
                    if data.input_tokens == 0 {
                        data.input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                    if data.cache_read_tokens == 0 {
                        data.cache_read_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                    if data.cache_creation_tokens == 0 {
                        data.cache_creation_tokens = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                }
            }
            Some("content_block_start") => {
                // Extract model from content_block_start
                if data.model.is_none() {
                    data.model = event.get("message").and_then(|m| m.get("model")).and_then(|v| v.as_str()).map(String::from);
                }
            }
            Some("message_delta") => {
                if data.stop_reason.is_none() {
                    data.stop_reason = event.get("delta").and_then(|d| d.get("stop_reason")).and_then(|v| v.as_str()).map(String::from);
                }
                // Extract model from message_delta usage
                if data.model.is_none() {
                    data.model = event.get("usage").and_then(|u| u.get("model")).and_then(|v| v.as_str()).map(String::from);
                }
                // Extract usage from message_delta
                if data.output_tokens == 0 {
                    data.output_tokens = event.get("usage").and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
            }
            // OpenAI format
            _ => {
                if data.message_id.is_none() {
                    data.message_id = event.get("id").and_then(|v| v.as_str()).map(String::from);
                }
                if data.created.is_none() {
                    data.created = event.get("created").and_then(|v| v.as_i64());
                }
                if data.stop_reason.is_none() {
                    if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
                        if let Some(choice) = choices.first() {
                            data.stop_reason = choice.get("finish_reason").and_then(|v| v.as_str()).map(String::from);
                        }
                    }
                }
                // Extract usage from OpenAI format (usage field in the response)
                if data.input_tokens == 0 {
                    data.input_tokens = event.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
                if data.output_tokens == 0 {
                    data.output_tokens = event.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
                if data.cache_read_tokens == 0 {
                    data.cache_read_tokens = event.get("usage")
                        .and_then(|u| u.get("prompt_tokens_details"))
                        .and_then(|p| p.get("cached_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                }
            }
        }
    }

    /// 完成收集并触发回调
    pub async fn finish(&self) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }

        let data = {
            let mut guard = self.inner.data.lock().await;
            std::mem::take(&mut *guard)
        };

        // First token time (TTFT)
        let first_token_ms = *self.inner.first_token_ms.lock().await;

        // Total latency (for streaming, this is the end-to-end latency)
        let latency_ms = self.inner.start_time.elapsed().as_millis() as u64;

        // Streaming response body
        let response_body = self.inner.response_body.lock().await.take();

        // Build final JSON response body from extracted data
        let combined_output = Self::build_final_response_body_from_data(&data);

        // Pass: data, first_token_ms, latency_ms, response_body, combined_output
        (self.inner.on_complete)(data, first_token_ms, latency_ms, response_body, combined_output);
    }

    /// Build final JSON response body from extracted stream data
    fn build_final_response_body_from_data(data: &ExtractedStreamData) -> Option<String> {
        if data.text.is_empty() {
            return None;
        }

        let final_json = serde_json::json!({
            "text": data.text,
            "id": data.message_id,
            "stop_reason": data.stop_reason,
            "created": data.created,
            "model": data.model,
        });

        serde_json::to_string(&final_json).ok()
    }
}

// ============================================================================
// 内部辅助函数
// ============================================================================

/// Build final response body from extracted stream data
fn build_response_from_data(
    data: &ExtractedStreamData,
    fallback_response_body: Option<String>,
    usage: Option<&TokenUsage>,
    first_token_ms: Option<u64>,
) -> Option<String> {
    if data.text.is_empty() {
        return fallback_response_body;
    }

    let final_json = if let Some(usage) = usage {
        serde_json::json!({
            "text": data.text,
            "usage": {
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "cache_read_tokens": usage.cache_read_tokens,
                "cache_creation_tokens": usage.cache_creation_tokens,
            },
            "id": data.message_id,
            "stop_reason": data.stop_reason,
            "created": data.created,
            "model": usage.model.clone().unwrap_or_else(|| data.model.clone().unwrap_or_default()),
            "first_token_ms": first_token_ms,
        })
    } else {
        serde_json::json!({
            "text": data.text,
            "id": data.message_id,
            "stop_reason": data.stop_reason,
            "created": data.created,
            "model": data.model,
            "first_token_ms": first_token_ms,
        })
    };

    serde_json::to_string(&final_json).ok().or(fallback_response_body)
}

/// 创建使用量收集器
fn create_usage_collector(
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    parser_config: &UsageParserConfig,
    request_body: Option<String>,
    response_body: Option<String>,
) -> SseUsageCollector {
    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let request_model = ctx.request_model.clone();
    let app_type_str = parser_config.app_type_str;
    let tag = ctx.tag;
    let start_time = ctx.start_time;
    let model_extractor = parser_config.model_extractor;
    let session_id = ctx.session_id.clone();

    SseUsageCollector::new(start_time, move |data, first_token_ms, latency_ms, stream_response_body, _combined_output| {
        // Get model from extracted data or use model_extractor with request_model as fallback
        let model = data.model.clone().unwrap_or_else(|| model_extractor(&[], &request_model));

        // Construct TokenUsage from extracted stream data
        let usage = TokenUsage {
            input_tokens: data.input_tokens,
            output_tokens: data.output_tokens,
            cache_read_tokens: data.cache_read_tokens,
            cache_creation_tokens: data.cache_creation_tokens,
            model: data.model.clone(),
        };

        let has_usage = data.input_tokens > 0 || data.output_tokens > 0 || data.cache_read_tokens > 0 || data.cache_creation_tokens > 0;

        // Build response body using helper function
        let fallback_response_body = stream_response_body.or_else(|| response_body.clone());
        let final_body = if has_usage {
            build_response_from_data(
                &data,
                fallback_response_body,
                Some(&usage),
                first_token_ms,
            )
        } else {
            build_response_from_data(
                &data,
                fallback_response_body,
                None,
                first_token_ms,
            )
        };

        let state = state.clone();
        let provider_id = provider_id.clone();
        let session_id = session_id.clone();
        let request_model = request_model.clone();
        let request_body = request_body.clone();

        tokio::spawn(async move {
            log_usage_internal(
                &state,
                &provider_id,
                app_type_str,
                &model,
                &request_model,
                usage,
                latency_ms,
                first_token_ms,
                true, // is_streaming
                status_code,
                Some(session_id),
                request_body,
                final_body,
            )
            .await;
        });

        if !has_usage {
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
    request_model: &str,
    status_code: u16,
    is_streaming: bool,
    request_body: Option<String>,
    response_body: Option<String>,
) {
    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let app_type_str = ctx.app_type_str.to_string();
    let model = model.to_string();
    let request_model = request_model.to_string();
    let latency_ms = ctx.latency_ms();
    let session_id = ctx.session_id.clone();

    tokio::spawn(async move {
        log_usage_internal(
            &state,
            &provider_id,
            &app_type_str,
            &model,
            &request_model,
            usage,
            latency_ms,
            None, // first_token_ms
            is_streaming,
            status_code,
            Some(session_id),
            request_body,
            response_body,
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
    request_model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
    session_id: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
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
        request_model.to_string(),
        pricing_model.to_string(),
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        session_id,
        Some(app_type.to_string()), // provider_type
        is_streaming,
        request_body,
        response_body,
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
        // Temporary buffer for parsing SSE events
        let mut buffer = String::new();
        // Buffer for saving complete response body
        let mut response_body_buffer = String::new();
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
                    if is_first_chunk {
                        log::debug!(
                            "[{tag}] 已接收上游流式首包: bytes={}",
                            bytes.len()
                        );
                    }
                    is_first_chunk = false;
                    let text = String::from_utf8_lossy(&bytes);
                    // Append to complete response body buffer
                    response_body_buffer.push_str(&text);
                    // Also append to event parsing buffer
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
                                            log::debug!("[{tag}] <<< SSE 事件: {data}");
                                        } else {
                                            log::debug!("[{tag}] <<< SSE 数据: {data}");
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
                    // Set received response body even on error
                    if let Some(ref c) = collector {
                        c.set_response_body(response_body_buffer.clone()).await;
                    }
                    yield Err(std::io::Error::other(e.to_string()));
                    break;
                }
                None => {
                    // Stream ended normally (stream.next() returned None), set response body to collector
                    if let Some(ref c) = collector {
                        c.set_response_body(response_body_buffer.clone()).await;
                    }
                    break;
                }
            }
        }

        if let Some(c) = collector.take() {
            c.finish().await;
        }
    }
}

fn format_headers(headers: &HeaderMap) -> String {
    headers
        .iter()
        .map(|(key, value)| {
            let value_str = value.to_str().unwrap_or("<non-utf8>");
            format!("{key}={value_str}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::ProviderMeta;
    use crate::proxy::failover_switch::FailoverSwitchManager;
    use crate::proxy::provider_router::ProviderRouter;
    use crate::proxy::types::{ProxyConfig, ProxyStatus};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(ProxyConfig::default())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            app_handle: None,
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        }
    }

    fn seed_pricing(db: &Database) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["resp-model", "Resp Model", "1.0", "0"],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["req-model", "Req Model", "2.0", "0"],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    fn insert_provider(
        db: &Database,
        id: &str,
        app_type: &str,
        meta: ProviderMeta,
    ) -> Result<(), AppError> {
        let meta_json =
            serde_json::to_string(&meta).map_err(|e| AppError::Database(e.to_string()))?;
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, app_type, "Test Provider", "{}", meta_json],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    #[tokio::test]
    async fn test_log_usage_uses_provider_override_config() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_default_cost_multiplier(app_type, "1.5").await?;
        db.set_pricing_model_source(app_type, "response").await?;
        seed_pricing(&db)?;

        let mut meta = ProviderMeta::default();
        meta.cost_multiplier = Some("2".to_string());
        meta.pricing_model_source = Some("request".to_string());
        insert_provider(&db, "provider-1", app_type, meta)?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
        };

        log_usage_internal(
            &state,
            "provider-1",
            app_type,
            "resp-model",
            "req-model",
            usage,
            10,
            None, // first_token_ms
            false,
            200,
            None,
            None, // request_body
            None, // response_body
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (model, request_model, total_cost, cost_multiplier): (String, String, String, String) =
            conn.query_row(
                "SELECT model, request_model, total_cost_usd, cost_multiplier
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        assert_eq!(model, "resp-model");
        assert_eq!(request_model, "req-model");
        assert_eq!(
            Decimal::from_str(&cost_multiplier).unwrap(),
            Decimal::from_str("2").unwrap()
        );
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("4").unwrap()
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_log_usage_falls_back_to_global_defaults() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_default_cost_multiplier(app_type, "1.5").await?;
        db.set_pricing_model_source(app_type, "response").await?;
        seed_pricing(&db)?;

        let meta = ProviderMeta::default();
        insert_provider(&db, "provider-2", app_type, meta)?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
        };

        log_usage_internal(
            &state,
            "provider-2",
            app_type,
            "resp-model",
            "req-model",
            usage,
            10,
            None, // first_token_ms
            false,
            200,
            None,
            None, // request_body
            None, // response_body
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (total_cost, cost_multiplier): (String, String) = conn
            .query_row(
                "SELECT total_cost_usd, cost_multiplier
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-2"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        assert_eq!(
            Decimal::from_str(&cost_multiplier).unwrap(),
            Decimal::from_str("1.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("1.5").unwrap()
        );
        Ok(())
    }
}
