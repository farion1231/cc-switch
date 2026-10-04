use std::path::Path;

use serde_json::Value;

use crate::session_manager::model::{
    ContentRef, DiffOp, EventKind, MessageMeta, SessionBlock, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, count_diff_lines, single_file_diff, thinking_block, tool_call_block,
    tool_result_block, ToolSource,
};
use super::utils::{parse_timestamp_to_ms, truncate_summary};

const PROVIDER_ID: &str = "gemini";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Iterate over project directories: tmp/<project_name>/chats/session-*.json
    let project_dirs = match std::fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in project_dirs.flatten() {
        let chats_dir = entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let chat_files = match std::fs::read_dir(&chats_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let project_root_file = entry.path().join(".project_root");
        let project_dir = std::fs::read_to_string(project_root_file).ok();

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(meta) = parse_session(&path) {
                sessions.push(SessionMeta {
                    project_dir: project_dir.clone(),
                    ..meta
                });
            }
        }
    }

    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    let value: Value =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse session JSON: {e}"))?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;

    // 全文引用指向会话文件本身：rel_path 相对 provider 根（`~/.gemini/tmp`），即 `<hash>/chats/<file>`
    let rel_path = relative_to_tmp_root(path);
    let file_ref = |pointer: String| -> Option<ContentRef> {
        Some(ContentRef::File {
            rel_path: rel_path.clone(),
            pointer,
        })
    };

    let mut result = Vec::new();
    for (index, msg) in messages.iter().enumerate() {
        let ts = msg.get("timestamp").and_then(parse_timestamp_to_ms);
        let base = format!("/messages/{index}");
        let mut message = match msg.get("type").and_then(Value::as_str) {
            Some("user") => {
                let text = content_text(msg.get("content"));
                if text.trim().is_empty() {
                    continue;
                }
                SessionMessage::from_blocks("user", ts, vec![SessionBlock::text(text)])
            }
            Some("gemini") => {
                let blocks = gemini_blocks(msg, &base, &file_ref);
                let mut message = SessionMessage::from_blocks("assistant", ts, blocks);
                message.meta = gemini_meta(msg);
                message
            }
            Some(kind @ ("info" | "error")) => {
                let text = content_text(msg.get("content"));
                if text.trim().is_empty() {
                    continue;
                }
                let event_kind = if kind == "info" {
                    EventKind::Info
                } else {
                    EventKind::Error
                };
                let mut message = SessionMessage::from_blocks(
                    "system",
                    ts,
                    vec![SessionBlock::event(event_kind, Some(text), None)],
                );
                // info（登录、刷新等提示）默认折叠
                message.injected = kind == "info";
                message
            }
            Some(_) | None => continue,
        };
        if message.is_empty() {
            continue;
        }
        message.id = msg.get("id").and_then(Value::as_str).map(str::to_string);
        result.push(message);
    }

    assign_turn_ids(&mut result);
    Ok(result)
}

/// 多条 `thoughts[{subject, description}]` 合并后的思考：`summary` 为各 subject 以 ` · ` 连接，
/// `text` 为 `**subject**\n\ndescription` 以空行连接。
pub(crate) struct MergedThoughts {
    pub summary: String,
    pub text: String,
}

