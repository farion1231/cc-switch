//! 响应处理器模块
//!
//! 统一处理流式和非流式 API 响应

use super::{
    debug_capture_store,
    handler_config::UsageParserConfig,
    handler_context::{RequestContext, StreamingTimeoutConfig},
    server::ProxyState,
    usage::parser::TokenUsage,
    ProxyError,
};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
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

const MAX_CAPTURED_RESPONSE_PREVIEW_LEN: usize = 4_000_000;
const MAX_CAPTURED_LLM_TO_AGENT_LEN: usize = 1_000_000;

#[inline]
fn is_debug_capture_enabled() -> bool {
    crate::settings::get_settings().capture_system_prompt
}

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
        );
    }

    let response_preview = String::from_utf8_lossy(&body_bytes).to_string();
    if is_debug_capture_enabled() {
        spawn_append_response_debug_capture_file(
            ctx.app_type_str.to_string(),
            ctx.session_id.clone(),
            ctx.provider.id.clone(),
            ctx.request_model.clone(),
            false,
            status.as_u16(),
            response_preview,
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
        let stream_model = model_extractor(&events, &request_model);
        let stream_preview = serde_json::to_string_pretty(&Value::Array(events.clone()))
            .unwrap_or_else(|_| Value::Array(events.clone()).to_string());
        if is_debug_capture_enabled() {
            spawn_append_response_debug_capture_file(
                app_type_str.to_string(),
                session_id.clone(),
                provider_id.clone(),
                stream_model,
                true,
                status_code,
                stream_preview,
            );
        }

        if let Some(usage) = stream_parser(&events) {
            let model = model_extractor(&events, &request_model);
            let latency_ms = start_time.elapsed().as_millis() as u64;

            let state = state.clone();
            let provider_id = provider_id.clone();
            let session_id = session_id.clone();
            let request_model = request_model.clone();

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
                )
                .await;
            });
        } else {
            let model = model_extractor(&events, &request_model);
            let latency_ms = start_time.elapsed().as_millis() as u64;
            let state = state.clone();
            let provider_id = provider_id.clone();
            let session_id = session_id.clone();
            let request_model = request_model.clone();

            tokio::spawn(async move {
                log_usage_internal(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    &request_model,
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
    request_model: &str,
    status_code: u16,
    is_streaming: bool,
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
    request_model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
    session_id: Option<String>,
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
                    if is_first_chunk {
                        log::debug!(
                            "[{tag}] 已接收上游流式首包: bytes={}",
                            bytes.len()
                        );
                    }
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

fn truncate_utf8_for_debug(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

fn append_response_debug_capture_file(
    app_type: &str,
    session_id: &str,
    provider_id: &str,
    model: &str,
    is_streaming: bool,
    status_code: u16,
    response_preview: &str,
) -> Result<(), std::io::Error> {
    let mut preview = response_preview.to_string();
    if preview.len() > MAX_CAPTURED_RESPONSE_PREVIEW_LEN {
        truncate_utf8_for_debug(&mut preview, MAX_CAPTURED_RESPONSE_PREVIEW_LEN);
    }
    let mut llm_to_agent = extract_llm_to_agent_text(&preview).unwrap_or_default();
    if llm_to_agent.len() > MAX_CAPTURED_LLM_TO_AGENT_LEN {
        truncate_utf8_for_debug(&mut llm_to_agent, MAX_CAPTURED_LLM_TO_AGENT_LEN);
    }

    let session_file = debug_capture_store::capture_session_path(app_type, session_id);
    let index_file = debug_capture_store::capture_index_path();

    let entry = format!(
        "\n===== CC SWITCH MINDTRACE LOG =====\n\
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
[llm_to_agent]\n{}\n\
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
        llm_to_agent,
        preview
    );

    debug_capture_store::append_session_debug_entry(app_type, session_id, model, "RESPONSE", &entry)
}

fn extract_llm_to_agent_text(response_preview: &str) -> Option<String> {
    let value: Value = serde_json::from_str(response_preview).ok()?;
    let mut fragments = Vec::new();
    collect_llm_response_fragments(&value, &mut fragments);

    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for fragment in fragments {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            unique.push(trimmed.to_string());
        }
    }

    if unique.is_empty() {
        None
    } else {
        Some(unique.join("\n"))
    }
}

fn collect_llm_response_fragments(value: &Value, fragments: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_llm_response_fragments(item, fragments);
            }
        }
        Value::Object(map) => {
            let role = map
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let event_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();

            if role == "assistant" {
                if let Some(content) = map.get("content") {
                    collect_llm_text_like(content, fragments);
                }
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    push_llm_text(text, fragments);
                }
            }

            if event_type.contains("delta") || event_type.contains("content") {
                if let Some(delta) = map.get("delta") {
                    collect_llm_text_like(delta, fragments);
                }
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    push_llm_text(text, fragments);
                }
            }

            if let Some(choices) = map.get("choices") {
                collect_llm_response_fragments(choices, fragments);
            }
            if let Some(message) = map.get("message") {
                collect_llm_response_fragments(message, fragments);
            }
            if let Some(output) = map.get("output") {
                collect_llm_response_fragments(output, fragments);
            }
            if let Some(data) = map.get("data") {
                collect_llm_response_fragments(data, fragments);
            }
            if let Some(content) = map.get("content") {
                collect_llm_text_like(content, fragments);
            }
            if let Some(parts) = map.get("parts") {
                collect_llm_text_like(parts, fragments);
            }
            if let Some(delta) = map.get("delta") {
                collect_llm_text_like(delta, fragments);
            }
        }
        _ => {}
    }
}

