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
};

const PROVIDER_ID: &str = "openclaw";

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

        for entry in session_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            // Skip sessions.json index file
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "sessions.json")
                .unwrap_or(false)
            {
                continue;
            }

            if let Some(meta) = parse_session(&path) {
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

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut summary: Option<String> = None;

    // Extract metadata and first message summary from head lines
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

        // OpenClaw summary is the first message content
        if event_type == "message" && summary.is_none() {
            if let Some(message) = value.get("message") {
                let text = message.get("content").map(extract_text).unwrap_or_default();
                if !text.trim().is_empty() {
                    summary = Some(text);
                }
            }
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

    let title = cwd
        .as_deref()
        .and_then(path_basename)
        .map(|s| s.to_string());

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
