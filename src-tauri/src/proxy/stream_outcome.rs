//! 流式响应结局判定与流式失败的供应商记账
//!
//! ## 为什么需要这个模块
//!
//! 故障转移只在「响应提交给客户端之前」才能换供应商。响应一旦提交（首包已下发），
//! 上游仍然可能在流中途返回错误事件（`event: error` / `response.failed`），
//! 或者直接断开连接。此时客户端看到的是
//! `stream disconnected before completion: Our servers are currently overloaded...`
//! 这类报错，而代理此前把「首包已到」当作成功：
//!
//! - 该供应商不会计入失败，熔断器永远不会打开；
//! - 故障转移队列里排第二、第三家的供应商不会被启用；
//! - 客户端重试十几次，全部命中同一家已经过载的供应商。
//!
//! 因此需要在流收尾时判定真实结局，并把「上游流中途失败」按普通失败记入
//! 熔断器、健康统计和代理状态，让后续请求（含客户端自动重试）落到别的供应商。
//!
//! ## 职责边界
//!
//! - [`SseOutcomeTracker`]：纯解析逻辑，判定「正常终止 / 上游错误事件 / 被截断」。
//! - [`StreamFailureReporter`]：把失败结束写回熔断器、数据库健康度和代理状态。

use super::handler_context::RequestContext;
use super::log_codes::{fo as log_fo, rsp as log_rsp};
use super::provider_router::ProviderRouter;
use super::server::ProxyState;
use super::sse::strip_sse_field;
use super::types::ProxyStatus;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 日志与状态里保留的最大错误消息长度，避免把整个响应体写进日志。
const MAX_MESSAGE_CHARS: usize = 400;

/// 流式响应收尾后的真实结局
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamOutcome {
    /// 看到了协议终止事件（`response.completed` / `message_stop` / `[DONE]` 等）
    Completed,
    /// 上游在流内明确返回了错误事件
    ErrorEvent(String),
    /// 流已经开始，但未收到终止事件就结束（连接被截断 / 静默超时 / EOF）
    Truncated(String),
}

impl StreamOutcome {
    /// 需要计为供应商失败时返回失败原因
    pub fn failure_message(&self) -> Option<String> {
        match self {
            StreamOutcome::Completed => None,
            StreamOutcome::ErrorEvent(message) | StreamOutcome::Truncated(message) => {
                Some(message.clone())
            }
        }
    }

    /// 是否属于「上游明确报错」（用于日志区分）
    pub fn is_error_event(&self) -> bool {
        matches!(self, StreamOutcome::ErrorEvent(_))
    }
}

/// SSE 结局跟踪器
///
/// 调用方按 SSE 事件块（已用空行切分）依次喂入 [`SseOutcomeTracker::on_block`]，
/// 流结束后用 [`SseOutcomeTracker::finish`] 得到最终判定。
#[derive(Debug, Default)]
pub struct SseOutcomeTracker {
    enabled: bool,
    saw_data_event: bool,
    saw_terminal_event: bool,
    error_message: Option<String>,
}

impl SseOutcomeTracker {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }

    /// 送入一个完整的 SSE 事件块
    pub fn on_block(&mut self, block: &str) {
        if !self.enabled || block.trim().is_empty() {
            return;
        }

        let mut event_name: Option<&str> = None;
        let mut data_lines: Vec<&str> = Vec::new();
        for line in block.lines() {
            if let Some(name) = strip_sse_field(line, "event") {
                event_name = Some(name.trim());
            } else if let Some(data) = strip_sse_field(line, "data") {
                data_lines.push(data);
            }
        }
        if data_lines.is_empty() {
            // 只有注释或事件名的保活块，不参与结局判定
            return;
        }

        let raw = data_lines.join("\n");
        let trimmed = raw.trim();
        self.saw_data_event = true;

        if trimmed == "[DONE]" {
            self.saw_terminal_event = true;
            return;
        }

        // 快速路径：绝大多数增量事件不含这些关键字，跳过 JSON 解析。
        let error_hint = matches!(event_name, Some("error") | Some("response.failed"))
            || trimmed.contains("\"error\"")
            || trimmed.contains("\"failed\"");
        if error_hint {
            if let Some(message) = error_event_message(trimmed, event_name) {
                if self.error_message.is_none() {
                    self.error_message = Some(truncate_message(&message));
                }
                return;
            }
        }

        if is_terminal_event(trimmed, event_name) {
            self.saw_terminal_event = true;
        }
    }

    /// 流收尾判定
    ///
    /// `transport_error` 为传输层错误描述（连接中断、首字节/静默期超时），
    /// 由调用方在读取上游失败时提供。
    pub fn finish(&self, transport_error: Option<String>) -> StreamOutcome {
        if !self.enabled {
            return StreamOutcome::Completed;
        }
        if let Some(message) = self.error_message.clone() {
            return StreamOutcome::ErrorEvent(message);
        }
        if let Some(message) = transport_error {
            return StreamOutcome::Truncated(message);
        }
        if self.saw_data_event && !self.saw_terminal_event {
            return StreamOutcome::Truncated(
                "上游流在发送终止事件前结束（响应被截断）".to_string(),
            );
        }
        StreamOutcome::Completed
    }
}

