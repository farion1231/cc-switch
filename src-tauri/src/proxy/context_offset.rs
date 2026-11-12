//! Stack 模式下按请求补齐 Claude Code 的上下文用量。
//!
//! Claude Code 只有一个全局窗口 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`（`[1m]` 之外没有按模型设窗口的
//! 办法），所有会话共用。契约里它取所有非 1M Stack 模型的最大窗口
//! （`mode::controller::stack_max_context`）。窗口 `W` 更小的模型，代理给发回 Claude Code 的
//! `input_tokens` 加上 `MAX − W`：Claude Code 在「上下文 ≥ MAX − 预留」时自动压缩，预留是固定
//! 常数，加上偏移后正好在「真实上下文 ≥ W − 预留」时压缩，和直接把 MAX 设成 W 一样。
//!
//! 偏移按每个请求的模型算，所以并行会话、`--model`、项目配置、子代理和会话中途换模型都按
//! 各自的模型，互不影响。只改发给客户端的响应；CC Switch 的用量统计在这之前按真实值记录。

use axum::body::Body;
use axum::http::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;

use crate::app_config::AppType;
use crate::live::engine::DeviceStore;
use crate::mode::stack::StackTarget;
use crate::mode::state;
use crate::proxy::error::ProxyError;

/// 这个请求要加的偏移：只对 Claude Code 带 Stack id、Claude Code 按 MAX 算窗口的模型
/// （[`StackTarget::claude_window`]）；读不到契约里的 MAX 时不加。
pub fn claude_offset(app: &AppType, target: Option<&StackTarget>) -> u64 {
    if !matches!(app, AppType::Claude) {
        return 0;
    }
    let Some(window) = target.and_then(|target| target.claude_window) else {
        return 0;
    };
    offset(contract_max(), window)
}

/// `MAX − W`。窗口不小于 MAX（契约还没跟上）时不加。
fn offset(max: Option<u64>, window: u64) -> u64 {
    max.map_or(0, |max| max.saturating_sub(window))
}

/// 契约写进 `settings.json` 的 MAX，也就是 Claude Code 正在用的值。
fn contract_max() -> Option<u64> {
    let mode = state::mode_state(&DeviceStore::for_device(), AppType::Claude.as_str())
        .map_err(|error| log::warn!("[Claude] 读取契约里的 MAX 失败，不补上下文用量: {error}"))
        .ok()?;
    match mode
        .contract?
        .exclusive
        .get(crate::mode::controller::CLAUDE_MAX_CONTEXT_ENV)?
    {
        Value::String(value) => value.trim().parse().ok(),
        Value::Number(value) => value.as_u64(),
        _ => None,
    }
}

/// 给发往 Claude Code 的成功响应加上偏移。流式改 `message_start` 和 `message_delta` 的
/// `usage.input_tokens`，整包改 `message` 的；错误响应、压缩过的响应原样返回。
pub async fn apply(response: Response, offset: u64, request_is_stream: bool) -> Response {
    if offset == 0
        || !response.status().is_success()
        || response.headers().contains_key(CONTENT_ENCODING)
    {
        return response;
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase);
    let is_sse = match content_type.as_deref() {
        Some(content_type) if content_type.contains("event-stream") => true,
        Some(content_type) if content_type.contains("json") => false,
        // 上游流式回包可能不带 Content-Type（见 `response_processor::is_sse_response`）。
        None if request_is_stream => true,
        _ => return response,
    };
    let (mut parts, body) = response.into_parts();
    parts.headers.remove(CONTENT_LENGTH);
    let body = if is_sse {
        sse_body(body, offset)
    } else {
        match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => Body::from(json_body(bytes, offset)),
            Err(error) => {
                return ProxyError::Internal(format!("Failed to read response body: {error}"))
                    .into_response()
            }
        }
    };
    Response::from_parts(parts, body)
}

fn add_to_input_tokens(usage: Option<&mut Value>, offset: u64) -> bool {
    let Some(usage) = usage else {
        return false;
    };
    let Some(tokens) = usage.get("input_tokens").and_then(Value::as_u64) else {
        return false;
    };
    usage["input_tokens"] = Value::from(tokens.saturating_add(offset));
    true
}

