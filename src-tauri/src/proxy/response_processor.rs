//! 响应处理器模块
//!
//! 统一处理流式和非流式 API 响应

use super::{
    content_encoding::{decompress_body_with_limit, get_content_encoding, DecompressError},
    forwarder::ActiveConnectionGuard,
    handler_config::{StreamUsageEventFilter, UsageParserConfig},
    handler_context::{RequestContext, StreamingTimeoutConfig},
    hyper_client::{ProxyResponse, MAX_RESPONSE_BODY_BYTES},
    server::ProxyState,
    sse::{strip_sse_field, take_sse_block},
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::database::PRICING_SOURCE_REQUEST;
use axum::http::{header::HeaderMap, HeaderName};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
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
// 响应头处理
// ============================================================================

/// RFC 2616 / RFC 7230 中定义的不应被代理继续转发的响应头。
const HOP_BY_HOP_RESPONSE_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

/// 移除响应侧 hop-by-hop 头，以及 `Connection` 中点名的扩展头。
pub(crate) fn strip_hop_by_hop_response_headers(headers: &mut HeaderMap) {
    let connection_listed_headers: Vec<HeaderName> = headers
        .get_all(axum::http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter_map(|name| HeaderName::from_bytes(name.as_bytes()).ok())
        .collect();

    for name in HOP_BY_HOP_RESPONSE_HEADERS {
        headers.remove(*name);
    }

    for name in connection_listed_headers {
        headers.remove(name);
    }
}

/// 移除在重建响应体后会失真的实体头。
pub(crate) fn strip_entity_headers_for_rebuilt_body(headers: &mut HeaderMap) {
    headers.remove(axum::http::header::CONTENT_ENCODING);
    headers.remove(axum::http::header::CONTENT_LENGTH);
    headers.remove(axum::http::header::TRANSFER_ENCODING);
}

/// 读取响应体并在需要时解压，确保 headers 与返回 body 一致。
///
/// `body_timeout`: 整包超时。当非零时用 `tokio::time::timeout` 包住 `.bytes()` 调用，
/// 防止上游发完响应头后卡住 body 导致请求永远挂住。
/// 传入 `Duration::ZERO` 表示不启用超时（故障转移关闭时）。
pub(crate) async fn read_decoded_body(
    response: ProxyResponse,
    tag: &str,
    body_timeout: Duration,
) -> Result<(HeaderMap, http::StatusCode, Bytes), ProxyError> {
    let mut headers = response.headers().clone();
    let status = response.status();
    let bytes_future = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES);
    let raw_bytes = if body_timeout.is_zero() {
        bytes_future.await?
    } else {
        tokio::time::timeout(body_timeout, bytes_future)
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ))
            })??
    };

    log::debug!(
        "[{tag}] 已接收上游响应体: status={}, bytes={}, headers={}",
        status.as_u16(),
        raw_bytes.len(),
        format_headers(&headers)
    );

    let mut body_bytes = raw_bytes.clone();
    let mut decoded = false;

    if let Some(encoding) = get_content_encoding(&headers) {
        log::debug!("[{tag}] 解压非流式响应: content-encoding={encoding}");
        match decompress_body_with_limit(&encoding, &raw_bytes, MAX_RESPONSE_BODY_BYTES) {
            Ok(Some(decompressed)) => {
                // 解码器在预算耗尽处即截停，此处必然 ≤ MAX_RESPONSE_BODY_BYTES
                body_bytes = Bytes::from(decompressed);
                decoded = true;
            }
            // 不支持的编码：原样透传且保留 content-encoding 头，
            // 让下游诊断/客户端知道这仍是压缩字节
            Ok(None) => {}
            Err(DecompressError::TooLarge { .. }) => {
                return Err(ProxyError::ResponseBodyTooLarge(MAX_RESPONSE_BODY_BYTES));
            }
            Err(DecompressError::Io(e)) => {
                log::warn!("[{tag}] 解压失败 ({encoding}): {e}，使用原始数据");
            }
        }
    }

    if decoded {
        strip_entity_headers_for_rebuilt_body(&mut headers);
    }

    Ok((headers, status, body_bytes))
}

// ============================================================================
// 公共接口
// ============================================================================

/// 检测响应是否为 SSE 流式响应
#[inline]
pub fn is_sse_response(response: &ProxyResponse) -> bool {
    response.is_sse()
}

