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

use super::sse::{sse_data, sse_lines, strip_sse_field, SseDecoder, SseFrame};
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

/// 改写一个 SSE 事件块里 data 载荷的 model 字段（允许包含结尾空行）。
///
/// 未改动时返回 `Cow::Borrowed` 原样引用，避免热路径上的无效分配；
/// `event:` / `id:` / 注释等非 data 行始终原样保留。
pub fn rewrite_sse_block_model<'a>(block: &'a str, target: &str) -> Cow<'a, str> {
    // 廉价前置：块里没有 model 字段时直接跳过 JSON 解析
    // JSON 字段名也可能写成 "mo\u0064el"；含转义时交给 JSON 解析器判断。
    if !block.contains("\"model\"") && !block.contains("\\u") {
        return Cow::Borrowed(block);
    }

    let Some(data) = sse_data(block) else {
        return Cow::Borrowed(block);
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&data) else {
        return Cow::Borrowed(block);
    };
    if !rewrite_value_model(&mut value, target) {
        return Cow::Borrowed(block);
    }
    let Ok(serialized) = serde_json::to_string(&value) else {
        return Cow::Borrowed(block);
    };

    // 只有实际回写才分配输出。多行 data 合并到最后一个 data 行，保留
    // 它与事件结束空行之间的换行符，避免混合 CR/LF 在删行后意外合成 CRLF。
    let mut remaining_data = match &data {
        Cow::Borrowed(_) => 1,
        Cow::Owned(_) => sse_lines(block)
            .enumerate()
            .filter(|(index, (line, _))| {
                let field = if *index == 0 {
                    line.strip_prefix('\u{feff}').unwrap_or(line)
                } else {
                    line
                };
                strip_sse_field(field, "data").is_some()
            })
            .count(),
    };
    let mut out = String::with_capacity(block.len());
    for (index, (line, ending)) in sse_lines(block).enumerate() {
        let field = if index == 0 {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        };
        if strip_sse_field(field, "data").is_some() {
            if field.len() != line.len() {
                out.push('\u{feff}');
            }
            remaining_data -= 1;
            if remaining_data == 0 {
                out.push_str("data: ");
                out.push_str(&serialized);
                out.push_str(ending);
            }
        } else {
            out.push_str(line);
            out.push_str(ending);
        }
    }
    Cow::Owned(out)
}