/// 判定一个数据块是否为协议终止事件
fn is_terminal_event(raw: &str, event_name: Option<&str>) -> bool {
    if matches!(
        event_name,
        Some("message_stop") | Some("response.completed")
    ) {
        return true;
    }
    const TERMINAL_TYPES: &[&str] = &[
        "response.completed",
        "response.incomplete",
        "response.done",
        "message_stop",
        "message_stop_event",
    ];
    TERMINAL_TYPES.iter().any(|marker| raw.contains(marker))
}

/// 从数据块中提取「上游错误事件」的消息；不是错误事件时返回 `None`
fn error_event_message(raw: &str, event_name: Option<&str>) -> Option<String> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        // 非 JSON 载荷：只有事件名明确是 error 时才按错误处理
        return matches!(event_name, Some("error") | Some("response.failed"))
            .then(|| raw.to_string());
    };

    let named_error = matches!(event_name, Some("error") | Some("response.failed"));
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);

    // Responses API：{"type":"response.failed","response":{...,"status":"failed","error":{...}}}
    if let Some(response) = value.get("response") {
        let status_failed = response.get("status").and_then(Value::as_str) == Some("failed");
        if named_error || status_failed || event_type.as_deref() == Some("response.failed") {
            let error = response.get("error").unwrap_or(response);
            return Some(format_error_payload(error, "response.failed"));
        }
    }

    // 顶层错误对象：{"error":{"type":"overloaded_error","message":"..."}}
    if let Some(error) = value.get("error") {
        if !error.is_null() && error.as_object().is_some_and(|object| !object.is_empty()) {
            return Some(format_error_payload(error, "upstream_error"));
        }
    }

    // 扁平错误事件：{"type":"error","code":"server_error","message":"..."}
    if named_error || event_type.as_deref() == Some("error") {
        let kind = value
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| value.get("type").and_then(Value::as_str))
            .unwrap_or("upstream_error");
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("上游在流式响应中返回错误");
        return Some(format!("{kind}: {message}"));
    }

    // status:"failed" 但取不到 error 对象时仍视为错误
    if value.get("status").and_then(Value::as_str) == Some("failed") {
        return Some("upstream_error: 上游响应状态为 failed".to_string());
    }

    None
}

/// 把错误对象格式化成 `type: message`
fn format_error_payload(error: &Value, fallback_type: &str) -> String {
    if let Some(message) = error.as_str() {
        return format!("{fallback_type}: {message}");
    }
    let kind = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or(fallback_type);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("上游在流式响应中返回错误");
    format!("{kind}: {message}")
}

fn truncate_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX_MESSAGE_CHARS {
        return trimmed.to_string();
    }
    let mut truncated: String = trimmed.chars().take(MAX_MESSAGE_CHARS).collect();
    truncated.push('…');
    truncated
}

/// 流式失败记账器
///
/// 把「已提交给客户端的流最终失败」写回熔断器、数据库健康度和代理状态，
/// 使后续请求（或客户端自动重试）能够转移到队列里的下一家供应商。
#[derive(Clone)]
pub struct StreamFailureReporter {
    router: Arc<ProviderRouter>,
    status: Arc<RwLock<ProxyStatus>>,
    app_type: &'static str,
    provider_id: String,
    provider_name: String,
}

impl StreamFailureReporter {
    /// 按请求上下文构造：以「本次实际使用的供应商」为记账对象
    pub fn for_request(ctx: &RequestContext, state: &ProxyState) -> Self {
        Self::new(
            state.provider_router.clone(),
            state.status.clone(),
            ctx.app_type_str,
            ctx.provider.id.clone(),
            ctx.provider.name.clone(),
        )
    }

