use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "claude";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_claude_config_dir().join("projects");
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);

    let mut sessions = Vec::new();
    for path in files {
        if let Some(meta) = parse_session(&path) {
            sessions.push(meta);
        }
    }

    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(value) => value,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        let message = match value.get("message") {
            Some(message) => message,
            None => continue,
        };

        let mut role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        // Claude wraps tool_result inside user messages; reclassify as "tool" role
        if role == "user" {
            if let Some(Value::Array(items)) = message.get("content") {
                let all_tool_results = !items.is_empty()
                    && items.iter().all(|item| {
                        item.get("type").and_then(Value::as_str) == Some("tool_result")
                    });
                if all_tool_results {
                    role = "tool".to_string();
                }
            }
        }

        let content = message.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);

        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Claude session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Claude session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    if let Some(stem) = path.file_stem() {
        let sibling = path.parent().unwrap_or_else(|| Path::new("")).join(stem);
        remove_path_if_exists(&sibling).map_err(|e| {
            format!(
                "Failed to delete Claude session sidecar {}: {e}",
                sibling.display()
            )
        })?;
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Claude session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    if is_agent_session(path) {
        return None;
    }

    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;

    // Extract metadata and first user message from head lines
    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if project_dir.is_none() {
            project_dir = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Extract first real user message as title candidate
        // Skip system-injected caveats and slash commands (e.g. /clear, /compact)
        if first_user_message.is_none() {
            let is_user = value.get("type").and_then(Value::as_str) == Some("user")
                || value
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str)
                    == Some("user");
            if is_user {
                if let Some(message) = value.get("message") {
                    let text = message.get("content").map(extract_text).unwrap_or_default();
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.contains("<local-command-caveat>")
                        && !trimmed.starts_with("<command-name>")
                    {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
        }
        if session_id.is_some()
            && project_dir.is_some()
            && created_at.is_some()
            && first_user_message.is_some()
        {
            break;
        }
    }

    // Extract last_active_at, summary, and custom-title from tail lines (reverse order)
    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;
    let mut custom_title: Option<String> = None;

    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Look for custom-title entry (take the last one, i.e. first in reverse)
        if custom_title.is_none()
            && value.get("type").and_then(Value::as_str) == Some("custom-title")
        {
            custom_title = value
                .get("customTitle")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if summary.is_none() {
            if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(message) = value.get("message") {
                let text = message.get("content").map(extract_text).unwrap_or_default();
                if !text.trim().is_empty() {
                    summary = Some(text);
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() && custom_title.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_session_id_from_filename(path));
    let session_id = session_id?;

    // Title priority: custom-title > first user message > directory basename
    let title = custom_title
        .map(|t| truncate_summary(&t, TITLE_MAX_CHARS))
        .or_else(|| first_user_message.map(|t| truncate_summary(&t, TITLE_MAX_CHARS)))
        .or_else(|| {
            project_dir
                .as_deref()
                .and_then(path_basename)
                .map(|v| v.to_string())
        });

    let summary = summary.map(|text| truncate_summary(&text, 160));

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("claude --resume {session_id}")),
    })
}

fn is_agent_session(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("agent-"))
        .unwrap_or(false)
}

fn infer_session_id_from_filename(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_main_file_and_sidecar_directory() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("abc123-session.jsonl");
        let sidecar = temp.path().join("abc123-session");
        let subagents = sidecar.join("subagents");
        let tool_results = sidecar.join("tool-results");

        std::fs::create_dir_all(&subagents).expect("create subagents");
        std::fs::create_dir_all(&tool_results).expect("create tool-results");
        std::fs::write(subagents.join("agent-1.jsonl"), "{}").expect("write subagent");
        std::fs::write(tool_results.join("tool-1.txt"), "result").expect("write tool result");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-123\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write session");

        delete_session(temp.path(), &path, "session-123").expect("delete session");

        assert!(!path.exists());
        assert!(!sidecar.exists());
    }

    #[test]
    fn load_messages_tool_use_shows_as_assistant() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"a.txt\"}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"File written\"}]},\"timestamp\":\"2026-03-06T10:00:01Z\"}\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: Write]"));
        assert_eq!(msgs[1].role, "tool");
        assert_eq!(msgs[1].content, "File written");
    }

    #[test]
    fn load_messages_mixed_text_and_tool_use() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Let me help.\"},{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Read\",\"input\":{}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("Let me help."));
        assert!(msgs[0].content.contains("[Tool: Read]"));
    }

    #[test]
    fn load_messages_mixed_user_tool_result_and_text_stays_user() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"result\"},{\"type\":\"text\",\"text\":\"Please continue\"}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert!(msgs[0].content.contains("Please continue"));
    }

    #[test]
    fn parse_session_uses_first_user_message_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-abc.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"How do I deploy?\"},\"sessionId\":\"session-abc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Here is how...\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("How do I deploy?"));
    }

    #[test]
    fn parse_session_custom_title_overrides_first_message() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-def.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-def\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"fix something\"},\"sessionId\":\"session-def\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"fix-login-bug\",\"sessionId\":\"session-def\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("fix-login-bug"));
    }

    #[test]
    fn parse_session_falls_back_to_dir_basename() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-ghi.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-ghi\",\"cwd\":\"/tmp/my-project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // No user message and no custom-title → falls back to dir basename
        assert_eq!(meta.title.as_deref(), Some("my-project"));
    }

    #[test]
    fn parse_session_truncates_long_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-trunc.jsonl");
        let long_msg = "a".repeat(200);
        std::fs::write(
            &path,
            format!(
                "{{\"sessionId\":\"session-trunc\",\"cwd\":\"/tmp/p\",\"timestamp\":\"2026-03-06T10:00:00Z\"}}\n\
                 {{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{long_msg}\"}},\"sessionId\":\"session-trunc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        let title = meta.title.unwrap();
        assert!(title.len() <= TITLE_MAX_CHARS + 3); // +3 for "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn parse_session_new_format_with_snapshot() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-new.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"请帮我重构这个函数\"},\"sessionId\":\"session-new\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"OK\"},\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("请帮我重构这个函数"));
    }

    #[test]
    fn parse_session_skips_command_caveat_and_slash_commands() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-clear.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\\n<command-message>clear</command-message>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:01Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:00:02Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"帮我看看工作区的改动\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("帮我看看工作区的改动"));
    }
}