fn collect_llm_text_like(value: &Value, fragments: &mut Vec<String>) {
    match value {
        Value::String(text) => push_llm_text(text, fragments),
        Value::Array(items) => {
            for item in items {
                collect_llm_text_like(item, fragments);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                push_llm_text(text, fragments);
            }
            if let Some(text) = map.get("output_text").and_then(Value::as_str) {
                push_llm_text(text, fragments);
            }
            if let Some(content) = map.get("content") {
                collect_llm_text_like(content, fragments);
            }
            if let Some(parts) = map.get("parts") {
                collect_llm_text_like(parts, fragments);
            }
            if let Some(delta) = map.get("delta") {
                collect_llm_text_like(delta, fragments);
            }
            if let Some(message) = map.get("message") {
                collect_llm_text_like(message, fragments);
            }
        }
        _ => {}
    }
}

fn push_llm_text(text: &str, fragments: &mut Vec<String>) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        fragments.push(trimmed.to_string());
    }
}

fn spawn_append_response_debug_capture_file(
    app_type: String,
    session_id: String,
    provider_id: String,
    model: String,
    is_streaming: bool,
    status_code: u16,
    response_preview: String,
) {
    if !is_debug_capture_enabled() {
        return;
    }

    tokio::spawn(async move {
        let app_type_for_log = app_type.clone();
        let session_id_for_log = session_id.clone();

        let join_result = tokio::task::spawn_blocking(move || {
            append_response_debug_capture_file(
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
            None,
            false,
            200,
            None,
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
            None,
            false,
            200,
            None,
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

    #[test]
    #[ignore]
    fn manual_write_response_debug_file() {
        let enable = std::env::var("CC_SWITCH_WRITE_DEBUG_FILE").unwrap_or_default();
        assert_eq!(
            enable, "1",
            "set CC_SWITCH_WRITE_DEBUG_FILE=1 to run this manual response debug file test"
        );

        let session_id = std::env::var("CC_SWITCH_SEED_SESSION_ID")
            .unwrap_or_else(|_| "manual-response-debug-session".to_string());

        let response_preview = serde_json::json!({
            "id": "resp_manual_1",
            "type": "response",
            "model": "demo/manual-model",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "这是一条用于验证 llm_to_agent 的响应文本。"
                        }
                    ]
                }
            ]
        })
        .to_string();

        append_response_debug_capture_file(
            "codex",
            &session_id,
            "manual-provider",
            "demo/manual-model",
            false,
            200,
            &response_preview,
        )
        .expect("write response debug capture file");

        let path = debug_capture_store::capture_session_path("codex", &session_id);
        let file_content =
            std::fs::read_to_string(&path).expect("read response debug capture file from session path");
        assert!(file_content.contains("direction: RESPONSE"));
        assert!(file_content.contains("[llm_to_agent]"));
        assert!(file_content.contains("验证 llm_to_agent"));

        let index_path = debug_capture_store::capture_index_path();
        let index_content =
            std::fs::read_to_string(&index_path).expect("read response debug capture index file");
        assert!(index_content.contains(&session_id));
        assert!(index_content.contains(path.file_name().and_then(|n| n.to_str()).unwrap_or("")));

        println!("response_debug_capture_file_path={}", path.display());
        println!("response_debug_capture_index_path={}", index_path.display());
        println!("response_debug_session_id={session_id}");
    }
}