    pub fn new(
        router: Arc<ProviderRouter>,
        status: Arc<RwLock<ProxyStatus>>,
        app_type: &'static str,
        provider_id: String,
        provider_name: String,
    ) -> Self {
        Self {
            router,
            status,
            app_type,
            provider_id,
            provider_name,
        }
    }

    /// 记录一次流式失败
    ///
    /// 熔断器/数据库写入放到后台任务，避免拖住响应体流的收尾。
    pub async fn record_failure(&self, outcome: &StreamOutcome) {
        let Some(message) = outcome.failure_message() else {
            return;
        };

        let log_code = if outcome.is_error_event() {
            log_rsp::STREAM_ERROR_EVENT
        } else {
            log_rsp::STREAM_TRUNCATED
        };
        log::warn!(
            "[{}] [{log_code}] 上游流式响应未正常结束，计入供应商失败: provider={} ({}), reason={}",
            self.app_type,
            self.provider_name,
            self.provider_id,
            message
        );

        {
            let mut status = self.status.write().await;
            status.success_requests = status.success_requests.saturating_sub(1);
            status.failed_requests = status.failed_requests.saturating_add(1);
            status.last_error = Some(format!("{}: {}", self.provider_name, message));
            if status.total_requests > 0 {
                status.success_rate =
                    (status.success_requests as f32 / status.total_requests as f32) * 100.0;
            }
        }

        let router = self.router.clone();
        let app_type = self.app_type.to_string();
        let provider_id = self.provider_id.clone();
        tokio::spawn(async move {
            if let Err(error) = router
                .record_result(&provider_id, &app_type, false, false, Some(message.clone()))
                .await
            {
                log::warn!(
                    "[{app_type}] [{}] 记录流式失败结果失败: provider_id={provider_id}, error={error}",
                    log_fo::STREAM_FAILURE_RECORDED
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_event_completes_stream() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("event: response.created\ndata: {\"type\":\"response.created\"}");
        tracker.on_block(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}",
        );
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }

    #[test]
    fn done_marker_completes_chat_stream() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}");
        tracker.on_block("data: [DONE]");
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }

    #[test]
    fn anthropic_message_stop_completes_stream() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("event: message_stop\ndata: {\"type\":\"message_stop\"}");
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }

    #[test]
    fn flat_error_event_is_reported() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("event: response.created\ndata: {\"type\":\"response.created\"}");
        tracker.on_block(
            "event: error\ndata: {\"type\":\"error\",\"code\":\"server_error\",\"message\":\"Our servers are currently overloaded. Please try again later.\"}",
        );
        assert_eq!(
            tracker.finish(None),
            StreamOutcome::ErrorEvent(
                "server_error: Our servers are currently overloaded. Please try again later."
                    .to_string()
            )
        );
    }

    #[test]
    fn response_failed_event_is_reported() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block(
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"overloaded\"}}}",
        );
        assert_eq!(
            tracker.finish(None),
            StreamOutcome::ErrorEvent("server_error: overloaded".to_string())
        );
    }

    #[test]
    fn anthropic_error_event_is_reported() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block(
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}",
        );
        assert_eq!(
            tracker.finish(None),
            StreamOutcome::ErrorEvent("overloaded_error: Overloaded".to_string())
        );
    }

    #[test]
    fn stream_without_terminal_event_is_truncated() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("event: response.created\ndata: {\"type\":\"response.created\"}");
        tracker.on_block("data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}");
        assert_eq!(
            tracker.finish(None),
            StreamOutcome::Truncated("上游流在发送终止事件前结束（响应被截断）".to_string())
        );
    }

    #[test]
    fn transport_error_is_truncated() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block("data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}");
        assert_eq!(
            tracker.finish(Some("上游连接中断: connection reset".to_string())),
            StreamOutcome::Truncated("上游连接中断: connection reset".to_string())
        );
    }

    #[test]
    fn error_event_wins_over_terminal_event() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block(
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"boom\"}}}",
        );
        tracker.on_block(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}",
        );
        assert!(matches!(tracker.finish(None), StreamOutcome::ErrorEvent(_)));
    }

    #[test]
    fn null_error_field_is_not_an_error_event() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":null}}",
        );
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }

    #[test]
    fn keepalive_only_stream_is_not_reported() {
        let mut tracker = SseOutcomeTracker::new(true);
        tracker.on_block(": ping");
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }

    #[test]
    fn disabled_tracker_never_reports_failure() {
        let mut tracker = SseOutcomeTracker::new(false);
        tracker.on_block("data: {\"type\":\"error\",\"message\":\"boom\"}");
        assert_eq!(tracker.finish(None), StreamOutcome::Completed);
    }
}
