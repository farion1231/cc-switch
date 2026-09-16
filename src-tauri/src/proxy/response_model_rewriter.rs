//! 响应模型回写模块
//!
//! 代理在请求方向会把客户端模型映射成上游真实模型（`model_mapper` /
//! `claude_desktop_config` / Codex 上游模型目录等），上游响应里的 `model`
//! 字段因此是映射后的真值。本定制在响应出方向把它回写为客户端请求的模型名，
//! 让客户端（Claude Code / Codex 等）看到的始终是它请求的名字。
//!
//! 注意：
//! - 回写目标会剥掉 `[1m]` 本地能力标记（与请求方向的
//!   [`super::model_mapper::strip_one_m_suffix_for_upstream`] 对齐）。
//! - 所有 usage 记账都在回写之前基于原始响应完成，归因模型不受影响。

use super::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::borrow::Cow;

/// 计算响应回写目标：客户端请求的模型名，剥掉 `[1m]` 后缀。
///
/// 返回 `None` 表示不回写：
/// - 请求体没有 model 字段（`RequestContext` 用 `"unknown"` 占位）
/// - 剥离后为空串
pub fn response_model_target(request_model: &str) -> Option<String> {
    if request_model.is_empty() || request_model == "unknown" {
        return None;
    }
    let stripped = super::model_mapper::strip_one_m_suffix_for_upstream(request_model).trim();
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// 改写 JSON Value 里的 model 字段，返回是否有改动。覆盖三种形态：
/// - 顶层 `model`（Anthropic Message / OpenAI ChatCompletion / Responses 对象、
///   OpenAI SSE chunk）
/// - `message.model`（Anthropic SSE `message_start` 事件）
/// - `response.model`（Responses SSE `response.created`/`completed` 等事件）
///
/// 已是目标值的字段不动，避免无谓的重序列化。
pub fn rewrite_value_model(value: &mut Value, target: &str) -> bool {
    let mut changed = false;

    if value
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|m| m != target)
    {
        value["model"] = Value::String(target.to_string());
        changed = true;
    }

    for key in ["message", "response"] {
        let Some(nested) = value.get_mut(key) else {
            continue;
        };
        if nested
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|m| m != target)
        {
            nested["model"] = Value::String(target.to_string());
            changed = true;
        }
    }

    changed
}

/// 改写非流式响应体（JSON bytes）里的 model 字段。
///
/// 返回 `Some(重写后的字节)` 表示发生了改动；`None` 表示无需改动（非 JSON、
/// 无 model 字段、或已是目标值），调用方应原样透传。
pub fn rewrite_json_body_model(body: &[u8], target: &str) -> Option<Vec<u8>> {
    // 廉价前置：JSON 对象才可能有 model 字段
    let first = body.iter().find(|b| !b.is_ascii_whitespace())?;
    if *first != b'{' {
        return None;
    }

    let mut value: Value = serde_json::from_slice(body).ok()?;
    if !rewrite_value_model(&mut value, target) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

/// 改写一个 SSE 事件块（不含结尾空行）里 data 载荷的 model 字段。
///
/// 未改动时返回 `Cow::Borrowed` 原样引用，避免热路径上的无效分配；
/// `event:` / `id:` / 注释等非 data 行始终原样保留。
pub fn rewrite_sse_block_model<'a>(block: &'a str, target: &str) -> Cow<'a, str> {
    // 廉价前置：块里没有 model 字段时直接跳过 JSON 解析
    if !block.contains("\"model\"") {
        return Cow::Borrowed(block);
    }

    let mut changed = false;
    let mut out = String::with_capacity(block.len());

    for (i, line) in block.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }

        let Some(data) = strip_sse_field(line, "data") else {
            out.push_str(line);
            continue;
        };
        if data.trim() == "[DONE]" || !data.contains("\"model\"") {
            out.push_str(line);
            continue;
        }

        let Ok(mut value) = serde_json::from_str::<Value>(data) else {
            out.push_str(line);
            continue;
        };
        if !rewrite_value_model(&mut value, target) {
            out.push_str(line);
            continue;
        }

        match serde_json::to_string(&value) {
            Ok(serialized) => {
                changed = true;
                out.push_str("data: ");
                out.push_str(&serialized);
            }
            // 序列化失败不该发生（刚解析成功的 Value），兜底保留原始行
            Err(_) => out.push_str(line),
        }
    }

    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(block)
    }
}

