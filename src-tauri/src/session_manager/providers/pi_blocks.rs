//! Pi 与 OpenClaw（同构 JSONL）的记录 → block 映射（§4.5）。
//!
//! 调用方负责逐行读取并决定哪些记录参与（Pi 只取活动分支，OpenClaw 全取），
//! 每条记录连同它在文件里的字节区间交给 [`PiTranscript::push_entry`]，大内容据此
//! 生成 [`ContentRef::Jsonl`] 引用。

use std::collections::HashMap;

use serde_json::Value;

use crate::session_manager::model::{
    ContentRef, DiffOp, EventKind, ImageRef, ImageSource, MessageMeta, SessionBlock,
    SessionMessage, ToolKind, ToolStatus,
};

use super::blocks::{
    assign_turn_ids, count_diff_lines, estimate_base64_size, input_preview, parse_arguments,
    single_file_diff, summary_event_block, thinking_block, title_shell, tool_call_block,
    tool_result_block, ToolSource,
};
use super::utils::{extract_text, parse_timestamp_to_ms, JsonlSpan};

/// 逐条累积 Pi / OpenClaw 记录，最后统一分配 turn。
#[derive(Default)]
pub(super) struct PiTranscript {
    messages: Vec<SessionMessage>,
    /// toolCall.id → (消息下标, block 下标)，toolResult 回填 diff 用
    calls: HashMap<String, (usize, usize)>,
}

impl PiTranscript {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一条记录。`entry_id` 为记录自身的 id（Pi v2+ / OpenClaw 有则传）。
    pub fn push_entry(&mut self, value: &Value, span: JsonlSpan, entry_id: Option<String>) {
        let entry_ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        let message = match value.get("type").and_then(Value::as_str) {
            Some("message") => {
                let Some(msg) = value.get("message") else {
                    return;
                };
                let ts = msg
                    .get("timestamp")
                    .and_then(parse_timestamp_to_ms)
                    .or(entry_ts);
                self.message_entry(msg, span, entry_id.as_deref(), ts)
            }
            Some("model_change") => {
                let model = value.get("modelId").and_then(Value::as_str).unwrap_or("");
                let text = match value.get("provider").and_then(Value::as_str) {
                    Some(provider) if !provider.is_empty() && !model.is_empty() => {
                        format!("{provider}/{model}")
                    }
                    _ => model.to_string(),
                };
                event_message(EventKind::ModelChange, text, entry_ts)
            }
            Some("thinking_level_change") => event_message(
                EventKind::ThinkingLevel,
                value
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                entry_ts,
            ),
            Some("compaction" | "branch_summary") => {
                summary_event(value, entry_ts, span, "/summary")
            }
            Some("custom_message")
                if value.get("display").and_then(Value::as_bool) != Some(false) =>
            {
                let text = value.get("content").map(extract_text).unwrap_or_default();
                (!text.trim().is_empty()).then(|| {
                    SessionMessage::from_blocks("system", entry_ts, vec![SessionBlock::text(text)])
                })
            }
            _ => None,
        };
        let Some(mut message) = message.filter(|m| !m.is_empty()) else {
            return;
        };
        message.id = entry_id;
        self.messages.push(message);
    }

    pub fn finish(mut self) -> Vec<SessionMessage> {
        assign_turn_ids(&mut self.messages);
        self.messages
    }

    fn message_entry(
        &mut self,
        msg: &Value,
        span: JsonlSpan,
        entry_id: Option<&str>,
        ts: Option<i64>,
    ) -> Option<SessionMessage> {
        match msg.get("role").and_then(Value::as_str)? {
            "user" => Some(SessionMessage::from_blocks(
                "user",
                ts,
                content_blocks(msg.get("content"), span),
            )),
            "assistant" => Some(self.assistant_message(msg, span, ts)),
            "toolResult" => Some(self.tool_result_message(msg, span, ts)),
            "bashExecution" => Some(bash_execution_message(msg, span, entry_id, ts)),
            "branchSummary" | "compactionSummary" => {
                summary_event(msg, ts, span, "/message/summary")
            }
            // system（system prompt sections）等非对话内容跳过
            _ => None,
        }
    }

