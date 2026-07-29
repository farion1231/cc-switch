//! Responses SSE 流中断的有界自动重连
//!
//! forwarder 的语义预热（`validate_responses_stream_start`）只保护提交前的失败：
//! 一旦上游发出第一个 productive 事件，响应就提交给下游客户端，此后上游掐断
//! SSE（Cloudflare 空闲切断、连接池陈旧连接等）会直接以 `stream_error` 终止整轮
//! 请求。官方 Codex CLI 对同类中断用 `stream_max_retries`（默认 5 次）整请求重发
//! 吸收；本模块把同样的语义移植到代理侧。
//!
//! 代理与 CLI 的关键差异：CLI 重试时可以丢弃已渲染的部分输出重画 UI，代理无法
//! 撤回已发给客户端的字节。因此重试只在下游尚未收到任何实质内容（至多收到
//! `message_start`/`ping` 这类协议脚手架）时进行——这恰好覆盖最常见的中断窗口：
//! 预热在 reasoning `output_item.added`（不向下游产出字节）后提交，随后模型长时间
//! 静默思考，空闲切断发生时下游只收到过 `message_start`。重试用全新的转换器实例
//! 重发同一上游请求，并抑制重复的 `message_start`，对客户端完全透明；一旦转发过
//! 实质内容，行为与现状一致（错误事件透传）。
//!
//! 上游语义性失败（`response.failed`/`error` 事件）不重试：那是上游的明确判决，
//! 与 #5546 在提交边界前的处理保持同一分界。可重试与否由转换器附加的
//! [`RETRYABLE_STREAM_MARKER`] 注释行标识，而非事件里的 `error.type`——后者
//! 逐字透传自上游可控字段，不可信。

use super::streaming_responses::{
    anthropic_error_sse, anthropic_sse, create_anthropic_sse_stream_from_responses,
    RETRYABLE_STREAM_MARKER,
};
use crate::proxy::error::ProxyError;
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// 与官方 Codex CLI 的 `stream_max_retries` 默认值对齐。
pub(crate) const RESPONSES_STREAM_MAX_RETRIES: u32 = 5;

/// 重连等待期间向下游发 keepalive 的间隔。
///
/// 退避（≤3.2s）+ 重连（≤60s）+ 等待重连后首个事件（≤60s）三段串联可超过
/// 下游 `create_logged_passthrough_stream` 的单次 120s 空闲超时；每段内部
/// 都合规也可能被外层掐掉。周期性 ping 喂空闲计时器，保住合法的重试窗口。
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
type ConnectFuture = Pin<Box<dyn Future<Output = Result<ProxyResponse, ProxyError>> + Send>>;
pub(crate) type ConnectFn = Box<dyn Fn() -> ConnectFuture + Send + Sync>;

/// 重新向同一供应商发起原始上游请求的工厂。
///
/// 由 `Forwarder::forward` 在发出首次请求前捕获最终解析的 URL/headers/body 构造；
/// 每次调用发出一条全新的上游连接（认证头沿用首次请求的令牌，重试窗口只有数秒，
/// 不需要重新走令牌刷新）。
pub struct StreamReconnector {
    connect: ConnectFn,
    first_byte_timeout: Duration,
}

impl StreamReconnector {
    pub fn new(connect: ConnectFn, first_byte_timeout: Duration) -> Self {
        Self {
            connect,
            first_byte_timeout,
        }
    }

    async fn connect(&self) -> Result<ProxyResponse, ProxyError> {
        if self.first_byte_timeout.is_zero() {
            (self.connect)().await
        } else {
            tokio::time::timeout(self.first_byte_timeout, (self.connect)())
                .await
                .map_err(|_| {
                    ProxyError::Timeout(format!(
                        "Responses stream reconnect timed out after {}s",
                        self.first_byte_timeout.as_secs()
                    ))
                })?
        }
    }
}

/// Codex CLI 的标称退避序列（无抖动）：200/400/800/1600/3200ms。
fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_millis(200u64 << attempt.saturating_sub(1).min(4))
}

/// 单个转换器输出块里解析出的 Anthropic SSE 事件。
struct ScannedEvent {
    name: &'static str,
    data: Option<Value>,
    /// 事件在原始块文本中的完整片段（含结尾空行），用于选择性转发。
    raw: String,
}