/// 包装一个字节流，逐个 SSE 事件回写 model 字段。
///
/// 事件边界按 `\n\n` / `\r\n\r\n` 切分（与 [`take_sse_block`] 一致），
/// 未到结尾空行的数据会缓冲等待——SSE 事件在分隔符之前对客户端本就不可见，
/// 因此这不改变可观察的时序语义。流尾未终止的残余数据原样透传。
pub fn create_model_rewriting_stream(
    stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    target: String,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        let rewritten = rewrite_sse_block_model(&block, &target);
                        yield Ok(Bytes::from(format!("{rewritten}\n\n")));
                    }
                }
                Err(e) => yield Err(e),
            }
        }

        // 流正常结束：冲刷未终止的尾部残余，原样透传
        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        if !buffer.is_empty() {
            yield Ok(Bytes::from(buffer));
        }
    }
}

/// 便捷包装：有回写目标时用 [`create_model_rewriting_stream`] 包装，
/// 否则原样返回。统一装箱，让各调用点的两种分支类型一致。
pub fn wrap_stream_for_model_rewrite(
    stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    target: Option<String>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    match target {
        Some(target) => Box::pin(create_model_rewriting_stream(stream, target)),
        None => Box::pin(stream),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------------
    // response_model_target
    // ------------------------------------------------------------------

    #[test]
    fn target_passthrough_plain_model() {
        assert_eq!(
            response_model_target("claude-sonnet-4-5"),
            Some("claude-sonnet-4-5".to_string())
        );
    }

    #[test]
    fn target_strips_one_m_suffix_case_insensitive() {
        assert_eq!(
            response_model_target("claude-fable-5[1m]"),
            Some("claude-fable-5".to_string())
        );
        assert_eq!(
            response_model_target("gpt-5.4-mini[1M]"),
            Some("gpt-5.4-mini".to_string())
        );
    }

    #[test]
    fn target_none_for_unknown_or_empty() {
        assert_eq!(response_model_target("unknown"), None);
        assert_eq!(response_model_target(""), None);
    }

    // ------------------------------------------------------------------
    // rewrite_value_model
    // ------------------------------------------------------------------

    #[test]
    fn rewrites_top_level_model() {
        let mut value = json!({"id": "msg_1", "type": "message", "model": "deepseek-v4-pro"});
        assert!(rewrite_value_model(&mut value, "claude-sonnet-4-5"));
        assert_eq!(value["model"], "claude-sonnet-4-5");
        assert_eq!(value["id"], "msg_1");
    }

    #[test]
    fn rewrites_nested_message_and_response_model() {
        let mut value = json!({
            "type": "message_start",
            "message": {"id": "msg_1", "model": "kimi-k2", "usage": {"input_tokens": 1}}
        });
        assert!(rewrite_value_model(&mut value, "claude-opus-4-5"));
        assert_eq!(value["message"]["model"], "claude-opus-4-5");
        assert_eq!(value["message"]["usage"]["input_tokens"], 1);

        let mut value = json!({
            "type": "response.completed",
            "response": {"id": "resp_1", "model": "glm-5.2"}
        });
        assert!(rewrite_value_model(&mut value, "gpt-5.4"));
        assert_eq!(value["response"]["model"], "gpt-5.4");
    }

    #[test]
    fn no_change_when_already_target_or_missing() {
        let mut same = json!({"model": "claude-sonnet-4-5"});
        assert!(!rewrite_value_model(&mut same, "claude-sonnet-4-5"));

        let mut no_model = json!({"type": "content_block_delta", "delta": {"text": "hi"}});
        assert!(!rewrite_value_model(&mut no_model, "claude-sonnet-4-5"));

        // model 不是字符串（异常形状）时不该被插入/覆盖
        let mut weird = json!({"model": {"name": "x"}});
        assert!(!rewrite_value_model(&mut weird, "claude-sonnet-4-5"));
        assert_eq!(weird["model"], json!({"name": "x"}));
    }

    // ------------------------------------------------------------------
    // rewrite_json_body_model
    // ------------------------------------------------------------------

    #[test]
    fn json_body_rewrite_roundtrip() {
        let body = br#"{"id":"msg_1","type":"message","model":"deepseek-v4-pro","content":[]}"#;
        let rewritten = rewrite_json_body_model(body, "claude-sonnet-4-5").expect("should rewrite");
        let value: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(value["model"], "claude-sonnet-4-5");
        assert_eq!(value["id"], "msg_1");
    }

    #[test]
    fn json_body_skips_non_object_and_non_json() {
        assert!(rewrite_json_body_model(b"[1,2,3]", "m").is_none());
        assert!(rewrite_json_body_model(b"not json", "m").is_none());
        assert!(rewrite_json_body_model(b"", "m").is_none());
        // 已是目标值 → None（调用方原样透传，不重新序列化）
        assert!(rewrite_json_body_model(br#"{"model":"m"}"#, "m").is_none());
    }

    // ------------------------------------------------------------------
    // rewrite_sse_block_model
    // ------------------------------------------------------------------

    #[test]
    fn sse_block_rewrites_message_start() {
        let block = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"deepseek-v4-pro\",\"usage\":{\"input_tokens\":12}}}"
        );
        let rewritten = rewrite_sse_block_model(block, "claude-sonnet-4-5");
        assert!(rewritten.contains("event: message_start"));
        let data_line = rewritten.lines().nth(1).unwrap();
        let data = strip_sse_field(data_line, "data").unwrap();
        let value: Value = serde_json::from_str(data).unwrap();
        assert_eq!(value["message"]["model"], "claude-sonnet-4-5");
        assert_eq!(value["message"]["usage"]["input_tokens"], 12);
    }

    #[test]
    fn sse_block_rewrites_openai_chunk() {
        let block = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"kimi-k2.6\",\"choices\":[]}";
        let rewritten = rewrite_sse_block_model(block, "claude-fable-5");
        let data = strip_sse_field(&rewritten, "data").unwrap();
        let value: Value = serde_json::from_str(data).unwrap();
        assert_eq!(value["model"], "claude-fable-5");
    }

    #[test]
    fn sse_block_rewrites_responses_event() {
        let block = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.4\"}}"
        );
        let rewritten = rewrite_sse_block_model(block, "claude-opus-4-5");
        assert!(rewritten.contains("event: response.completed"));
        let data = strip_sse_field(rewritten.lines().nth(1).unwrap(), "data").unwrap();
        let value: Value = serde_json::from_str(data).unwrap();
        assert_eq!(value["response"]["model"], "claude-opus-4-5");
    }

    #[test]
    fn sse_block_passthrough_cases() {
        // 无 model 字段：借用原样返回
        let no_model = "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}";
        assert!(matches!(
            rewrite_sse_block_model(no_model, "m"),
            Cow::Borrowed(_)
        ));

        // [DONE] 不动
        let done = "data: [DONE]";
        assert_eq!(rewrite_sse_block_model(done, "m"), done);

        // 注释 / keep-alive 不动
        let comment = ": keep-alive";
        assert_eq!(rewrite_sse_block_model(comment, "m"), comment);

        // data 不是合法 JSON：原样保留
        let bad_json = "data: {\"model\": broken";
        assert_eq!(rewrite_sse_block_model(bad_json, "m"), bad_json);
    }

    #[test]
    fn sse_block_multiline_event_only_rewrites_data_lines() {
        let block = concat!(
            "id: evt-1\n",
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"model\":\"deepseek-v4-pro\"}}"
        );
        let rewritten = rewrite_sse_block_model(block, "claude-sonnet-4-5");
        let lines: Vec<&str> = rewritten.lines().collect();
        assert_eq!(lines[0], "id: evt-1");
        assert_eq!(lines[1], "event: message_start");
        assert!(lines[2].contains("claude-sonnet-4-5"));
    }

    // ------------------------------------------------------------------
    // create_model_rewriting_stream
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn stream_rewrites_across_chunk_boundaries() {
        let event = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"deepseek-v4-pro\",\"content\":[]}}\n\n";
        let bytes = event.as_bytes();
        // 切成 7 字节一片，制造跨 chunk 的事件边界与 UTF-8 边界
        let chunks: Vec<Result<Bytes, std::io::Error>> = bytes
            .chunks(7)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();

        let stream = futures::stream::iter(chunks);
        let rewritten = create_model_rewriting_stream(stream, "claude-sonnet-4-5".to_string());

        use futures::StreamExt;
        let collected: Vec<u8> = rewritten
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .concat();

        let text = String::from_utf8(collected).unwrap();
        assert!(text.contains("event: message_start"));
        assert!(text.contains("\"claude-sonnet-4-5\""));
        assert!(!text.contains("deepseek-v4-pro"));
        assert!(text.ends_with("\n\n"));
    }

    #[tokio::test]
    async fn stream_passes_through_done_and_keepalive() {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(": keep-alive\n\n")),
            Ok(Bytes::from(
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"好\"}}\n\n",
            )),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];
        let stream = futures::stream::iter(chunks);
        let rewritten = create_model_rewriting_stream(stream, "claude-sonnet-4-5".to_string());

        use futures::StreamExt;
        let collected: Vec<u8> = rewritten
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .concat();

        let text = String::from_utf8(collected).unwrap();
        assert_eq!(
            text,
            ": keep-alive\n\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"好\"}}\n\ndata: [DONE]\n\n"
        );
    }

    #[tokio::test]
    async fn stream_flushes_unterminated_tail() {
        let chunks = vec![Ok::<Bytes, std::io::Error>(Bytes::from(
            "data: {\"type\":\"ping\"}",
        ))];
        let stream = futures::stream::iter(chunks);
        let rewritten = create_model_rewriting_stream(stream, "m".to_string());

        use futures::StreamExt;
        let collected: Vec<u8> = rewritten
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .concat();

        assert_eq!(
            String::from_utf8(collected).unwrap(),
            "data: {\"type\":\"ping\"}"
        );
    }
}