/// 合并 Gemini 的 `thoughts` 数组；不是对象数组、或没有任何非空 subject/description 时返回 `None`。
/// 解析器生成预览与按引用取全文共用这一份格式。
pub(crate) fn format_thoughts(value: &Value) -> Option<MergedThoughts> {
    let items = value.as_array()?;
    let mut thoughts = Vec::new();
    for item in items {
        let object = item.as_object()?;
        let field = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or("").trim();
        let (subject, description) = (field("subject"), field("description"));
        if !subject.is_empty() || !description.is_empty() {
            thoughts.push((subject, description));
        }
    }
    if thoughts.is_empty() {
        return None;
    }
    let summary = thoughts
        .iter()
        .map(|(subject, _)| *subject)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let text = thoughts
        .iter()
        .map(
            |(subject, description)| match (subject.is_empty(), description.is_empty()) {
                (false, false) => format!("**{subject}**\n\n{description}"),
                (false, true) => format!("**{subject}**"),
                _ => description.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(MergedThoughts { summary, text })
}

/// 会话文件相对 `tmp/` 的路径：取末尾三段 `<hash>/chats/<file>`（不足三段时取已有部分）。
fn relative_to_tmp_root(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(3)
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// Gemini content 可能是字符串，也可能是 `[{text}]` 数组。
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `type=gemini` 消息：思考（多条合并为一块）→ 正文 → 工具调用与结果（按生成顺序）。
fn gemini_blocks(
    msg: &Value,
    base: &str,
    file_ref: &dyn Fn(String) -> Option<ContentRef>,
) -> Vec<SessionBlock> {
    let mut blocks = Vec::new();

    if let Some(thoughts) = msg.get("thoughts").and_then(format_thoughts) {
        // 合并后的正文与取回的全文同一口径：引用指向整个数组，由 `content::resolve_content_ref`
        // 按 `format_thoughts` 格式化
        blocks.push(thinking_block(
            &thoughts.text,
            (!thoughts.summary.is_empty()).then_some(thoughts.summary),
            None,
            || file_ref(format!("{base}/thoughts")),
        ));
    }

    let text = content_text(msg.get("content"));
    if !text.trim().is_empty() {
        blocks.push(SessionBlock::text(text));
    }

    for (j, call) in msg
        .get("toolCalls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(name) = call.get("name").and_then(Value::as_str) else {
            continue;
        };
        let call_base = format!("{base}/toolCalls/{j}");
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{name}-{j}"));
        let args = call.get("args").cloned().unwrap_or(Value::Null);
        let mut call_block = tool_call_block(ToolSource::Gemini, id.clone(), name, &args, || {
            file_ref(format!("{call_base}/args"))
        });

        let (output, pointer) = tool_output(call, &call_base);
        // resultDisplay 是 FileDiff 时用真实 diff 覆盖参数估算
        if let Some(diff_text) = call
            .pointer("/resultDisplay/fileDiff")
            .and_then(Value::as_str)
        {
            if let SessionBlock::ToolCall { diff, title, .. } = &mut call_block {
                let (added, removed) = count_diff_lines(diff_text);
                let path = call
                    .pointer("/resultDisplay/filePath")
                    .or_else(|| call.pointer("/resultDisplay/fileName"))
                    .and_then(Value::as_str)
                    .unwrap_or(title.as_str())
                    .to_string();
                let op = diff
                    .as_ref()
                    .and_then(|d| d.files.first())
                    .map_or(DiffOp::Update, |f| f.op);
                let mut summary = single_file_diff(&path, op, added, removed);
                summary.full = file_ref(format!("{call_base}/resultDisplay/fileDiff"));
                *diff = Some(summary);
            }
        }
        blocks.push(call_block);

        let status = match call.get("status").and_then(Value::as_str) {
            Some("success" | "completed") => ToolStatus::Success,
            Some("error") => ToolStatus::Error,
            Some("cancelled" | "canceled") => ToolStatus::Interrupted,
            Some("executing" | "scheduled" | "validating" | "awaiting_approval") => {
                ToolStatus::Pending
            }
            _ => ToolStatus::Unknown,
        };
        blocks.push(tool_result_block(id, status, &output, || {
            pointer.and_then(file_ref)
        }));
    }

    blocks
}

/// 工具输出文本与其 JSON Pointer（能精确指到字符串时才给）。
/// 优先 `resultDisplay`（字符串或 FileDiff），否则 `result[].functionResponse.response.output|error`。
fn tool_output(call: &Value, call_base: &str) -> (String, Option<String>) {
    match call.get("resultDisplay") {
        Some(Value::String(text)) if !text.is_empty() => {
            return (text.clone(), Some(format!("{call_base}/resultDisplay")));
        }
        Some(display @ Value::Object(_)) => {
            if let Some(diff) = display.get("fileDiff").and_then(Value::as_str) {
                return (
                    diff.to_string(),
                    Some(format!("{call_base}/resultDisplay/fileDiff")),
                );
            }
            return (display.to_string(), None);
        }
        _ => {}
    }
    for (k, item) in call
        .get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        for key in ["output", "error"] {
            if let Some(text) = item
                .pointer(&format!("/functionResponse/response/{key}"))
                .and_then(Value::as_str)
            {
                return (
                    text.to_string(),
                    Some(format!(
                        "{call_base}/result/{k}/functionResponse/response/{key}"
                    )),
                );
            }
        }
    }
    match call.get("result") {
        Some(Value::String(text)) => (text.clone(), Some(format!("{call_base}/result"))),
        Some(Value::Null) | None => (String::new(), None),
        Some(other) => (other.to_string(), None),
    }
}

/// `tokens{input, output, cached, thoughts, tool, total}` + `model` → meta（0 视为缺省）。
fn gemini_meta(msg: &Value) -> Option<MessageMeta> {
    let tokens = msg.get("tokens");
    let count = |key: &str| {
        tokens
            .and_then(|t| t.get(key))
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
        cache_read_tokens: count("cached"),
        reasoning_tokens: count("thoughts"),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Gemini session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Gemini session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Gemini session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("sessionId").and_then(Value::as_str)?.to_string();

    let created_at = value.get("startTime").and_then(parse_timestamp_to_ms);
    let last_active_at = value.get("lastUpdated").and_then(parse_timestamp_to_ms);

    // Derive title from first user message
    let title = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|msgs| {
            msgs.iter()
                .find(|m| m.get("type").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("content").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate_summary(s, 160))
        });

    let source_path = path.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None, // (optionally) populated later
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(source_path),
        resume_command: Some(format!("gemini --resume {session_id}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::ToolKind;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_json_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-03-06T10-17-test.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "gemini-session-123",
              "startTime": "2026-03-06T10:17:58.000Z",
              "lastUpdated": "2026-03-06T10:20:00.000Z",
              "messages": [
                {
                  "id": "msg-1",
                  "timestamp": "2026-03-06T10:17:58.000Z",
                  "type": "user",
                  "content": "hello"
                }
              ]
            }"#,
        )
        .expect("write session");

        delete_session(temp.path(), &path, "gemini-session-123").expect("delete session");

        assert!(!path.exists());
    }

    #[test]
    fn load_messages_handles_array_content() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-06T10:00:00Z","type":"user","content":[{"text":"hello"}]},
                {"id":"2","timestamp":"2026-03-06T10:00:01Z","type":"gemini","content":"world"},
                {"id":"3","timestamp":"2026-03-06T10:00:02Z","type":"info","content":"system info"},
                {"id":"4","timestamp":"2026-03-06T10:00:03Z","type":"error","content":"MCP ERROR"}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        // info / error 现在作为 system 事件保留（info 标记为注入内容）
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "world");
        assert_eq!((msgs[2].role.as_str(), msgs[2].injected), ("system", true));
        assert!(matches!(
            &msgs[2].blocks[0],
            SessionBlock::Event { kind: EventKind::Info, text: Some(t), .. } if t == "system info"
        ));
        assert!(!msgs[3].injected);
        assert!(matches!(
            &msgs[3].blocks[0],
            SessionBlock::Event { kind: EventKind::Error, text: Some(t), .. } if t == "MCP ERROR"
        ));
    }

    #[test]
    fn load_messages_includes_tool_calls() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"","toolCalls":[{"id":"call_1","name":"web_search","args":{"query":"test"}}]},
                {"id":"2","timestamp":"2026-03-10T08:25:00Z","type":"gemini","content":"Here are the results.","toolCalls":[{"id":"call_2","name":"web_fetch","args":{"url":"http://example.com"}}]}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: web_search]"));
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("Here are the results."));
        assert!(msgs[1].content.contains("[Tool: web_fetch]"));
    }

    #[test]
    fn load_messages_maps_thoughts_tool_calls_and_meta() {
        let temp = tempdir().expect("tempdir");
        let chats = temp.path().join("tmp").join("hash1").join("chats");
        std::fs::create_dir_all(&chats).expect("chats dir");
        let path = chats.join("session-x.json");
        let long_output: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let session = serde_json::json!({
            "sessionId": "test",
            "messages": [
                {"id":"i0","timestamp":"2026-03-10T08:24:40Z","type":"info","content":"Authenticated"},
                {"id":"u1","timestamp":"2026-03-10T08:24:45Z","type":"user","content":"build it"},
                {"id":"g1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"Done.",
                 "model":"gemini-2.5-pro",
                 "tokens":{"input":10342,"output":418,"cached":0,"thoughts":236,"tool":0,"total":10996},
                 "thoughts":[
                    {"subject":"Locating the entry point","description":"Read src/main.ts first."},
                    {"subject":"Planning the build check","description":"Then run npm run build."}],
                 "toolCalls":[
                    {"id":"run-1","name":"run_shell_command","args":{"command":"npm run build","description":"Build the project"},
                     "status":"error","resultDisplay":long_output},
                    {"id":"rep-1","name":"replace","status":"success",
                     "args":{"file_path":"/p/config.ts","old_string":"a","new_string":"b"},
                     "resultDisplay":{"fileName":"config.ts","fileDiff":"--- a\n+++ b\n-a\n+b\n+c\n"}},
                    {"id":"cancel-1","name":"run_shell_command","args":{"command":"sleep 9"},"status":"cancelled",
                     "result":[{"functionResponse":{"id":"cancel-1","name":"run_shell_command",
                       "response":{"error":"Command was cancelled by the user."}}}]}
                 ]}
            ]
        });
        std::fs::write(&path, session.to_string()).expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 3);
        let turns: Vec<_> = msgs.iter().map(|m| m.turn_id.as_deref().unwrap()).collect();
        assert_eq!(turns, ["t0", "t1", "t1"]);
        assert_eq!(msgs[1].id.as_deref(), Some("u1"));

        let a = &msgs[2];
        match &a.blocks[0] {
            SessionBlock::Thinking {
                text,
                summary,
                full,
                ..
            } => {
                assert_eq!(
                    summary.as_deref(),
                    Some("Locating the entry point · Planning the build check")
                );
                assert!(text.starts_with("**Locating the entry point**\n\nRead src/main.ts first."));
                assert!(full.is_none());
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&a.blocks[1], SessionBlock::Text { text, .. } if text == "Done."));
        match &a.blocks[2] {
            SessionBlock::ToolCall {
                kind,
                title,
                detail,
                ..
            } => {
                assert_eq!(*kind, ToolKind::Shell);
                assert_eq!(title, "npm run build");
                assert_eq!(detail.as_deref(), Some("Build the project"));
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[3] {
            SessionBlock::ToolResult {
                status,
                truncated,
                full,
                ..
            } => {
                assert_eq!(*status, ToolStatus::Error);
                assert!(*truncated);
                assert_eq!(
                    full,
                    &Some(ContentRef::File {
                        rel_path: "hash1/chats/session-x.json".into(),
                        pointer: "/messages/2/toolCalls/0/resultDisplay".into(),
                    })
                );
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[4] {
            SessionBlock::ToolCall { kind, diff, .. } => {
                assert_eq!(*kind, ToolKind::Edit);
                let diff = diff.as_ref().expect("diff");
                assert_eq!((diff.added, diff.removed), (2, 1));
                assert_eq!(diff.files[0].path, "config.ts");
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[7] {
            SessionBlock::ToolResult {
                status, preview, ..
            } => {
                assert_eq!(*status, ToolStatus::Interrupted);
                assert_eq!(preview, "Command was cancelled by the user.");
            }
            other => panic!("{other:?}"),
        }
        let meta = a.meta.as_ref().expect("meta");
        assert_eq!(meta.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(meta.input_tokens, Some(10342));
        assert_eq!(meta.cache_read_tokens, None);
        assert_eq!(meta.reasoning_tokens, Some(236));
        // 思考不进入 content
        assert!(a
            .content
            .starts_with("Done.\n\n[Tool: run_shell_command] npm run build"));
    }
}
