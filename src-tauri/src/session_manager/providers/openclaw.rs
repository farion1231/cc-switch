use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::openclaw_config::get_openclaw_dir;
use crate::{
    config::write_json_file,
    session_manager::{SessionMessage, SessionMeta},
};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "openclaw";

/// Strip trailing `\n[message_id: ...]` metadata injected by OpenClaw gateway.
fn strip_message_id_suffix(text: &str) -> &str {
    let trimmed_end = text.trim_end();
    let Some(pos) = trimmed_end.rfind('\n') else {
        return text;
    };

    let suffix = &trimmed_end[pos + 1..];
    if is_message_id_metadata_line(suffix) {
        &text[..pos]
    } else {
        text
    }
}

fn is_message_id_metadata_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(id) = trimmed
        .strip_prefix("[message_id:")
        .and_then(|value| value.strip_suffix(']'))
    else {
        return false;
    };

    !id.trim().is_empty()
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let agents_dir = get_openclaw_dir().join("agents");
    if !agents_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Traverse each agent directory
    let agent_entries = match std::fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(_) => return sessions,
    };

    for agent_entry in agent_entries.flatten() {
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }

        let sessions_dir = agent_path.join("sessions");
        if !sessions_dir.is_dir() {
            continue;
        }

        let session_entries = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let display_names = load_display_names(&sessions_dir);

        for entry in session_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }

            if let Some(meta) = parse_session(&path, Some(&display_names)) {
                sessions.push(meta);
            }
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

        if value.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        let message = match value.get("message") {
            Some(msg) => msg,
            None => continue,
        };

        let raw_role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        // Map OpenClaw roles to our standard roles
        let role = match raw_role {
            "toolResult" => "tool".to_string(),
            other => other.to_string(),
        };

        let content = message.get("content").map(extract_text).unwrap_or_default();
        let content = strip_message_id_suffix(&content).to_string();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);

        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path, None).ok_or_else(|| {
        format!(
            "Failed to parse OpenClaw session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "OpenClaw session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    let index_path = path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("sessions.json");
    prune_sessions_index(&index_path, session_id, path)?;

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete OpenClaw session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

/// Read `sessions.json` index and build a sessionId → displayName lookup map.
/// Returns an empty map if the file does not exist or cannot be parsed.
fn load_display_names(sessions_dir: &Path) -> HashMap<String, String> {
    let index_path = sessions_dir.join("sessions.json");
    let content = match std::fs::read_to_string(&index_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let index: serde_json::Map<String, Value> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for (_key, entry) in &index {
        if let (Some(id), Some(name)) = (
            entry.get("sessionId").and_then(Value::as_str),
            entry.get("displayName").and_then(Value::as_str),
        ) {
            if !name.is_empty() {
                map.insert(id.to_string(), name.to_string());
            }
        }
    }
    map
}

fn parse_session(
    path: &Path,
    display_names: Option<&HashMap<String, String>>,
) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut summary: Option<String> = None;
    let mut first_user_message: Option<String> = None;

    // Extract metadata, summary, and first user message from head lines
    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }

        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

        if event_type == "session" {
            if session_id.is_none() {
                session_id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
            }
            if cwd.is_none() {
                cwd = value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
            }
            if let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
                created_at.get_or_insert(ts);
            }
            continue;
        }

        if event_type == "message" {
            if let Some(message) = value.get("message") {
                let text = message.get("content").map(extract_text).unwrap_or_default();
                let cleaned = strip_message_id_suffix(&text);
                if !cleaned.trim().is_empty() {
                    if first_user_message.is_none()
                        && message.get("role").and_then(Value::as_str) == Some("user")
                    {
                        first_user_message = Some(cleaned.trim().to_string());
                    }
                    if summary.is_none() {
                        summary = Some(cleaned.trim().to_string());
                    }
                }
            }
        }

        if session_id.is_some()
            && cwd.is_some()
            && created_at.is_some()
            && summary.is_some()
            && first_user_message.is_some()
        {
            break;
        }
    }

    // Extract last_active_at from tail lines (reverse order)
    let mut last_active_at: Option<i64> = None;
    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
            last_active_at = Some(ts);
            break;
        }
    }

    // Fall back to filename as session ID
    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });
    let session_id = session_id?;

    // Title priority: displayName (from sessions.json) > first user message > dir basename
    let title = display_names
        .and_then(|m| m.get(&session_id))
        .filter(|s| !s.is_empty())
        .map(|t| truncate_summary(t, TITLE_MAX_CHARS))
        .or_else(|| first_user_message.map(|t| truncate_summary(&t, TITLE_MAX_CHARS)))
        .or_else(|| {
            cwd.as_deref()
                .and_then(path_basename)
                .map(|s| s.to_string())
        });

    let summary = summary.map(|text| truncate_summary(&text, 160));

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir: cwd,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: None, // OpenClaw sessions are gateway-managed, no CLI resume
    })
}