fn json_body(bytes: Bytes, offset: u64) -> Bytes {
    let Ok(mut message) = serde_json::from_slice::<Value>(&bytes) else {
        return bytes;
    };
    if message.get("type").and_then(Value::as_str) != Some("message")
        || !add_to_input_tokens(message.get_mut("usage"), offset)
    {
        return bytes;
    }
    serde_json::to_vec(&message)
        .map(Bytes::from)
        .unwrap_or(bytes)
}

/// 一个 SSE 事件里的 `data:` 行。`message_start` 和 `message_delta` 带 usage
/// （`message_delta` 的是累计值，会覆盖前面的，所以也要加）。其余行原样。
fn sse_line(line: &[u8], offset: u64) -> Option<Vec<u8>> {
    let content_end = line
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map_or(0, |index| index + 1);
    let (content, ending) = line.split_at(content_end);
    let payload = content.strip_prefix(b"data:")?;
    if !payload
        .windows(b"input_tokens".len())
        .any(|window| window == b"input_tokens")
    {
        return None;
    }
    let mut event: Value = serde_json::from_slice(payload).ok()?;
    let usage = match event.get("type").and_then(Value::as_str)? {
        "message_start" => event
            .get_mut("message")
            .and_then(|message| message.get_mut("usage")),
        "message_delta" => event.get_mut("usage"),
        _ => return None,
    };
    if !add_to_input_tokens(usage, offset) {
        return None;
    }
    let mut out = b"data: ".to_vec();
    out.extend(serde_json::to_vec(&event).ok()?);
    out.extend_from_slice(ending);
    Some(out)
}

/// 按行改写 SSE 字节流。没收到换行的半行留到下一块；SSE 事件以换行结束，不会因此延迟。
struct SseRewriter {
    offset: u64,
    pending: Vec<u8>,
}

impl SseRewriter {
    fn rewrite(&self, lines: &[u8]) -> Bytes {
        let mut out = Vec::with_capacity(lines.len() + 16);
        for line in lines.split_inclusive(|byte| *byte == b'\n') {
            match sse_line(line, self.offset) {
                Some(rewritten) => out.extend(rewritten),
                None => out.extend_from_slice(line),
            }
        }
        Bytes::from(out)
    }

    fn feed(&mut self, chunk: &[u8]) -> Bytes {
        self.pending.extend_from_slice(chunk);
        let Some(last) = self.pending.iter().rposition(|byte| *byte == b'\n') else {
            return Bytes::new();
        };
        let rest = self.pending.split_off(last + 1);
        let lines = std::mem::replace(&mut self.pending, rest);
        self.rewrite(&lines)
    }

    fn finish(&mut self) -> Bytes {
        let rest = std::mem::take(&mut self.pending);
        self.rewrite(&rest)
    }
}

