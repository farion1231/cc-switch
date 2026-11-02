use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::Value;

use crate::session_manager::model::{SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{assign_turn_ids, openai_tool_calls, tool_result_block};
use super::utils::{
    collect_files, extract_text, for_each_jsonl_value, parse_timestamp_to_ms, truncate_summary,
    FileParseCache, TITLE_MAX_CHARS,
};

#[derive(Debug, Deserialize)]
struct GrokSessionInfo {
    id: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrokSessionSummary {
    info: GrokSessionInfo,
    #[serde(default)]
    session_summary: Option<String>,
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    created_at: Option<Value>,
    #[serde(default)]
    updated_at: Option<Value>,
    #[serde(default)]
    last_active_at: Option<Value>,
}

pub fn session_roots() -> Vec<PathBuf> {
    let config_dir = crate::grok_config::get_grok_config_dir();
    vec![
        config_dir.join("sessions"),
        config_dir.join("archived_sessions"),
    ]
}

/// 会话页每次打开都会全量扫描；没变过的 summary.json 直接复用上次的解析结果。
static PARSE_CACHE: LazyLock<FileParseCache> =
    LazyLock::new(|| FileParseCache::persistent("grokbuild"));

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut summaries = Vec::new();
    for root in session_roots() {
        collect_summary_files(&root, &mut summaries);
    }
    PARSE_CACHE.scan(summaries, scan_summary_file)
}

/// `chat_history.jsonl`：`type ∈ {system, user, assistant, tool}`。
///
/// 会话的 sourcePath 是同目录的 `summary.json`；大内容的 Jsonl 引用指向 chat_history.jsonl
/// 的行（`content::resolve_content_ref` 对 grokbuild 固定改读该文件）。`tool` 记录的配对字段与
/// assistant 的 `tool_calls` 都是按 OpenAI 形状推断（待核实），缺失时 `callId` 为空串。
pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let session_dir = path
        .parent()
        .ok_or_else(|| format!("Invalid Grok Build session path: {}", path.display()))?;
    let chat_path = session_dir.join("chat_history.jsonl");
    if !chat_path.is_file() {
        return Err(format!(
            "Failed to open Grok Build chat history: {}",
            chat_path.display()
        ));
    }
    let mut messages = Vec::new();

    for_each_jsonl_value(&chat_path, |span, value| {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let role = match kind {
            "system" | "user" | "assistant" | "tool" => kind,
            // Reasoning records can contain encrypted/internal state and are not
            // conversation messages shown by Grok's own history view.
            _ => return Ok(()),
        };
        let text = value.get("content").map(extract_text).unwrap_or_default();
        let blocks = match role {
            "tool" => vec![tool_result_block(
                value
                    .get("tool_call_id")
                    .or_else(|| value.get("call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                ToolStatus::Unknown,
                &text,
                || Some(span.content_ref("/content")),
            )],
            "assistant" => {
                let mut blocks = Vec::new();
                if !text.trim().is_empty() {
                    blocks.push(SessionBlock::text(text));
                }
                blocks.extend(openai_tool_calls(value.get("tool_calls"), |pointer| {
                    Some(span.content_ref(format!("/tool_calls{pointer}")))
                }));
                blocks
            }
            _ if text.trim().is_empty() => Vec::new(),
            _ => vec![SessionBlock::text(text)],
        };
        let ts = value
            .get("timestamp")
            .or_else(|| value.get("ts"))
            .and_then(parse_timestamp_to_ms);
        let mut message = SessionMessage::from_blocks(role, ts, blocks);
        if message.is_empty() {
            return Ok(());
        }
        // system prompt 属于注入内容，默认隐藏
        message.injected = role == "system";
        messages.push(message);
        Ok(())
    })?;

    assign_turn_ids(&mut messages);
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if !path.starts_with(root) {
        return Err(format!(
            "Grok Build session source is outside the session root: {}",
            path.display()
        ));
    }
    if path.file_name().and_then(|name| name.to_str()) != Some("summary.json") {
        return Err(format!(
            "Unexpected Grok Build session source: {}",
            path.display()
        ));
    }
    let summary = read_summary(path)?;
    if summary.info.id != session_id {
        return Err(format!(
            "Grok Build session ID mismatch: expected {session_id}, found {}",
            summary.info.id
        ));
    }
    let session_dir = path
        .parent()
        .ok_or_else(|| format!("Invalid Grok Build session path: {}", path.display()))?;
    if session_dir == root || !session_dir.starts_with(root) {
        return Err(format!(
            "Refusing to delete Grok Build session directory outside its root: {}",
            session_dir.display()
        ));
    }
    if session_dir.file_name().and_then(|name| name.to_str()) != Some(session_id) {
        return Err(format!(
            "Grok Build session directory does not match session ID: {}",
            session_dir.display()
        ));
    }
    std::fs::remove_dir_all(session_dir).map_err(|e| {
        format!(
            "Failed to delete Grok Build session directory {}: {e}",
            session_dir.display()
        )
    })?;
    Ok(true)
}

/// 会话目录里还有 chat_history.jsonl 等大量文件；类型直接取自目录枚举，不为每个条目再 stat
fn collect_summary_files(root: &Path, files: &mut Vec<PathBuf>) {
    collect_files(root, files, &|path| {
        path.file_name().and_then(|name| name.to_str()) == Some("summary.json")
    });
}

/// 列表扫描用：读不了（权限、被占用等）返回 `Err`，不进解析缓存、下轮重试；
/// 读到了但不是有效的会话摘要返回 `Ok(None)`，可以缓存（文件改动后会重新解析）。
fn scan_summary_file(path: &Path) -> std::io::Result<Option<SessionMeta>> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str::<GrokSessionSummary>(&text)
        .ok()
        .map(|summary| summary_meta(path, summary)))
}