/// 转换器产出的块都是本进程自己构造的 `event: X\ndata: {...}\n\n` 文本；
/// 这里做一次浅解析以便分类。解析失败的片段按"实质内容"保守处理。
fn scan_chunk(chunk: &str) -> Vec<ScannedEvent> {
    let mut buffer = chunk.to_string();
    let mut events = Vec::new();
    while let Some(block) = take_sse_block(&mut buffer) {
        if block.trim().is_empty() {
            continue;
        }
        let mut name = "";
        let mut data_lines: Vec<&str> = Vec::new();
        for line in block.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                name = event.trim();
            } else if let Some(data) = strip_sse_field(line, "data") {
                data_lines.push(data);
            }
        }
        let data = serde_json::from_str::<Value>(&data_lines.join("\n")).ok();
        let name = if name.is_empty() {
            data.as_ref()
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("")
        } else {
            name
        };
        // 分类只关心这四种身份，映射为静态名以避免借用局部 buffer。
        let name_owned: &'static str = match name {
            "message_start" => "message_start",
            "ping" => "ping",
            "error" => "error",
            _ => "",
        };
        events.push(ScannedEvent {
            name: name_owned,
            data,
            raw: format!("{block}\n\n"),
        });
    }
    // 转换器只产出完整块；万一出现残块，按实质内容原样转发，绝不吞字节。
    if !buffer.is_empty() {
        events.push(ScannedEvent {
            name: "",
            data: None,
            raw: buffer,
        });
    }
    events
}

/// 只把携带 [`RETRYABLE_STREAM_MARKER`] 注释行的错误事件视为可重试。
///
/// 该标记仅由本进程的转换器在传输层中断（`stream_error`）与无终止事件的
/// 过早 EOF（`stream_truncated`）时附加。不能改判 `error.type`：上游语义性
/// 失败的 type 逐字透传自上游可控字段，恰好叫 "stream_error" 时会被误吸收。
fn is_retryable_error_event(event: &ScannedEvent) -> bool {
    event.name == "error"
        && event
            .raw
            .lines()
            .any(|line| line == RETRYABLE_STREAM_MARKER)
}