/// 包装一个字节流，逐个 SSE 事件回写 model 字段。
///
/// 按 SSE 的 LF / CRLF / CR 行结束规则增量分帧。事件前的注释心跳
/// 即时透传；data 事件等待结尾空行后回写。未修改的单 chunk
/// 事件复用原始 Bytes，跨 chunk 事件只缓冲一次。正常 EOF 的尾部原样透传。
pub fn create_model_rewriting_stream(
    stream: impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    target: String,
) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut decoder = SseDecoder::default();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    decoder.push(bytes);
                    while let Some(frame) = decoder.next_frame() {
                        yield Ok(match frame {
                            SseFrame::Passthrough(bytes) => bytes,
                            SseFrame::Event(bytes) => match std::str::from_utf8(&bytes) {
                                Ok(block) => match rewrite_sse_block_model(block, &target) {
                                    Cow::Borrowed(_) => bytes,
                                    Cow::Owned(rewritten) => Bytes::from(rewritten),
                                },
                                // 无效 UTF-8 不应被有损转换破坏；保留上游原始字节。
                                Err(_) => bytes,
                            },
                        });
                    }
                }
                Err(e) => {
                    yield Err(e);
                    return;
                }
            }
        }

        // 流正常结束：冲刷未终止的尾部残余，原样透传
        let tail = decoder.finish();
        if !tail.is_empty() {
            yield Ok(tail);
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
    use futures::FutureExt;
    use serde_json::json;

    async fn rewrite_chunks(bytes: Bytes, chunk_size: usize) -> Bytes {
        let chunks: Vec<_> = (0..bytes.len())
            .step_by(chunk_size)
            .map(|start| {
                Ok::<_, std::io::Error>(bytes.slice(start..(start + chunk_size).min(bytes.len())))
            })
            .collect();
        let output = create_model_rewriting_stream(futures::stream::iter(chunks), "alias".into())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        Bytes::from(output.concat())
    }

    #[tokio::test]
    async fn stream_preserves_content_tools_and_usage_with_all_line_endings() {
        let original = json!({
            "model": "upstream",
            "choices": [{"delta": {
                "content": "你好😀",
                "reasoning_content": "reasoning",
                "tool_calls": [{"function": {"name": "edit", "arguments": "{\"model\":\"keep-me\"}"}}]
            }}],
            "usage": {"prompt_tokens": 7}
        });
        let mut expected = original.clone();
        expected["model"] = json!("alias");
        for ending in ["\n", "\r\n", "\r"] {
            let raw = Bytes::from(format!(
                "event: chunk{ending}data: {original}{ending}{ending}"
            ));
            for size in [1, 2, 7, 4096] {
                let output = rewrite_chunks(raw.clone(), size).await;
                let data = sse_data(std::str::from_utf8(&output).unwrap()).unwrap();
                assert_eq!(serde_json::from_str::<Value>(&data).unwrap(), expected);
            }
        }
    }

    #[tokio::test]
    async fn stream_rewrites_multiline_data_without_merging_mixed_line_endings() {
        for first_ending in ["\n", "\r\n", "\r"] {
            for delimiter in ["\n\n", "\r\n\r\n", "\r\r", "\r\n\n", "\n\r"] {
                let raw = Bytes::from(format!(
                    "data: {{\"model\":\"upstream\",{first_ending}id: event-1\rdata: \"usage\":{{\"input_tokens\":7}}}}{delimiter}"
                ));
                for size in [1, 7, 4096] {
                    let output = rewrite_chunks(raw.clone(), size).await;
                    let mut decoder = SseDecoder::default();
                    decoder.push(output);
                    let mut events = vec![];
                    while let Some(frame) = decoder.next_frame() {
                        if let SseFrame::Event(bytes) = frame {
                            let data = sse_data(std::str::from_utf8(&bytes).unwrap()).unwrap();
                            events.push(serde_json::from_str::<Value>(&data).unwrap());
                        }
                    }
                    assert!(decoder.finish().is_empty());
                    assert_eq!(
                        events,
                        [json!({"model":"alias", "usage":{"input_tokens":7}})]
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn unchanged_event_reuses_original_bytes_including_invalid_utf8() {
        for raw in [
            Bytes::from_static(b"data: {\"model\":\"alias\"}\r\n\r\n"),
            Bytes::from_static(b"data: {\"text\":\"hi\"}\n\n"),
            Bytes::from_static(b"data: {\"model\":broken}\n\n"),
            Bytes::from_static(b"data: \xff\n\n"),
        ] {
            let stream = create_model_rewriting_stream(
                futures::stream::iter(vec![Ok::<_, std::io::Error>(raw.clone())]),
                "alias".into(),
            );
            futures::pin_mut!(stream);
            let output = stream.next().await.unwrap().unwrap();
            assert_eq!(output, raw);
            assert_eq!(
                output.as_ptr(),
                raw.as_ptr(),
                "unchanged event should not copy its body"
            );
            assert!(stream.next().await.is_none());
        }
    }

    #[test]
    fn heartbeat_and_cr_event_are_emitted_before_upstream_closes() {
        for raw in [
            b": keepalive\n".as_slice(),
            b": keepalive\r",
            b"data: {\"model\":\"upstream\"}\r\r",
        ] {
            let upstream =
                futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(raw))])
                    .chain(futures::stream::pending());
            let output = create_model_rewriting_stream(upstream, "alias".into());
            futures::pin_mut!(output);
            assert!(matches!(output.next().now_or_never(), Some(Some(Ok(_)))));
        }
    }

    #[tokio::test]
    async fn stream_stops_on_error_without_emitting_buffered_tail() {
        let input = futures::stream::iter(vec![
            Ok(Bytes::from_static(b"data: {\"model\":")),
            Err(std::io::Error::other("upstream failed")),
            Ok(Bytes::from_static(b"must not be polled")),
        ]);
        let output = create_model_rewriting_stream(input, "alias".into())
            .collect::<Vec<_>>()
            .await;
        assert_eq!(output.len(), 1);
        assert_eq!(
            output[0].as_ref().unwrap_err().to_string(),
            "upstream failed"
        );
    }

    #[tokio::test]
    async fn stream_rewrites_large_fragmented_event() {
        let text = "x".repeat(1024 * 1024);
        let raw = Bytes::from(format!(
            "data: {}\n\n",
            json!({"response":{"model":"upstream","output":text}})
        ));
        let output = rewrite_chunks(raw, 4096).await;
        let data = sse_data(std::str::from_utf8(&output).unwrap()).unwrap();
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["response"]["model"], "alias");
        assert_eq!(value["response"]["output"], text);
    }

    #[test]
    fn escaped_model_key_and_leading_bom_are_rewritten() {
        let block = "\u{feff}data: {\"mo\\u0064el\":\"upstream\"}\r\r";
        let output = rewrite_sse_block_model(block, "alias");
        assert!(output.starts_with('\u{feff}'));
        let data = sse_data(&output).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&data).unwrap()["model"],
            "alias"
        );
    }

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