fn sse_body(body: Body, offset: u64) -> Body {
    let mut upstream = body.into_data_stream();
    let mut rewriter = SseRewriter {
        offset,
        pending: Vec::new(),
    };
    Body::from_stream(async_stream::stream! {
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(chunk) => {
                    let out = rewriter.feed(&chunk);
                    if !out.is_empty() {
                        yield Ok::<Bytes, axum::Error>(out);
                    }
                }
                Err(error) => {
                    let rest = rewriter.finish();
                    if !rest.is_empty() {
                        yield Ok(rest);
                    }
                    yield Err(error);
                    return;
                }
            }
        }
        let rest = rewriter.finish();
        if !rest.is_empty() {
            yield Ok(rest);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::json;

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn response(status: StatusCode, content_type: Option<&str>, body: Body) -> Response {
        let mut builder = Response::builder().status(status);
        if let Some(content_type) = content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }
        builder.header(CONTENT_LENGTH, "1").body(body).unwrap()
    }

    #[test]
    fn the_offset_is_max_minus_the_models_window() {
        assert_eq!(offset(Some(400_000), 128_000), 272_000);
        assert_eq!(offset(Some(400_000), 400_000), 0);
        assert_eq!(offset(Some(200_000), 400_000), 0, "contract not caught up");
        assert_eq!(offset(None, 128_000), 0);
    }

    #[test]
    fn only_claude_requests_with_a_known_window_get_an_offset() {
        let target = |window| StackTarget {
            provider: crate::provider::Provider::with_id(
                "p".to_string(),
                "p".to_string(),
                json!({}),
                None,
            ),
            upstream_model: "m".to_string(),
            original_model: "ccs-claude-p--m".to_string(),
            claude_window: window,
        };
        assert_eq!(claude_offset(&AppType::Claude, None), 0);
        assert_eq!(claude_offset(&AppType::Claude, Some(&target(None))), 0);
        assert_eq!(
            claude_offset(&AppType::Codex, Some(&target(Some(128_000)))),
            0
        );
    }

    #[tokio::test]
    async fn a_streamed_message_reports_the_offset_on_start_and_delta() {
        let start = json!({
            "type": "message_start",
            "message": { "id": "m", "type": "message", "usage": {
                "input_tokens": 10, "cache_read_input_tokens": 90_000, "output_tokens": 1
            }}
        });
        let delta = json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn" },
            "usage": { "input_tokens": 10, "output_tokens": 50 }
        });
        let text = json!({
            "type": "content_block_delta", "index": 0,
            "delta": { "type": "text_delta", "text": "input_tokens" }
        });
        let stream = format!(
            "event: message_start\r\ndata: {start}\r\n\r\nevent: content_block_delta\ndata: {text}\n\nevent: message_delta\ndata: {delta}\n\n"
        );
        // 按 7 字节切块，覆盖行被拆开的情况。
        let chunks: Vec<Result<Bytes, std::io::Error>> = stream
            .as_bytes()
            .chunks(7)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        let upstream = response(
            StatusCode::OK,
            Some("text/event-stream"),
            Body::from_stream(futures::stream::iter(chunks)),
        );
        let out = apply(upstream, 272_000, true).await;
        assert!(out.headers().get(CONTENT_LENGTH).is_none());
        let out = body_of(out).await;

        let events: Vec<Value> = out
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str(data).unwrap())
            .collect();
        assert_eq!(events[0]["message"]["usage"]["input_tokens"], 272_010);
        assert_eq!(
            events[0]["message"]["usage"]["cache_read_input_tokens"],
            90_000
        );
        assert_eq!(events[1], text, "other events pass through");
        assert_eq!(events[2]["usage"]["input_tokens"], 272_010);
        assert_eq!(events[2]["usage"]["output_tokens"], 50);
        assert!(out.contains("event: message_start\r\ndata: "));
        assert!(out.ends_with("\n\n"));
    }

    #[tokio::test]
    async fn a_whole_message_reports_the_offset() {
        let message = json!({
            "id": "m", "type": "message", "role": "assistant", "content": [],
            "usage": { "input_tokens": 5, "output_tokens": 3 }
        });
        let upstream = response(
            StatusCode::OK,
            Some("application/json"),
            Body::from(message.to_string()),
        );
        let out: Value =
            serde_json::from_str(&body_of(apply(upstream, 100, false).await).await).unwrap();
        assert_eq!(out["usage"]["input_tokens"], 105);
        assert_eq!(out["usage"]["output_tokens"], 3);
    }

    #[tokio::test]
    async fn errors_and_zero_offsets_pass_through() {
        let error = json!({ "type": "error", "error": { "type": "overloaded_error" } });
        let upstream = response(
            StatusCode::SERVICE_UNAVAILABLE,
            Some("application/json"),
            Body::from(error.to_string()),
        );
        let out = apply(upstream, 100, false).await;
        assert_eq!(out.headers()[CONTENT_LENGTH], "1");
        assert_eq!(body_of(out).await, error.to_string());

        let message = json!({ "type": "message", "usage": { "input_tokens": 5 } }).to_string();
        let upstream = response(
            StatusCode::OK,
            Some("application/json"),
            Body::from(message.clone()),
        );
        assert_eq!(body_of(apply(upstream, 0, false).await).await, message);
    }
}