fn retry_failure_reason(event: &ScannedEvent) -> String {
    event
        .data
        .as_ref()
        .and_then(|data| data.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or("Responses upstream stream failed")
        .to_string()
}

/// 把 Responses 上游字节流转换为 Anthropic SSE，并在下游只收到协议脚手架
/// （`message_start`/`ping`）时对可重试的流中断做有界自动重连。
///
/// `reconnector` 为 `None` 时行为与 `create_anthropic_sse_stream_from_responses`
/// 完全一致。
pub fn create_resilient_anthropic_sse_stream_from_responses(
    initial: ByteStream,
    reconnector: Option<StreamReconnector>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let Some(reconnector) = reconnector else {
            let translated = create_anthropic_sse_stream_from_responses(initial);
            futures::pin_mut!(translated);
            while let Some(item) = translated.next().await {
                yield item;
            }
            return;
        };

        let mut attempt: u32 = 0;
        let mut message_start_forwarded = false;
        let mut content_forwarded = false;
        let mut current: Option<ByteStream> = Some(initial);

        'attempts: loop {
            let byte_stream = match current.take() {
                Some(stream) => stream,
                None => break,
            };
            let translated = create_anthropic_sse_stream_from_responses(byte_stream);
            futures::pin_mut!(translated);

            // 本次尝试因可重试原因终止时的描述；None 表示尝试正常走完。
            let mut fail_reason: Option<String> = None;
            // 重连后的尝试对首个转换事件套用首包超时，防止上游连上后静默挂起。
            let mut awaiting_first_item = attempt > 0;

            loop {
                let next = if awaiting_first_item && !reconnector.first_byte_timeout.is_zero() {
                    let deadline =
                        tokio::time::Instant::now() + reconnector.first_byte_timeout;
                    let mut result = None;
                    let mut timed_out = true;
                    while tokio::time::Instant::now() < deadline {
                        let tick =
                            KEEPALIVE_INTERVAL.min(deadline - tokio::time::Instant::now());
                        match tokio::time::timeout(tick, translated.next()).await {
                            Ok(item) => {
                                result = item;
                                timed_out = false;
                                break;
                            }
                            Err(_) => {
                                if message_start_forwarded
                                    && tokio::time::Instant::now() < deadline
                                {
                                    yield Ok(anthropic_sse("ping", &json!({"type": "ping"})));
                                }
                            }
                        }
                    }
                    if timed_out {
                        fail_reason = Some(format!(
                            "reconnected stream produced no output within {}s",
                            reconnector.first_byte_timeout.as_secs()
                        ));
                        break;
                    }
                    result
                } else {
                    translated.next().await
                };
                let Some(item) = next else {
                    break;
                };
                awaiting_first_item = false;

                let chunk = match item {
                    Ok(chunk) => chunk,
                    // 当前转换器从不产出 Err；防御性透传并终止。
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };
                let text = String::from_utf8_lossy(&chunk);
                let mut forward = String::new();
                for event in scan_chunk(&text) {
                    if !content_forwarded && is_retryable_error_event(&event) {
                        fail_reason = Some(retry_failure_reason(&event));
                        break;
                    }
                    if event.name == "message_start" {
                        if message_start_forwarded {
                            // 重连后的新一轮生成会重新产生 message_start；
                            // 客户端已收到首轮的，抑制重复。
                            continue;
                        }
                        message_start_forwarded = true;
                    } else if event.name != "ping" {
                        content_forwarded = true;
                    }
                    forward.push_str(&event.raw);
                }
                if !forward.is_empty() {
                    yield Ok(Bytes::from(forward));
                }
                if fail_reason.is_some() {
                    break;
                }
            }

            let Some(mut reason) = fail_reason else {
                // 尝试正常结束（成功完成，或不可重试的错误已透传）。
                break 'attempts;
            };

            loop {
                if attempt >= RESPONSES_STREAM_MAX_RETRIES {
                    log::error!(
                        "[Claude/Responses] stream failed after {attempt} reconnect attempt(s): {reason}"
                    );
                    yield Ok(anthropic_error_sse(
                        &format!(
                            "Responses upstream stream failed after {attempt} reconnect attempt(s): {reason}"
                        ),
                        "stream_error",
                    ));
                    break 'attempts;
                }
                attempt += 1;
                log::warn!(
                    "[Claude/Responses] upstream stream dropped before any content reached the client ({reason}); reconnecting (attempt {attempt}/{RESPONSES_STREAM_MAX_RETRIES})"
                );
                if message_start_forwarded {
                    // 喂一下下游的空闲计时器（Anthropic 协议允许任意时刻出现 ping）。
                    yield Ok(anthropic_sse("ping", &json!({"type": "ping"})));
                }
                tokio::time::sleep(backoff_delay(attempt)).await;
                let connect_result = {
                    let connect = reconnector.connect();
                    futures::pin_mut!(connect);
                    loop {
                        match tokio::time::timeout(KEEPALIVE_INTERVAL, connect.as_mut()).await {
                            Ok(result) => break result,
                            Err(_) => {
                                if message_start_forwarded {
                                    yield Ok(anthropic_sse("ping", &json!({"type": "ping"})));
                                }
                            }
                        }
                    }
                };
                match connect_result {
                    Ok(response) if response.status().is_success() => {
                        current = Some(Box::pin(response.bytes_stream()));
                        continue 'attempts;
                    }
                    Ok(response) => {
                        reason = format!(
                            "reconnect got HTTP {} from upstream",
                            response.status().as_u16()
                        );
                    }
                    Err(error) => {
                        reason = format!("reconnect failed: {error}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    fn sse(event: &str, data: Value) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    fn created() -> String {
        sse(
            "response.created",
            json!({"type":"response.created","response":{"id":"resp_1","model":"gpt-5"}}),
        )
    }

    fn text_delta(text: &str) -> String {
        sse(
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","delta": text}),
        )
    }

    fn completed() -> String {
        sse(
            "response.completed",
            json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":2}}}),
        )
    }

    fn ok_chunks(parts: &[&str]) -> ByteStream {
        let items: Vec<Result<Bytes, std::io::Error>> = parts
            .iter()
            .map(|part| Ok(Bytes::from(part.to_string())))
            .collect();
        Box::pin(stream::iter(items))
    }

    fn chunks_then_error(parts: &[&str]) -> ByteStream {
        let mut items: Vec<Result<Bytes, std::io::Error>> = parts
            .iter()
            .map(|part| Ok(Bytes::from(part.to_string())))
            .collect();
        items.push(Err(std::io::Error::other("error decoding response body")));
        Box::pin(stream::iter(items))
    }

    fn streamed_response(parts: &[&str]) -> ProxyResponse {
        let items: Vec<Result<Bytes, std::io::Error>> = parts
            .iter()
            .map(|part| Ok(Bytes::from(part.to_string())))
            .collect();
        ProxyResponse::streamed(
            http::StatusCode::OK,
            http::HeaderMap::new(),
            stream::iter(items),
        )
    }

    /// 每次 connect 弹出脚本队列里的下一个结果。
    fn scripted_reconnector(
        script: Vec<Result<ProxyResponse, ProxyError>>,
    ) -> (StreamReconnector, Arc<AtomicU32>) {
        scripted_reconnector_with_timeout(script, Duration::ZERO)
    }

    fn scripted_reconnector_with_timeout(
        script: Vec<Result<ProxyResponse, ProxyError>>,
        first_byte_timeout: Duration,
    ) -> (StreamReconnector, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let script = Arc::new(Mutex::new(script));
        let reconnector = StreamReconnector::new(
            Box::new(move || {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                let next = script
                    .lock()
                    .unwrap()
                    .pop()
                    .unwrap_or_else(|| Err(ProxyError::ForwardFailed("script empty".into())));
                Box::pin(async move { next })
            }),
            first_byte_timeout,
        );
        (reconnector, calls)
    }

    async fn collect(stream: impl Stream<Item = Result<Bytes, std::io::Error>>) -> String {
        futures::pin_mut!(stream);
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            out.push_str(&String::from_utf8_lossy(&item.unwrap()));
        }
        out
    }

    #[tokio::test(start_paused = true)]
    async fn transport_drop_before_content_retries_transparently() {
        let retry_body = [created(), text_delta("hello"), completed()].concat();
        let (reconnector, calls) =
            scripted_reconnector(vec![Ok(streamed_response(&[retry_body.as_str()]))]);
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(out.matches("event: message_start").count(), 1);
        assert!(out.contains("hello"));
        assert!(out.contains("event: message_stop"));
        assert!(!out.contains("event: error"), "unexpected error in: {out}");
    }

    #[tokio::test(start_paused = true)]
    async fn premature_eof_without_terminal_event_retries() {
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let (reconnector, calls) =
            scripted_reconnector(vec![Ok(streamed_response(&[retry_body.as_str()]))]);
        // 干净 EOF：无传输错误，也没有任何终止事件（Codex CLI 同样视为可重试）。
        let first = ok_chunks(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(out.contains("event: message_stop"));
        assert!(!out.contains("event: error"), "unexpected error in: {out}");
    }

    #[tokio::test(start_paused = true)]
    async fn no_retry_after_content_reached_client() {
        let (reconnector, calls) = scripted_reconnector(vec![]);
        let first = chunks_then_error(&[[created(), text_delta("partial")].concat().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 0, "must not reconnect");
        assert!(out.contains("partial"));
        assert!(out.contains("\"type\":\"stream_error\""));
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_semantic_failure_is_not_retried() {
        let (reconnector, calls) = scripted_reconnector(vec![]);
        let failed = sse(
            "response.failed",
            json!({"type":"response.failed","response":{"status":"failed","error":{"type":"server_error","message":"backend exploded"}}}),
        );
        let first = ok_chunks(&[[created(), failed].concat().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 0, "must not reconnect");
        assert!(out.contains("backend exploded"));
    }

    #[tokio::test(start_paused = true)]
    async fn spoofed_stream_error_type_from_upstream_is_not_retried() {
        let (reconnector, calls) = scripted_reconnector(vec![]);
        // 上游语义性 error 事件的 error.type 逐字透传；恰好叫 "stream_error"
        // 也不能触发重试——只有转换器附加了 marker 注释行的中断才可重试。
        let spoofed = sse(
            "error",
            json!({"type":"error","error":{"type":"stream_error","message":"upstream says no"}}),
        );
        let first = ok_chunks(&[[created(), spoofed].concat().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 0, "must not reconnect");
        assert!(out.contains("upstream says no"), "got: {out}");
    }

    #[tokio::test(start_paused = true)]
    async fn exhausts_bounded_retries_then_surfaces_error() {
        // 每次重连都成功建立、随即掐断：耗尽全部 5 次后必须把错误交给客户端。
        let script: Vec<Result<ProxyResponse, ProxyError>> = (0..5)
            .map(|_| {
                Ok(ProxyResponse::streamed(
                    http::StatusCode::OK,
                    http::HeaderMap::new(),
                    stream::iter(vec![
                        Ok(Bytes::from(created())),
                        Err(std::io::Error::other("connection reset")),
                    ]),
                ))
            })
            .collect();
        let (reconnector, calls) = scripted_reconnector(script);
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 5);
        assert!(out.contains("after 5 reconnect attempt(s)"), "got: {out}");
        assert_eq!(out.matches("event: message_start").count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_failure_counts_as_attempt() {
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        // 脚本按 pop() 逆序消费：两次失败后第三次成功。
        let (reconnector, calls) = scripted_reconnector(vec![
            Ok(streamed_response(&[retry_body.as_str()])),
            Err(ProxyError::ForwardFailed("connect refused".into())),
            Err(ProxyError::ForwardFailed("connect refused".into())),
        ]);
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(out.contains("event: message_stop"));
        assert!(!out.contains("event: error"), "unexpected error in: {out}");
    }

    #[tokio::test(start_paused = true)]
    async fn non_success_reconnect_status_counts_as_attempt() {
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let (reconnector, calls) = scripted_reconnector(vec![
            Ok(streamed_response(&[retry_body.as_str()])),
            Ok(ProxyResponse::buffered(
                http::StatusCode::BAD_GATEWAY,
                http::HeaderMap::new(),
                Bytes::new(),
            )),
        ]);
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(out.contains("event: message_stop"));
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_ping_emitted_during_retry_when_message_start_sent() {
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let (reconnector, _calls) =
            scripted_reconnector(vec![Ok(streamed_response(&[retry_body.as_str()]))]);
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert!(
            out.contains("event: ping"),
            "expected keepalive ping: {out}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_pings_flow_during_slow_reconnect() {
        // connect 挂 25 秒才成功：期间必须周期性发 ping，否则下游 passthrough
        // 的 120s 空闲超时会在真实场景中掐掉这次合法的重试。
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let body = Arc::new(Mutex::new(Some(retry_body)));
        let reconnector = StreamReconnector::new(
            Box::new(move || {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                let body = body.lock().unwrap().take().expect("single reconnect");
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_secs(25)).await;
                    Ok(streamed_response(&[body.as_str()]))
                })
            }),
            Duration::ZERO,
        );
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(out.contains("event: message_stop"));
        // 重连前 1 次 + 25 秒连接期间（10s/20s 处）至少 2 次。
        assert!(
            out.matches("event: ping").count() >= 3,
            "expected keepalive pings during slow reconnect: {out}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn keepalive_pings_flow_while_awaiting_first_reconnected_event() {
        // 重连秒回，但新流 30 秒后才产出首个事件；等待期间同样要发 ping。
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let delayed = ProxyResponse::streamed(
            http::StatusCode::OK,
            http::HeaderMap::new(),
            stream::once(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(Bytes::from(retry_body))
            }),
        );
        let (reconnector, calls) =
            scripted_reconnector_with_timeout(vec![Ok(delayed)], Duration::from_secs(60));
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(out.contains("event: message_stop"));
        assert!(
            out.matches("event: ping").count() >= 3,
            "expected keepalive pings while awaiting first event: {out}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn silent_reconnected_stream_times_out_and_retries_again() {
        // 重连成功但新流静默挂起：首包超时后计入尝试次数并再次重连。
        let retry_body = [created(), text_delta("ok"), completed()].concat();
        let hanging = ProxyResponse::streamed(
            http::StatusCode::OK,
            http::HeaderMap::new(),
            stream::pending::<Result<Bytes, std::io::Error>>(),
        );
        // pop() 逆序消费：先挂起流，再成功流。
        let (reconnector, calls) = scripted_reconnector_with_timeout(
            vec![Ok(streamed_response(&[retry_body.as_str()])), Ok(hanging)],
            Duration::from_secs(5),
        );
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first,
            Some(reconnector),
        ))
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(out.contains("event: message_stop"));
        assert!(!out.contains("event: error"), "unexpected error in: {out}");
    }

    #[tokio::test(start_paused = true)]
    async fn passthrough_without_reconnector_preserves_current_behavior() {
        let first = chunks_then_error(&[created().as_str()]);
        let out = collect(create_resilient_anthropic_sse_stream_from_responses(
            first, None,
        ))
        .await;

        assert!(out.contains("\"type\":\"stream_error\""));
    }
}