    fn assistant_message(
        &mut self,
        msg: &Value,
        span: JsonlSpan,
        ts: Option<i64>,
    ) -> SessionMessage {
        let mut blocks = Vec::new();
        let mut call_ids = Vec::new();
        match msg.get("content") {
            Some(Value::String(text)) if !text.trim().is_empty() => {
                blocks.push(SessionBlock::text(text.clone()));
            }
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let base = format!("/message/content/{i}");
                    match item.get("type").and_then(Value::as_str) {
                        Some("thinking") => {
                            let text = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                            blocks.push(thinking_block(text, None, None, || {
                                Some(span.content_ref(format!("{base}/thinking")))
                            }));
                        }
                        Some("text") => {
                            if let Some(text) = item
                                .get("text")
                                .and_then(Value::as_str)
                                .filter(|t| !t.trim().is_empty())
                            {
                                blocks.push(SessionBlock::text(text.to_string()));
                            }
                        }
                        Some("toolCall") => {
                            let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                            let name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            let input = item
                                .get("arguments")
                                .map(parse_arguments)
                                .unwrap_or(Value::Null);
                            call_ids.push((id.to_string(), blocks.len()));
                            blocks.push(tool_call_block(ToolSource::Pi, id, name, &input, || {
                                Some(span.content_ref(format!("{base}/arguments")))
                            }));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        // 中断 / 截断 / 出错追加事件
        match msg.get("stopReason").and_then(Value::as_str) {
            Some("aborted") => blocks.push(SessionBlock::event(EventKind::Aborted, None, None)),
            Some("length") => blocks.push(SessionBlock::event(
                EventKind::Other,
                Some("truncated".into()),
                None,
            )),
            Some("error") => blocks.push(SessionBlock::Event {
                kind: EventKind::Error,
                text: msg
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                url: None,
                full: None,
            }),
            _ => {}
        }

        let index = self.messages.len();
        for (id, block_index) in call_ids {
            if !id.is_empty() {
                self.calls.insert(id, (index, block_index));
            }
        }
        let mut message = SessionMessage::from_blocks("assistant", ts, blocks);
        message.meta = assistant_meta(msg);
        message
    }

    fn tool_result_message(
        &mut self,
        msg: &Value,
        span: JsonlSpan,
        ts: Option<i64>,
    ) -> SessionMessage {
        let call_id = msg
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_error = msg.get("isError").and_then(Value::as_bool) == Some(true);
        let details = msg.get("details");

        let mut texts: Vec<(usize, &str)> = Vec::new();
        let mut images = Vec::new();
        match msg.get("content") {
            Some(Value::String(text)) => texts.push((usize::MAX, text.as_str())),
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    match item.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = item.get("text").and_then(Value::as_str) {
                                texts.push((i, text));
                            }
                        }
                        Some("image") => {
                            if let Some(image) = inline_image(item, span, i) {
                                images.push(image);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let text = texts.iter().map(|(_, t)| *t).collect::<Vec<_>>().join("\n");
        // 一段文本精确指向它；多段指向整个 content 数组（取回时 `{text}` 项按行拼接，与预览一致）
        let pointer = match texts.as_slice() {
            [] => None,
            [(i, _)] if *i != usize::MAX => Some(format!("/message/content/{i}/text")),
            _ => Some("/message/content".to_string()),
        };

        let status = if is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Success
        };
        let mut block = tool_result_block(call_id, status, &text, || {
            pointer.map(|p| span.content_ref(p))
        });
        if let SessionBlock::ToolResult {
            exit_code,
            duration_ms,
            images: block_images,
            saved_path,
            ..
        } = &mut block
        {
            // details.exitCode / durationMs 待核实；bash 失败时 Pi 在输出末尾写 "Command exited with code N"
            *exit_code = details
                .and_then(|d| d.get("exitCode"))
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .or_else(|| parse_exit_code(&text));
            *duration_ms = details
                .and_then(|d| d.get("durationMs"))
                .and_then(Value::as_u64);
            *block_images = images;
            *saved_path = details
                .and_then(|d| d.get("fullOutputPath"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }

        // edit 的 details.diff 是带行号的 diff，回填到对应 ToolCall
        if let Some(diff_text) = details.and_then(|d| d.get("diff")).and_then(Value::as_str) {
            self.backfill_diff(
                call_id,
                diff_text,
                span.content_ref("/message/details/diff"),
            );
        }

        SessionMessage::from_blocks("tool", ts, vec![block])
    }

    fn backfill_diff(&mut self, call_id: &str, diff_text: &str, full: ContentRef) {
        let Some(&(mi, bi)) = self.calls.get(call_id) else {
            return;
        };
        let Some(SessionBlock::ToolCall {
            kind, title, diff, ..
        }) = self.messages.get_mut(mi).and_then(|m| m.blocks.get_mut(bi))
        else {
            return;
        };
        if !matches!(kind, ToolKind::Edit | ToolKind::Write) {
            return;
        }
        let (added, removed) = count_diff_lines(diff_text);
        let (path, op) = diff
            .as_ref()
            .and_then(|d| d.files.first())
            .map(|f| (f.path.clone(), f.op))
            .unwrap_or_else(|| (title.clone(), DiffOp::Update));
        let mut summary = single_file_diff(&path, op, added, removed);
        summary.full = Some(full);
        *diff = Some(summary);
    }
}

/// user / custom 内容：字符串或 `[{text}|{image}]`。
fn content_blocks(content: Option<&Value>, span: JsonlSpan) -> Vec<SessionBlock> {
    let mut blocks = Vec::new();
    match content {
        Some(Value::String(text)) if !text.trim().is_empty() => {
            blocks.push(SessionBlock::text(text.clone()));
        }
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = item
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|t| !t.trim().is_empty())
                        {
                            blocks.push(SessionBlock::text(text.to_string()));
                        }
                    }
                    Some("image") => {
                        if let Some(image) = inline_image(item, span, i) {
                            blocks.push(SessionBlock::Image { image });
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    blocks
}

/// `{type: image, data: <base64>, mimeType}` → 内联图片引用。
fn inline_image(item: &Value, span: JsonlSpan, index: usize) -> Option<ImageRef> {
    let data = item.get("data").and_then(Value::as_str)?;
    let media_type = item
        .get("mimeType")
        .or_else(|| item.get("mediaType"))
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    Some(ImageRef {
        source: ImageSource::Inline {
            content: span.content_ref(format!("/message/content/{index}/data")),
        },
        media_type: media_type.to_string(),
        size: estimate_base64_size(data.len()),
        alt: None,
    })
}

/// 用户 `!cmd`：ToolCall（by_user）+ ToolResult，归在 user 消息里。
fn bash_execution_message(
    msg: &Value,
    span: JsonlSpan,
    entry_id: Option<&str>,
    ts: Option<i64>,
) -> SessionMessage {
    let id = entry_id.unwrap_or_default().to_string();
    let command = msg.get("command").and_then(Value::as_str).unwrap_or("");
    let output = msg.get("output").and_then(Value::as_str).unwrap_or("");
    let exit = msg
        .get("exitCode")
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok());
    let input = input_preview(&Value::String(command.to_string()));
    let call = SessionBlock::ToolCall {
        id: id.clone(),
        raw_name: "bashExecution".into(),
        kind: ToolKind::Shell,
        title: title_shell(command),
        detail: None,
        server: None,
        input_preview: input.text,
        input_total_len: input.total_len,
        input_full: input
            .truncated
            .then(|| span.content_ref("/message/command")),
        diff: None,
        by_user: true,
    };
    let status = if msg.get("cancelled").and_then(Value::as_bool) == Some(true) {
        ToolStatus::Interrupted
    } else if exit.is_some_and(|code| code != 0) {
        ToolStatus::Error
    } else {
        ToolStatus::Success
    };
    let mut result = tool_result_block(id, status, output, || {
        Some(span.content_ref("/message/output"))
    });
    if let SessionBlock::ToolResult {
        exit_code,
        saved_path,
        ..
    } = &mut result
    {
        *exit_code = exit;
        *saved_path = msg
            .get("fullOutputPath")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    SessionMessage::from_blocks("user", ts, vec![call, result])
}

fn event_message(kind: EventKind, text: String, ts: Option<i64>) -> Option<SessionMessage> {
    (!text.trim().is_empty()).then(|| {
        SessionMessage::from_blocks(
            "system",
            ts,
            vec![SessionBlock::event(kind, Some(text), None)],
        )
    })
}

/// compaction / branch_summary：只放摘要预览，全文按 `pointer` 引用取。
fn summary_event(
    value: &Value,
    ts: Option<i64>,
    span: JsonlSpan,
    pointer: &str,
) -> Option<SessionMessage> {
    let summary = value.get("summary").and_then(Value::as_str)?;
    (!summary.trim().is_empty()).then(|| {
        SessionMessage::from_blocks(
            "system",
            ts,
            vec![summary_event_block(EventKind::Compaction, summary, || {
                Some(span.content_ref(pointer))
            })],
        )
    })
}

/// `model / usage{input, output, cacheRead, cacheWrite, cost.total} / stopReason` → meta（0 视为缺省）。
fn assistant_meta(msg: &Value) -> Option<MessageMeta> {
    let usage = msg.get("usage");
    let count = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
    };
    let meta = MessageMeta {
        model: msg
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        input_tokens: count("input"),
        output_tokens: count("output"),
        cache_read_tokens: count("cacheRead"),
        cache_write_tokens: count("cacheWrite"),
        cost_usd: usage
            .and_then(|u| u.pointer("/cost/total"))
            .and_then(Value::as_f64)
            .filter(|c| *c > 0.0),
        stop_reason: msg
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

/// 末行 `Command exited with code N` → N
fn parse_exit_code(text: &str) -> Option<i32> {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .strip_prefix("Command exited with code ")?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn span(offset: u64) -> JsonlSpan {
        JsonlSpan { offset, len: 10 }
    }

    fn push(t: &mut PiTranscript, value: Value, offset: u64, id: &str) {
        t.push_entry(&value, span(offset), Some(id.to_string()));
    }

    #[test]
    fn maps_pi_entries_to_blocks() {
        let mut t = PiTranscript::new();
        push(
            &mut t,
            json!({"type":"model_change","provider":"anthropic","modelId":"claude-sonnet-4-5","timestamp":"2026-01-01T00:00:00Z"}),
            0,
            "e1",
        );
        push(
            &mut t,
            json!({"type":"thinking_level_change","thinkingLevel":"high"}),
            10,
            "e2",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"system","content":"","sections":{"preamble":"..."}}}),
            20,
            "e3",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"user","timestamp":1000,"content":[
                {"type":"text","text":"看截图"},
                {"type":"image","data":"AAAAAAAA","mimeType":"image/png"}]}}),
            30,
            "e4",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"assistant","model":"claude-sonnet-4-5","stopReason":"toolUse",
                "usage":{"input":5321,"output":201,"cacheRead":4096,"cacheWrite":0,"cost":{"total":0.0073}},
                "content":[
                    {"type":"thinking","thinking":"","thinkingSignature":"sig"},
                    {"type":"toolCall","id":"c1","name":"read","arguments":{"path":"/a/b.tsx","offset":10,"limit":31}},
                    {"type":"toolCall","id":"c2","name":"edit","arguments":{"path":"/a/b.tsx","edits":[{"oldText":"x","newText":"y"}]}},
                    {"type":"toolCall","id":"c3","name":"bash","arguments":{"command":"pnpm test"}}]}}),
            40,
            "e5",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"toolResult","toolCallId":"c1","toolName":"read",
                "content":[{"type":"text","text":"file body"},{"type":"image","data":"AAAA","mimeType":"image/webp"}],"isError":false}}),
            50,
            "e6",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"toolResult","toolCallId":"c2","toolName":"edit",
                "content":[{"type":"text","text":"Successfully replaced 1 block(s)."}],
                "details":{"diff":"  ...\n 10 a\n-11 x\n+11 y\n+12 z\n"},"isError":false}}),
            60,
            "e7",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"toolResult","toolCallId":"c3","toolName":"bash",
                "content":[{"type":"text","text":"FAIL\n\n\nCommand exited with code 1"}],
                "details":{"fullOutputPath":"/tmp/pi-bash.log"},"isError":true}}),
            70,
            "e8",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"bashExecution","command":"pnpm test","output":"ok","exitCode":0}}),
            80,
            "e9",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"assistant","stopReason":"aborted","content":[{"type":"text","text":"我先搜一下"}]}}),
            90,
            "e10",
        );
        push(
            &mut t,
            json!({"type":"compaction","summary":"Fixed contrast.","timestamp":"2026-01-01T00:01:00Z"}),
            100,
            "e11",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"assistant","stopReason":"length","content":"cut"}}),
            110,
            "e12",
        );
        let messages = t.finish();

        let roles: Vec<(&str, &str)> = messages
            .iter()
            .map(|m| (m.role.as_str(), m.turn_id.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            roles,
            vec![
                ("system", "t0"),
                ("system", "t0"),
                ("user", "t1"),
                ("assistant", "t1"),
                ("tool", "t1"),
                ("tool", "t1"),
                ("tool", "t1"),
                // bashExecution 是用户自己跑的命令，不开新轮
                ("user", "t1"),
                ("assistant", "t1"),
                ("system", "t1"),
                ("assistant", "t1"),
            ]
        );
        assert_eq!(messages[0].content, "anthropic/claude-sonnet-4-5");
        assert_eq!(messages[0].id.as_deref(), Some("e1"));
        assert!(matches!(
            &messages[1].blocks[0],
            SessionBlock::Event { kind: EventKind::ThinkingLevel, text: Some(t), .. } if t == "high"
        ));

        // user：文本 + 内联图片
        match &messages[2].blocks[1] {
            SessionBlock::Image { image } => {
                assert_eq!(image.size, 6);
                assert_eq!(
                    image.source,
                    ImageSource::Inline {
                        content: ContentRef::Jsonl {
                            offset: 30,
                            len: 10,
                            pointer: "/message/content/1/data".into()
                        }
                    }
                );
            }
            other => panic!("{other:?}"),
        }

        // assistant：redacted 思考、read 行范围、edit diff 被 toolResult 回填
        let a = &messages[3];
        assert!(matches!(
            &a.blocks[0],
            SessionBlock::Thinking { redacted: true, .. }
        ));
        match &a.blocks[1] {
            SessionBlock::ToolCall {
                kind,
                title,
                detail,
                ..
            } => {
                assert_eq!(*kind, ToolKind::Read);
                assert_eq!(title, "/a/b.tsx");
                assert_eq!(detail.as_deref(), Some(":10-40"));
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[2] {
            SessionBlock::ToolCall { diff, .. } => {
                let diff = diff.as_ref().expect("diff");
                assert_eq!((diff.added, diff.removed), (2, 1));
                assert_eq!(
                    diff.full,
                    Some(ContentRef::Jsonl {
                        offset: 60,
                        len: 10,
                        pointer: "/message/details/diff".into()
                    })
                );
            }
            other => panic!("{other:?}"),
        }
        let meta = a.meta.as_ref().expect("meta");
        assert_eq!(meta.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(meta.cache_read_tokens, Some(4096));
        assert_eq!(meta.cache_write_tokens, None);
        assert_eq!(meta.cost_usd, Some(0.0073));
        assert_eq!(meta.stop_reason.as_deref(), Some("toolUse"));
        assert_eq!(
            a.content,
            "[Tool: read] /a/b.tsx\n\n[Tool: edit] /a/b.tsx\n\n[Tool: bash] pnpm test"
        );

        // toolResult：图片、错误、退出码、落盘路径
        match &messages[4].blocks[0] {
            SessionBlock::ToolResult {
                call_id,
                status,
                images,
                ..
            } => {
                assert_eq!(call_id, "c1");
                assert_eq!(*status, ToolStatus::Success);
                assert_eq!(images.len(), 1);
                assert_eq!(images[0].media_type, "image/webp");
            }
            other => panic!("{other:?}"),
        }
        match &messages[6].blocks[0] {
            SessionBlock::ToolResult {
                status,
                exit_code,
                saved_path,
                ..
            } => {
                assert_eq!(*status, ToolStatus::Error);
                assert_eq!(*exit_code, Some(1));
                assert_eq!(saved_path.as_deref(), Some("/tmp/pi-bash.log"));
            }
            other => panic!("{other:?}"),
        }

        // bashExecution：用户自己跑的命令，开启新一轮
        match &messages[7].blocks[..] {
            [SessionBlock::ToolCall {
                id, by_user, kind, ..
            }, SessionBlock::ToolResult {
                call_id,
                exit_code,
                status,
                ..
            }] => {
                assert_eq!(id, "e9");
                assert_eq!(call_id, "e9");
                assert!(*by_user);
                assert_eq!(*kind, ToolKind::Shell);
                assert_eq!(*exit_code, Some(0));
                assert_eq!(*status, ToolStatus::Success);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(messages[7].content, "[Tool: bashExecution] pnpm test\n\nok");

        assert!(matches!(
            messages[8].blocks.last(),
            Some(SessionBlock::Event {
                kind: EventKind::Aborted,
                ..
            })
        ));
        assert!(matches!(
            &messages[9].blocks[0],
            SessionBlock::Event { kind: EventKind::Compaction, text: Some(t), .. } if t == "Fixed contrast."
        ));
        assert_eq!(messages[10].content, "cut\n\ntruncated");
    }

    #[test]
    fn long_tool_output_and_thinking_get_jsonl_refs() {
        let long: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let thinking = "想".repeat(500);
        let mut t = PiTranscript::new();
        push(
            &mut t,
            json!({"type":"message","message":{"role":"assistant","content":[{"type":"thinking","thinking":thinking}]}}),
            0,
            "a",
        );
        push(
            &mut t,
            json!({"type":"message","message":{"role":"toolResult","toolCallId":"c","content":[{"type":"text","text":long}]}}),
            100,
            "b",
        );
        let messages = t.finish();
        match &messages[0].blocks[0] {
            SessionBlock::Thinking { text, full, .. } => {
                assert_eq!(text.chars().count(), 400);
                assert_eq!(
                    full,
                    &Some(ContentRef::Jsonl {
                        offset: 0,
                        len: 10,
                        pointer: "/message/content/0/thinking".into()
                    })
                );
            }
            other => panic!("{other:?}"),
        }
        match &messages[1].blocks[0] {
            SessionBlock::ToolResult {
                truncated, full, ..
            } => {
                assert!(*truncated);
                assert_eq!(
                    full,
                    &Some(ContentRef::Jsonl {
                        offset: 100,
                        len: 10,
                        pointer: "/message/content/0/text".into()
                    })
                );
            }
            other => panic!("{other:?}"),
        }
    }
}