/// 处理流式响应
pub async fn handle_streaming(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Response {
    let status = response.status();
    log::debug!(
        "[{}] 已接收上游流式响应: status={}, headers={}",
        ctx.tag,
        status.as_u16(),
        format_headers(response.headers())
    );
    // 检查流式响应是否被压缩（SSE 通常不压缩，如果压缩则 SSE 解析会失败）
    if let Some(encoding) = get_content_encoding(response.headers()) {
        log::warn!(
            "[{}] 流式响应含 content-encoding={encoding}，SSE 解析可能失败。\
             上游在 accept-encoding 透传后压缩了 SSE 流。",
            ctx.tag
        );
    }

    let mut response_headers = response.headers().clone();
    strip_hop_by_hop_response_headers(&mut response_headers);

    let mut builder = axum::response::Response::builder().status(status);

    // 复制响应头
    for (key, value) in &response_headers {
        builder = builder.header(key, value);
    }

    // 创建字节流
    let stream = response.bytes_stream();

    // 创建使用量收集器；关闭 usage logging 时不要在流式热路径上解析每个 SSE event。
    let usage_collector = create_usage_collector(ctx, state, status.as_u16(), parser_config);

    // 获取流式超时配置
    let timeout_config = ctx.streaming_timeout_config();

    // 创建带日志和超时的透传流
    let logged_stream = create_logged_passthrough_stream(
        stream,
        ctx.tag,
        usage_collector,
        timeout_config,
        connection_guard,
    );

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
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    // guard 在函数 scope 内持有，整包响应读取完成后随函数返回一并 drop
    _connection_guard: Option<ActiveConnectionGuard>,
) -> Result<Response, ProxyError> {
    // 整包超时：仅在故障转移开启且配置值非零时生效
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            Duration::ZERO
        };
    let (mut response_headers, status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;
    strip_hop_by_hop_response_headers(&mut response_headers);

    log::debug!(
        "[{}] 上游响应体已接收: bytes={} (content omitted)",
        ctx.tag,
        body_bytes.len()
    );

    // 解析并记录使用量。关闭 usage logging 时直接跳过，避免非流式响应整包 JSON parse。
    if usage_logging_enabled(state) {
        if let Ok(json_value) = serde_json::from_slice::<Value>(&body_bytes) {
            // 解析使用量
            if let Some(usage) = (parser_config.response_parser)(&json_value) {
                // 归因优先级：usage 解析出的模型 → 响应 model 字段 → 映射后的出站
                // 模型（路由接管真值）→ 客户端请求模型。空字符串视为缺失。
                let model = usage
                    .model
                    .clone()
                    .filter(|m| !m.is_empty())
                    .or_else(|| {
                        json_value
                            .get("model")
                            .and_then(|m| m.as_str())
                            .filter(|m| !m.is_empty())
                            .map(str::to_string)
                    })
                    .or_else(|| ctx.outbound_model.clone())
                    .unwrap_or_else(|| ctx.request_model.clone());

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
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .or_else(|| ctx.outbound_model.clone())
                    .unwrap_or_else(|| ctx.request_model.clone());
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
                ctx.outbound_model.as_deref().unwrap_or(&ctx.request_model),
                &ctx.request_model,
                status.as_u16(),
                false,
            );
        }
    } else {
        log::debug!("[{}] usage logging 已关闭，跳过非流式 usage 解析", ctx.tag);
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
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        Ok(handle_streaming(response, ctx, state, parser_config, connection_guard).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config, connection_guard).await
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
    first_event_set: AtomicBool,
    start_time: std::time::Instant,
    on_complete: UsageCallbackWithTiming,
    should_collect: Option<StreamUsageEventFilter>,
    finished: AtomicBool,
}

impl SseUsageCollector {
    /// 创建使用量收集器；`should_collect` 用来在 hot path 跳过与 usage 无关的事件。
    pub fn new(
        start_time: std::time::Instant,
        should_collect: Option<StreamUsageEventFilter>,
        callback: impl Fn(Vec<Value>, Option<u64>) + Send + Sync + 'static,
    ) -> Self {
        let on_complete: UsageCallbackWithTiming = Arc::new(callback);
        Self {
            inner: Arc::new(SseUsageCollectorInner {
                events: Mutex::new(Vec::new()),
                first_event_time: Mutex::new(None),
                first_event_set: AtomicBool::new(false),
                start_time,
                on_complete,
                should_collect,
                finished: AtomicBool::new(false),
            }),
        }
    }

    pub fn should_collect(&self, data: &str) -> bool {
        self.inner
            .should_collect
            .map(|filter| filter(data))
            .unwrap_or(true)
    }

    /// 标记首个被收集的 SSE 事件时间，沿用 `first_token_ms` 的既有近似语义。
    async fn mark_first_collected_event_time(&self) {
        if self.inner.first_event_set.load(Ordering::Acquire) {
            return;
        }
        let mut first_time = self.inner.first_event_time.lock().await;
        if first_time.is_none() {
            *first_time = Some(std::time::Instant::now());
            self.inner.first_event_set.store(true, Ordering::Release);
        }
    }

    /// 推送 SSE 事件
    pub async fn push(&self, event: Value) {
        self.mark_first_collected_event_time().await;
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

struct SseUsageFinishGuard {
    collector: Option<SseUsageCollector>,
}

impl SseUsageFinishGuard {
    fn new(collector: SseUsageCollector) -> Self {
        Self {
            collector: Some(collector),
        }
    }

    fn disarm(&mut self) {
        self.collector = None;
    }
}

impl Drop for SseUsageFinishGuard {
    fn drop(&mut self) {
        if let Some(collector) = self.collector.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    collector.finish().await;
                });
            } else {
                log::warn!("SSE 用量收尾保护触发时 Tokio runtime 不可用，跳过异步 finish");
            }
        }
    }
}

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 创建使用量收集器
pub(crate) fn create_usage_collector(
    ctx: &RequestContext,
    state: &ProxyState,
    status_code: u16,
    parser_config: &UsageParserConfig,
) -> Option<SseUsageCollector> {
    let logging_enabled = state
        .config
        .try_read()
        .map(|c| c.enable_logging)
        .unwrap_or(true);
    if !logging_enabled {
        return None;
    }

    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let request_model = ctx.request_model.clone();
    // 流式事件缺失模型名时的归因兜底：映射后的出站模型（路由接管真值）优先，
    // 其次才是客户端请求别名
    let fallback_model = ctx
        .outbound_model
        .clone()
        .unwrap_or_else(|| ctx.request_model.clone());
    // 用 ctx 的 app_type 而不是 parser_config 的：Claude Desktop 流式透传复用
    // CLAUDE_PARSER_CONFIG（app_type_str="claude"），按 parser_config 记账会把
    // claude-desktop 的行错记到 claude 名下，导致供应商计价覆盖解析不到。
    let app_type_str = ctx.app_type_str;
    let tag = ctx.tag;
    let start_time = ctx.start_time;
    let stream_parser = parser_config.stream_parser;
    let model_extractor = parser_config.model_extractor;
    let session_id = ctx.session_id.clone();

    Some(SseUsageCollector::new(
        start_time,
        parser_config.stream_event_filter,
        move |events, first_token_ms| {
            if let Some(usage) = stream_parser(&events) {
                let model = model_extractor(&events, &fallback_model);
                let latency_ms = start_time.elapsed().as_millis() as u64;

                let state = state.clone();
                let provider_id = provider_id.clone();
                let session_id = session_id.clone();
                let request_model = request_model.clone();
                let outbound_model = fallback_model.clone();

                tokio::spawn(async move {
                    log_usage_internal(
                        &state,
                        &provider_id,
                        app_type_str,
                        &model,
                        &request_model,
                        &outbound_model,
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
                let model = model_extractor(&events, &fallback_model);
                let latency_ms = start_time.elapsed().as_millis() as u64;
                let state = state.clone();
                let provider_id = provider_id.clone();
                let session_id = session_id.clone();
                let request_model = request_model.clone();
                let outbound_model = fallback_model.clone();

                tokio::spawn(async move {
                    log_usage_internal(
                        &state,
                        &provider_id,
                        app_type_str,
                        &model,
                        &request_model,
                        &outbound_model,
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
        },
    ))
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
    // Check enable_logging before spawning the log task
    if let Ok(config) = state.config.try_read() {
        if !config.enable_logging {
            return;
        }
    }

    let state = state.clone();
    let provider_id = ctx.provider.id.clone();
    let app_type_str = ctx.app_type_str.to_string();
    let model = model.to_string();
    let request_model = request_model.to_string();
    // 「按请求计价」模式的锚点：映射后的出站模型，无映射时等于 request_model
    let outbound_model = ctx
        .outbound_model
        .clone()
        .unwrap_or_else(|| ctx.request_model.clone());
    let latency_ms = ctx.latency_ms();
    let session_id = ctx.session_id.clone();

    tokio::spawn(async move {
        log_usage_internal(
            &state,
            &provider_id,
            &app_type_str,
            &model,
            &request_model,
            &outbound_model,
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

pub(crate) fn usage_logging_enabled(state: &ProxyState) -> bool {
    state
        .config
        .try_read()
        .map(|config| config.enable_logging)
        .unwrap_or(true)
}

/// 内部使用量记录函数
///
/// `outbound_model` 是「按请求计价」模式的锚点：实际发往上游的模型
/// （路由接管映射后的真值，无映射时等于 request_model）。该模式的语义是
/// 「按代理发出的请求计价、不信任上游回显」，接管场景下发出的请求模型是
/// 映射后的 Y 而非客户端别名 X，按 X 计价会用错定价表行。
#[allow(clippy::too_many_arguments)]
async fn log_usage_internal(
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

    let logger = UsageLogger::new(&state.db);
    let (multiplier, pricing_model_source) =
        logger.resolve_pricing_config(provider_id, app_type).await;
    let pricing_model = if pricing_model_source == PRICING_SOURCE_REQUEST {
        outbound_model
    } else {
        model
    };

    let dedup_scope = super::usage::parser::dedup_scope_for_app(app_type, provider_id);
    let request_id = usage.dedup_request_id(dedup_scope);

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

/// 检查单个 SSE data JSON 是否为空的 thinking/thinking_delta 事件
///
/// 检测逻辑：
/// - content_block_start(type="thinking", thinking="")：空 thinking 块起始，需要被状态机过滤。
///   注意：状态机同时会跳过紧跟的 content_block_stop，实现"start 后紧跟 stop 且中间无 delta"
///   的精准过滤——不会误伤有内容 thinking 块。
/// - content_block_delta 中 thinking_delta 为空：只需检测 start。
fn is_empty_thinking_event(data: &str) -> bool {
    if let Ok(json) = serde_json::from_str::<Value>(data) {
        let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "content_block_start" => {
                // 空 thinking 块起始：type="thinking" 且 thinking 字段为空（或不存在），
                // 且没有 signature（有 signature 的 redacted thinking 块非空）。
                json.pointer("/content_block/type").and_then(|v| v.as_str()) == Some("thinking")
                    && json
                        .pointer("/content_block/thinking")
                        .and_then(|v| v.as_str())
                        .map(|t| t.is_empty())
                        .unwrap_or(true)
                    && json
                        .pointer("/content_block/signature")
                        .and_then(|v| v.as_str())
                        .map(|s| s.is_empty())
                        .unwrap_or(true)
            }
            "content_block_delta" => {
                // 空 thinking_delta 本身无害，由 Claude Code 自行处理
                false
            }
            _ => false,
        }
    } else {
        false
    }
}

/// 截断 Responses API 类事件（response.created/in_progress/completed/succeeded/failed）
/// 中的超大字段，防止盲传上游回显的完整 instructions（Claude Code 系统提示可达数
/// 百 KB）导致下游 SSE JSON 解析失败（422 invalid SSE data JSON）。
///
/// 只裁剪：instructions（顶层字符串）、response.instructions（response 对象内）、
/// response.input（response 对象内的对话历史）。保留其余字段以保证客户端所需的
/// id/status/model/output/usage 等信息完整。若 JSON 解析失败则原样返回。
fn trim_oversized_responses_fields(data: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(data) else {
        return data.to_string();
    };
    let Some(event_type) = value.get("type").and_then(|v| v.as_str()) else {
        return data.to_string();
    };
    // 只有 Responses API 类事件才裁剪；Anthropic 格式事件（message_start 等）不动。
    let is_responses_event = event_type == "response.created"
        || event_type == "response.in_progress"
        || event_type == "response.completed"
        || event_type == "response.succeeded"
        || event_type == "response.failed"
        || event_type == "response.incomplete"
        || event_type == "response.queued"
        || event_type == "response.output_item.done"
        || event_type == "response.output_item.added";
    // 若 data 无 type 字段，尝试从 event 行回退（部分兼容网关只有 event: 行）
    // 这里假设调用者已在 event_text 层提取了 type，data 本身总是有 type 字段
    if !is_responses_event {
        return data.to_string();
    }

    let mut changed = false;
    // 顶层 instructions
    if let Some(obj) = value.as_object_mut() {
        if obj.remove("instructions").is_some() {
            changed = true;
        }
        if obj.remove("input").is_some() {
            changed = true;
        }
    }
    // response 对象内的 instructions/input
    if let Some(resp) = value.get_mut("response") {
        if let Some(obj) = resp.as_object_mut() {
            if obj.remove("instructions").is_some() {
                changed = true;
            }
            if obj.remove("input").is_some() {
                changed = true;
            }
        }
    }
    if !changed {
        return data.to_string();
    }
    serde_json::to_string(&value).unwrap_or_else(|_| data.to_string())
}

/// 创建带日志记录、超时控制和空thinking块过滤的透传流
pub fn create_logged_passthrough_stream(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    tag: &'static str,
    usage_collector: Option<SseUsageCollector>,
    timeout_config: StreamingTimeoutConfig,
    connection_guard: Option<ActiveConnectionGuard>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
            let _conn_guard = connection_guard;
            let mut buffer = String::new();
            let mut utf8_remainder: Vec<u8> = Vec::new();
            let mut collector = usage_collector;
            let mut finish_guard = collector.clone().map(SseUsageFinishGuard::new);
            let debug_enabled = log::log_enabled!(log::Level::Debug);
            let mut is_first_chunk = true;
            // 空thinking过滤：缓存最近一次 content_block_start(thinking)，若后一个事件是
            // content_block_stop（中间无 thinking_delta），则丢弃这对事件，抑制空thinking碎块。
            // 若后续是 thinking_delta，则是正常块开头，先输出缓存的 start 再处理 delta。
            let mut pending_empty_thinking_start: Option<String> = None;

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
                let timeout_duration = if is_first_chunk {
                    first_byte_timeout
                } else {
                    idle_timeout
                };

                let chunk_result = match timeout_duration {
                    Some(duration) => {
                        match tokio::time::timeout(duration, stream.next()).await {
                            Ok(Some(chunk)) => Some(chunk),
                            Ok(None) => None,
                            Err(_) => {
                                let timeout_type = if is_first_chunk { "首字节" } else { "静默期" };
                                log::error!("[{tag}] 流式响应{}超时 ({}秒)", timeout_type, duration.as_secs());
                                yield Err(std::io::Error::other(format!("流式响应{timeout_type}超时")));
                                break;
                            }
                        }
                    }
                    None => stream.next().await,
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

                        // 始终进行SSE解析，不再仅用于debug
                        crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                        // 安全阀：若buffer超过1MB仍未提取到SSE块，说明非SSE响应（如错误JSON），
                        // 直接透传原始buffer内容，防止内存泄漏
                        const MAX_SSE_BUFFER: usize = 1024 * 1024;
                        if buffer.len() > MAX_SSE_BUFFER {
                            log::warn!(
                                "[{tag}] SSE buffer超过{}KB仍未收到完整事件，作为非SSE响应透传",
                                MAX_SSE_BUFFER / 1024
                            );
                            yield Ok(Bytes::from(buffer.clone().into_bytes()));
                            buffer.clear();
                            continue;
                        }

                        // 解析完整的SSE事件并按需过滤
                        while let Some(event_text) = take_sse_block(&mut buffer) {
                            if event_text.trim().is_empty() {
                                continue;
                            }

                            let mut should_yield = true;
                            // 裁剪 Responses API 类事件中的超大回显字段（instructions/input）
                            let mut trimmed_data: Option<String> = None;

                            // 检查每个data行是否为空的thinking事件
                            for line in event_text.lines() {
                                if let Some(data) = strip_sse_field(line, "data") {
                                    if data.trim() == "[DONE]" {
                                        if debug_enabled {
                                            log::debug!("[{tag}] <<< SSE: [DONE]");
                                        }
                                        should_yield = true;
                                        pending_empty_thinking_start = None;
                                        break;
                                    }

                                    // 裁剪超大回显字段，避免下游 SSE JSON 解析失败
                                    let trimmed = trim_oversized_responses_fields(data);
                                    if trimmed != data {
                                        trimmed_data = Some(trimmed);
                                    }

                                    // 空thinking对过滤：缓存 start，紧跟 stop 则丢弃，有 delta 则放行
                                    if is_empty_thinking_event(data) {
                                        pending_empty_thinking_start = Some(data.to_string());
                                        should_yield = false;
                                        if debug_enabled {
                                            log::debug!("[{tag}] [FILTER] 缓存空thinking start: {data}");
                                        }
                                    } else if let Some(pending) = pending_empty_thinking_start.take() {
                                        let current_is_stop = data.contains("\"type\":\"content_block_stop\"");
                                        if current_is_stop {
                                            should_yield = false;
                                            if debug_enabled {
                                                log::debug!("[{tag}] [FILTER] 过滤空thinking块 (start+stop无delta)");
                                            }
                                        } else {
                                            // 当前是 delta/text 等 → 先输出缓存的空start，再输出当前事件
                                            let pending_sse = format!("data: {}\n\n", pending);
                                            yield Ok(Bytes::from(pending_sse));
                                            if debug_enabled {
                                                log::debug!("[{tag}] [FILTER] 空start实为正常块开头，先输出缓存的start");
                                            }
                                        }
                                    } else {
                                        should_yield = true;

                                        // usage收集和debug日志（原逻辑）
                                        let collected = match &collector {
                                            Some(c) if c.should_collect(data) => {
                                                match serde_json::from_str::<Value>(data) {
                                                    Ok(json_value) => {
                                                        c.push(json_value).await;
                                                        true
                                                    }
    Err(_) => false,
                                                }
                                            }
                                            _ => false,
                                        };
                                        if debug_enabled {
                                            if collected {
                                                log::debug!("[{tag}] <<< SSE 事件: {data}");
                                            } else {
                                                log::debug!("[{tag}] <<< SSE 数据: {data}");
                                            }
                                        }
                                    }
                                    break;
                                }
                            }

                            if should_yield {
                                // 重建SSE块并输出（若裁剪过则替换data行）
                                let block_bytes = if let Some(trimmed) = trimmed_data {
                                    let mut out = Vec::new();
                                    for l in event_text.lines() {
                                        if strip_sse_field(l, "data").is_some() {
                                            out.extend_from_slice(b"data: ");
                                            out.extend_from_slice(trimmed.as_bytes());
                                        } else {
                                            out.extend_from_slice(l.as_bytes());
                                        }
                                        out.extend_from_slice(b"\n");
                                    }
                                    out.extend_from_slice(b"\n");
                                    out
                                } else {
                                    let mut block_bytes = event_text.into_bytes();
                                    block_bytes.extend_from_slice(b"\n\n");
                                    block_bytes
                                };
                                yield Ok(Bytes::from(block_bytes));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        log::error!("[{tag}] 流错误: {e}");
                        yield Err(std::io::Error::other(e.to_string()));
                        break;
                    }
                    None => {
                        break;
                    }
                }
            }

            if let Some(c) = collector.take() {
                c.finish().await;
            }
            if let Some(guard) = &mut finish_guard {
                guard.disarm();
            }
        }
}

fn is_safe_diagnostic_header(name: &str) -> bool {
    matches!(
        name,
        "content-type"
            | "content-encoding"
            | "content-length"
            | "retry-after"
            | "cf-ray"
            | "x-request-id"
            | "request-id"
            | "x-correlation-id"
    ) || name.starts_with("x-ratelimit-")
        || name.starts_with("ratelimit-")
}

fn bounded_header_value(value: &axum::http::HeaderValue) -> Option<String> {
    let value = value.to_str().ok()?;
    let mut bounded = value.chars().take(160).collect::<String>();
    if value.chars().count() > 160 {
        bounded.push('…');
    }
    Some(bounded)
}

fn format_headers(headers: &HeaderMap) -> String {
    let mut entries = headers
        .keys()
        .map(|key| {
            let name = key.as_str();
            if !is_safe_diagnostic_header(name) {
                return name.to_string();
            }

            let values = headers
                .get_all(key)
                .iter()
                .filter_map(bounded_header_value)
                .collect::<Vec<_>>();
            if values.is_empty() {
                name.to_string()
            } else {
                format!("{name}={}", values.join("|"))
            }
        })
        .collect::<Vec<_>>();
    entries.sort();
    format!("[{}]", entries.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::ProviderMeta;
    use crate::proxy::failover_switch::FailoverSwitchManager;
    use crate::proxy::provider_router::ProviderRouter;
    use crate::proxy::providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore,
    };
    use crate::proxy::types::{ProxyConfig, ProxyStatus};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn format_headers_keeps_only_allowlisted_diagnostic_values() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer super-secret".parse().unwrap());
        headers.insert("set-cookie", "session=cookie-secret".parse().unwrap());
        headers.insert("retry-after", "30".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "2".parse().unwrap());
        headers.insert("cf-ray", "abc123-SJC".parse().unwrap());

        let formatted = format_headers(&headers);
        assert!(formatted.contains("authorization"), "{formatted}");
        assert!(formatted.contains("set-cookie"), "{formatted}");
        assert!(formatted.contains("retry-after=30"), "{formatted}");
        assert!(formatted.contains("x-ratelimit-remaining=2"), "{formatted}");
        assert!(formatted.contains("cf-ray=abc123-SJC"), "{formatted}");
        assert!(!formatted.contains("super-secret"), "{formatted}");
        assert!(!formatted.contains("cookie-secret"), "{formatted}");
    }

    #[tokio::test]
    async fn read_decoded_body_rejects_compressed_bomb_without_full_expansion() {
        // 128 MiB+1 全零 payload 的 gzip 只有 ~130 KiB：原始读取上限拦不住它，
        // 只有解压侧的有界解码能拒绝。若解码退化为"先完整展开再比较"，
        // 展开后长度 > MAX_RESPONSE_BODY_BYTES 的 payload 会成功返回（测试失败）。
        let payload = vec![0u8; MAX_RESPONSE_BODY_BYTES + 1];
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &payload).unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(compressed.len() < MAX_RESPONSE_BODY_BYTES);

        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", "gzip".parse().unwrap());
        let response =
            ProxyResponse::buffered(http::StatusCode::OK, headers, Bytes::from(compressed));

        let result = read_decoded_body(response, "test", Duration::ZERO).await;
        assert!(
            matches!(result, Err(ProxyError::ResponseBodyTooLarge(_))),
            "压缩炸弹应被拒绝而不是完整展开: {:?}",
            result.map(|(_, _, body)| body.len())
        );
    }

    #[test]
    fn test_strip_sse_field_accepts_optional_space() {
        assert_eq!(
            super::strip_sse_field("data: {\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            super::strip_sse_field("data:{\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            super::strip_sse_field("event: message_start", "event"),
            Some("message_start")
        );
        assert_eq!(
            super::strip_sse_field("event:message_start", "event"),
            Some("message_start")
        );
        assert_eq!(super::strip_sse_field("id:1", "data"), None);
    }

    #[test]
    fn test_strip_hop_by_hop_response_headers_removes_standard_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("keep-alive"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("keep-alive"),
            axum::http::HeaderValue::from_static("timeout=5"),
        );
        headers.insert(
            axum::http::header::TRANSFER_ENCODING,
            axum::http::HeaderValue::from_static("chunked"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("proxy-connection"),
            axum::http::HeaderValue::from_static("keep-alive"),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            axum::http::HeaderValue::from_static("12"),
        );

        strip_hop_by_hop_response_headers(&mut headers);

        assert!(!headers.contains_key(axum::http::header::CONNECTION));
        assert!(!headers.contains_key("keep-alive"));
        assert!(!headers.contains_key(axum::http::header::TRANSFER_ENCODING));
        assert!(!headers.contains_key("proxy-connection"));
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE),
            Some(&axum::http::HeaderValue::from_static("application/json"))
        );
        assert_eq!(
            headers.get(axum::http::header::CONTENT_LENGTH),
            Some(&axum::http::HeaderValue::from_static("12"))
        );
    }

    #[test]
    fn test_strip_hop_by_hop_response_headers_removes_connection_listed_extensions() {
        let mut headers = HeaderMap::new();
        headers.append(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("x-trace-hop, x-debug-hop"),
        );
        headers.append(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("upgrade"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("x-trace-hop"),
            axum::http::HeaderValue::from_static("trace"),
        );
        headers.insert(
            axum::http::header::HeaderName::from_static("x-debug-hop"),
            axum::http::HeaderValue::from_static("debug"),
        );
        headers.insert(
            axum::http::header::UPGRADE,
            axum::http::HeaderValue::from_static("websocket"),
        );
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );

        strip_hop_by_hop_response_headers(&mut headers);

        assert!(!headers.contains_key(axum::http::header::CONNECTION));
        assert!(!headers.contains_key("x-trace-hop"));
        assert!(!headers.contains_key("x-debug-hop"));
        assert!(!headers.contains_key(axum::http::header::UPGRADE));
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE),
            Some(&axum::http::HeaderValue::from_static("text/event-stream"))
        );
    }

    fn build_state(db: Arc<Database>) -> ProxyState {
        ProxyState {
            db: db.clone(),
            config: Arc::new(RwLock::new(ProxyConfig::default())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(ProviderRouter::new(db.clone())),
            gemini_shadow: Arc::new(GeminiShadowStore::default()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
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

        let meta = ProviderMeta {
            cost_multiplier: Some("2".to_string()),
            pricing_model_source: Some("request".to_string()),
            ..ProviderMeta::default()
        };
        insert_provider(&db, "provider-1", app_type, meta)?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        log_usage_internal(
            &state,
            "provider-1",
            app_type,
            "resp-model",
            "req-model",
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
    async fn test_request_pricing_mode_anchors_to_outbound_model() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let app_type = "claude";

        db.set_pricing_model_source(app_type, "request").await?;
        seed_pricing(&db)?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('outbound-model', 'Outbound Model', '4.0', '0')",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        insert_provider(&db, "provider-3", app_type, ProviderMeta::default())?;

        let state = build_state(db.clone());
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        // 路由接管场景：客户端请求 req-model（$2/M），代理实际发出 outbound-model
        // （$4/M），上游回显 resp-model。「按请求计价」必须锚定实际发出的模型。
        log_usage_internal(
            &state,
            "provider-3",
            app_type,
            "resp-model",
            "req-model",
            "outbound-model",
            usage,
            10,
            None,
            false,
            200,
            None,
        )
        .await;

        let conn = crate::database::lock_conn!(db.conn);
        let (model, request_model, total_cost): (String, String, String) = conn
            .query_row(
                "SELECT model, request_model, total_cost_usd
                 FROM proxy_request_logs WHERE provider_id = ?1",
                ["provider-3"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        // model / request_model 列不受计价锚点影响
        assert_eq!(model, "resp-model");
        assert_eq!(request_model, "req-model");
        // 按 outbound-model（$4/M）计价，而不是 req-model（$2/M）或 resp-model（$1/M）
        assert_eq!(
            Decimal::from_str(&total_cost).unwrap(),
            Decimal::from_str("4").unwrap()
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_claude_desktop_inherits_claude_global_defaults() -> Result<(), AppError> {
        use crate::proxy::usage::logger::UsageLogger;

        let db = Arc::new(Database::memory()?);

        // 全局计费配置只有 claude/codex/gemini 三行；claude-desktop 的
        // 全局默认必须继承 claude，而不是静默落回工厂默认（1 / response）
        db.set_default_cost_multiplier("claude", "1.5").await?;
        db.set_pricing_model_source("claude", "request").await?;

        let logger = UsageLogger::new(&db);
        let (multiplier, source) = logger
            .resolve_pricing_config("nonexistent-provider", "claude-desktop")
            .await;

        assert_eq!(multiplier, Decimal::from_str("1.5").unwrap());
        assert_eq!(source, "request");
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
            message_id: None,
        };

        log_usage_internal(
            &state,
            "provider-2",
            app_type,
            "resp-model",
            "req-model",
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

    // ========== is_empty_thinking_event 测试 ==========

    #[test]
    fn test_empty_thinking_block_start() {
        let data = r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#;
        assert!(
            is_empty_thinking_event(data),
            "空的thinking content_block_start应被检测为空——由状态机过滤（抑制start+紧跟的stop）"
        );
    }

    #[test]
    fn test_empty_thinking_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}"#;
        assert!(
            !is_empty_thinking_event(data),
            "空的thinking_delta本身无害，不应过滤以免打乱块结构"
        );
    }

    #[test]
    fn test_non_empty_thinking_not_filtered() {
        let data = r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"实际推理内容"}}"#;
        assert!(!is_empty_thinking_event(data), "非空thinking不应被过滤");
    }

    #[test]
    fn test_non_empty_thinking_delta_not_filtered() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"推理"}}"#;
        assert!(
            !is_empty_thinking_event(data),
            "非空thinking_delta不应被过滤"
        );
    }

    #[test]
    fn test_text_content_not_filtered() {
        let data =
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#;
        assert!(!is_empty_thinking_event(data), "text block不应被过滤");
    }

    #[test]
    fn test_text_delta_not_filtered() {
        let data = r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好"}}"#;
        assert!(!is_empty_thinking_event(data), "text_delta不应被过滤");
    }

    #[test]
    fn test_content_block_stop_not_filtered() {
        let data = r#"{"type":"content_block_stop","index":0}"#;
        assert!(
            !is_empty_thinking_event(data),
            "content_block_stop不应被is_empty_thinking_event过滤（由状态机处理）"
        );
    }

    #[test]
    fn test_message_start_not_filtered() {
        let data = r#"{"type":"message_start","message":{"id":"chatcmpl-xxx","type":"message","role":"assistant","model":"glm-5.2"}}"#;
        assert!(!is_empty_thinking_event(data), "message_start不应被过滤");
    }

    #[test]
    fn test_invalid_json_not_filtered() {
        assert!(
            !is_empty_thinking_event("not valid json"),
            "无效JSON不应被过滤"
        );
    }

    // ========== trim_oversized_responses_fields 测试 ==========

    #[test]
    fn test_trim_response_in_progress_removes_instructions_and_input() {
        let data = r#"{"type":"response.in_progress","response":{"id":"resp_1","object":"response","created_at":1,"status":"in_progress","instructions":"You are Claude Code, Anthropic's official CLI...","input":[{"role":"user"}],"output":[]}}"#;
        let trimmed = trim_oversized_responses_fields(data);
        let parsed: serde_json::Value = serde_json::from_str(&trimmed).unwrap();

        // 保留必要字段
        assert_eq!(parsed["type"], "response.in_progress");
        assert_eq!(parsed["response"]["id"], "resp_1");
        assert_eq!(parsed["response"]["status"], "in_progress");
        assert_eq!(parsed["response"]["output"], serde_json::json!([]));
        // 移除超大回显字段
        assert!(parsed["response"].get("instructions").is_none());
        assert!(parsed["response"].get("input").is_none());
    }

    #[test]
    fn test_trim_top_level_instructions() {
        let data = r#"{"type":"response.created","instructions":"large system prompt","id":"resp_2","object":"response","created_at":1,"status":"in_progress","output":[]}"#;
        let trimmed = trim_oversized_responses_fields(data);
        let parsed: serde_json::Value = serde_json::from_str(&trimmed).unwrap();
        assert_eq!(parsed["type"], "response.created");
        assert_eq!(parsed["id"], "resp_2");
        assert!(parsed.get("instructions").is_none());
    }

    #[test]
    fn test_trim_does_not_touch_anthropic_events() {
        let data = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"x"}}"#;
        let trimmed = trim_oversized_responses_fields(data);
        assert_eq!(trimmed, data);
    }

    #[test]
    fn test_trim_does_not_touch_output_delta() {
        // output_item.delta 不在裁剪列表，保留（内容是真正生成结果）
        let data = r#"{"type":"response.output_item.delta","item_id":"msg_1","output_index":0,"delta":"foo"}"#;
        let trimmed = trim_oversized_responses_fields(data);
        let parsed: serde_json::Value = serde_json::from_str(&trimmed).unwrap();
        assert_eq!(parsed["delta"], "foo");
    }

    #[test]
    fn test_trim_invalid_json_passthrough() {
        let data = "not valid json";
        assert_eq!(trim_oversized_responses_fields(data), data);
    }
}