fn read_summary(path: &Path) -> Result<GrokSessionSummary, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read Grok Build session summary: {e}"))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse Grok Build session summary: {e}"))
}

fn summary_meta(path: &Path, summary: GrokSessionSummary) -> SessionMeta {
    let session_id = summary.info.id;
    let title = summary
        .generated_title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            summary
                .session_summary
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })
        .map(|value| truncate_summary(value, TITLE_MAX_CHARS));
    let session_summary = summary
        .session_summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| truncate_summary(value, 160));
    let created_at = summary.created_at.as_ref().and_then(parse_timestamp_to_ms);
    let last_active_at = summary
        .last_active_at
        .as_ref()
        .or(summary.updated_at.as_ref())
        .and_then(parse_timestamp_to_ms);

    SessionMeta {
        provider_id: "grokbuild".to_string(),
        session_id: session_id.clone(),
        title,
        summary: session_summary,
        project_dir: summary.info.cwd,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("grok --resume {session_id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scans_native_grokbuild_session_layout() {
        let temp = tempdir().expect("tempdir");
        let sessions_dir = temp.path().join("sessions");
        let session_id = "019f6af2-18b0-7673-958e-d25be650e172";
        let session_dir = sessions_dir.join("encoded-project").join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        std::fs::write(
            session_dir.join("summary.json"),
            format!(
                r#"{{"info":{{"id":"{session_id}","cwd":"C:/work"}},"session_summary":"hello grok","generated_title":"Grok session","created_at":"2026-07-16T12:00:00Z","last_active_at":"2026-07-16T12:00:01Z"}}"#
            ),
        )
        .expect("write summary");
        let mut files = Vec::new();
        collect_summary_files(&sessions_dir, &mut files);
        // 混进一个不是摘要的文件，确认只收集 summary.json
        std::fs::write(session_dir.join("chat_history.jsonl"), "").expect("write history");
        let sessions = FileParseCache::new().scan(files, scan_summary_file);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_id, "grokbuild");
        assert_eq!(sessions[0].session_id, session_id);
        assert_eq!(sessions[0].title.as_deref(), Some("Grok session"));
        let expected_resume = format!("grok --resume {session_id}");
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some(expected_resume.as_str())
        );
    }

    #[test]
    fn scan_reuses_unchanged_summaries_and_rereads_changed_ones() {
        let temp = tempdir().expect("tempdir");
        let session_dir = temp.path().join("sessions").join("project").join("s1");
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let summary = session_dir.join("summary.json");
        let write = |title: &str| {
            std::fs::write(
                &summary,
                format!(r#"{{"info":{{"id":"s1"}},"generated_title":"{title}"}}"#),
            )
            .expect("write summary");
        };
        let scan = |cache: &FileParseCache| {
            let mut files = Vec::new();
            collect_summary_files(&temp.path().join("sessions"), &mut files);
            cache
                .scan(files, scan_summary_file)
                .into_iter()
                .map(|meta| meta.title.unwrap_or_default())
                .collect::<Vec<_>>()
        };
        let cache = FileParseCache::new();

        write("first");
        assert_eq!(scan(&cache), ["first"]);
        // 长度变了，按修改时间 + 大小判断为已改动，重新解析
        write("second title");
        assert_eq!(scan(&cache), ["second title"]);
        // 写坏的摘要不是会话；修好后重新出现
        std::fs::write(&summary, "{").expect("write broken summary");
        assert!(scan(&cache).is_empty());
        write("repaired");
        assert_eq!(scan(&cache), ["repaired"]);
    }

    #[test]
    fn loads_native_grokbuild_chat_history() {
        let temp = tempdir().expect("tempdir");
        let summary_path = temp.path().join("summary.json");
        std::fs::write(&summary_path, "{}").expect("write summary placeholder");
        std::fs::write(
            temp.path().join("chat_history.jsonl"),
            concat!(
                "{\"type\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n",
                "{\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"private\"}]}\n",
                "{\"type\":\"assistant\",\"content\":\"Hi there\"}\n"
            ),
        )
        .expect("write chat history");

        let messages = load_messages(&summary_path).expect("load messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].content, "Hi there");
    }

    #[test]
    fn delete_session_removes_only_the_matching_session_directory() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let session_id = "session-to-delete";
        let session_dir = root.join("project").join(session_id);
        let sibling_dir = root.join("project").join("session-to-keep");
        std::fs::create_dir_all(&session_dir).expect("create session directory");
        std::fs::create_dir_all(&sibling_dir).expect("create sibling directory");
        let summary_path = session_dir.join("summary.json");
        std::fs::write(
            &summary_path,
            format!(r#"{{"info":{{"id":"{session_id}"}}}}"#),
        )
        .expect("write summary");
        std::fs::write(sibling_dir.join("keep.txt"), "keep").expect("write sibling file");

        let deleted = delete_session(&root, &summary_path, session_id).expect("delete session");

        assert!(deleted);
        assert!(!session_dir.exists());
        assert!(sibling_dir.exists());
    }

    #[test]
    fn delete_session_rejects_remove_dir_all_target_outside_root() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let outside_dir = temp.path().join("outside").join("session-outside");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside_dir).expect("create outside directory");
        let summary_path = outside_dir.join("summary.json");
        std::fs::write(&summary_path, r#"{"info":{"id":"session-outside"}}"#)
            .expect("write summary");

        let error = delete_session(&root, &summary_path, "session-outside")
            .expect_err("outside path must be rejected");

        assert!(error.contains("outside the session root"));
        assert!(outside_dir.exists());
    }

    #[test]
    fn grokbuild_tool_records_become_results() {
        let temp = tempdir().expect("tempdir");
        let summary_path = temp.path().join("summary.json");
        std::fs::write(&summary_path, "{}").expect("write summary placeholder");
        std::fs::write(
            temp.path().join("chat_history.jsonl"),
            concat!(
                "{\"type\":\"system\",\"content\":\"You are Grok.\"}\n",
                "{\"type\":\"user\",\"content\":\"clean journal\"}\n",
                "{\"type\":\"assistant\",\"content\":\"\",\"tool_calls\":[{\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"run_terminal_cmd\",\"arguments\":\"{\\\"command\\\":\\\"journalctl --vacuum-time=3d\\\"}\"}}]}\n",
                "{\"type\":\"tool\",\"tool_call_id\":\"call_1\",\"content\":\"Vacuuming done\"}\n",
                "{\"type\":\"tool\",\"content\":\"unpaired output\"}\n",
                "{\"type\":\"assistant\",\"content\":\"freed 2.1G\"}\n"
            ),
        )
        .expect("write chat history");

        let messages = load_messages(&summary_path).expect("load messages");
        assert_eq!(messages.len(), 6);
        assert!(messages[0].injected);
        assert_eq!(messages[0].turn_id.as_deref(), Some("t0"));
        assert_eq!(
            messages[2].content,
            "[Tool: run_terminal_cmd] journalctl --vacuum-time=3d"
        );
        let result = |i: usize| match &messages[i].blocks[0] {
            SessionBlock::ToolResult {
                call_id, status, ..
            } => (call_id.clone(), *status),
            other => panic!("{other:?}"),
        };
        assert_eq!(result(3), ("call_1".into(), ToolStatus::Unknown));
        assert_eq!(result(4), (String::new(), ToolStatus::Unknown));
        assert_eq!(messages[4].role, "tool");
        assert_eq!(messages[5].turn_id.as_deref(), Some("t1"));
    }
}