fn prune_sessions_index(
    index_path: &Path,
    session_id: &str,
    source_path: &Path,
) -> Result<(), String> {
    if !index_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(index_path).map_err(|e| {
        format!(
            "Failed to read OpenClaw sessions index {}: {e}",
            index_path.display()
        )
    })?;
    let mut index: serde_json::Map<String, Value> =
        serde_json::from_str(&content).map_err(|e| {
            format!(
                "Failed to parse OpenClaw sessions index {}: {e}",
                index_path.display()
            )
        })?;

    let source = source_path.to_string_lossy();
    index.retain(|_, entry| {
        let same_id = entry.get("sessionId").and_then(Value::as_str) == Some(session_id);
        let same_file = entry.get("sessionFile").and_then(Value::as_str) == Some(source.as_ref());
        !(same_id || same_file)
    });

    write_json_file(index_path, &index).map_err(|e| {
        format!(
            "Failed to update OpenClaw sessions index {}: {e}",
            index_path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_session_uses_first_user_message_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-abc.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"session-abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"How do I deploy?\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"Here is how...\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path, None).unwrap();
        assert_eq!(meta.title.as_deref(), Some("How do I deploy?"));
    }

    #[test]
    fn parse_session_display_name_overrides_user_message() {
        let temp = tempdir().expect("tempdir");
        let sessions_dir = temp.path();

        let path = sessions_dir.join("session-abc.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"session-abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"fix something\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write session");

        std::fs::write(
            sessions_dir.join("sessions.json"),
            r#"{
                "agent:main:main": {
                    "sessionId": "session-abc",
                    "displayName": "重构登录模块"
                }
            }"#,
        )
        .expect("write index");

        let display_names = load_display_names(sessions_dir);
        let meta = parse_session(&path, Some(&display_names)).unwrap();
        assert_eq!(meta.title.as_deref(), Some("重构登录模块"));
    }

    #[test]
    fn parse_session_falls_back_to_dir_basename() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-def.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"session-def\",\"cwd\":\"/tmp/my-project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path, None).unwrap();
        // No user message and no displayName → falls back to dir basename
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
                "{{\"type\":\"session\",\"id\":\"session-trunc\",\"cwd\":\"/tmp/p\",\"timestamp\":\"2026-03-06T10:00:00Z\"}}\n\
                 {{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{long_msg}\"}},\"timestamp\":\"2026-03-06T10:01:00Z\"}}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path, None).unwrap();
        let title = meta.title.unwrap();
        assert!(title.len() <= TITLE_MAX_CHARS + 3); // +3 for "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn load_messages_strips_message_id_suffix_and_renders_tool_input() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Bash\",\"input\":{\"command\":\"pwd\"}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"content\":\"/tmp/project\\n[message_id: msg_1]\"},\"timestamp\":\"2026-03-06T10:00:01Z\"}\n"
            ),
        )
        .expect("write session");

        let messages = load_messages(&path).expect("load messages");
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("[Tool: Bash]"));
        assert!(messages[0].content.contains("Command: pwd"));
        assert_eq!(messages[1].role, "tool");
        assert_eq!(messages[1].content, "/tmp/project");
    }

    #[test]
    fn load_messages_preserves_content_whitespace_without_message_id_suffix() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"content\":\"  indented output\\nline\\n  \"},\"timestamp\":\"2026-03-06T10:00:01Z\"}\n",
        )
        .expect("write session");

        let messages = load_messages(&path).expect("load messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "tool");
        assert_eq!(messages[0].content, "  indented output\nline\n  ");
    }

    #[test]
    fn strip_message_id_suffix_preserves_embedded_message_id_lines() {
        let content = "command output\n[message_id: visible]\nmore output";

        assert_eq!(strip_message_id_suffix(content), content);
        assert_eq!(
            strip_message_id_suffix("command output\n[message_id: msg_1] trailing"),
            "command output\n[message_id: msg_1] trailing"
        );
    }

    #[test]
    fn strip_message_id_suffix_allows_trailing_whitespace_after_metadata() {
        assert_eq!(
            strip_message_id_suffix("command output\n[message_id: msg_1]\n  "),
            "command output"
        );
        assert_eq!(
            strip_message_id_suffix("command output\n\n[message_id: msg_1]\n  "),
            "command output\n"
        );
    }

    #[test]
    fn delete_session_updates_index_and_removes_jsonl() {
        let temp = tempdir().expect("tempdir");
        let sessions_dir = temp.path().join("main").join("sessions");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let session_path = sessions_dir.join("session-123.jsonl");
        std::fs::write(
            &session_path,
            concat!(
                "{\"type\":\"session\",\"id\":\"session-123\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write session");
        std::fs::write(
            sessions_dir.join("sessions.json"),
            format!(
                r#"{{
                  "agent:main:main": {{
                    "sessionId": "session-123",
                    "sessionFile": "{}"
                  }},
                  "agent:main:other": {{
                    "sessionId": "session-456",
                    "sessionFile": "{}/session-456.jsonl"
                  }}
                }}"#,
                session_path.display(),
                sessions_dir.display()
            ),
        )
        .expect("write index");

        delete_session(temp.path(), &session_path, "session-123").expect("delete session");

        assert!(!session_path.exists());
        let updated: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(sessions_dir.join("sessions.json")).expect("read index"),
        )
        .expect("parse index");
        assert!(updated.get("agent:main:main").is_none());
        assert!(updated.get("agent:main:other").is_some());
    }
}
