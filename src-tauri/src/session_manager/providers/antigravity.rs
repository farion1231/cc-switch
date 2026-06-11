use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::{collections::HashSet, time::UNIX_EPOCH};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, truncate_summary};

const PROVIDER_ID: &str = "antigravity";

pub fn session_roots() -> Vec<PathBuf> {
    let root = crate::antigravity_config::get_antigravity_dir();
    vec![
        root.join("antigravity"),
        root.join("antigravity-ide"),
        root.join("antigravity-cli"),
    ]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    for root in session_roots() {
        let brain = root.join("brain");
        let Ok(entries) = fs::read_dir(brain) else {
            continue;
        };

        for entry in entries.flatten() {
            let session_dir = entry.path();
            if !session_dir.is_dir() {
                continue;
            }
            let Some(session_id) = session_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let transcript = session_dir
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if let Some(meta) = parse_session(&transcript, session_id) {
                sessions.push(meta);
            }
        }
    }

    sessions.sort_by_key(|session| {
        std::cmp::Reverse(session.last_active_at.or(session.created_at).unwrap_or(0))
    });
    let mut seen = HashSet::new();
    sessions.retain(|session| seen.insert(session.session_id.clone()));
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Failed to open Antigravity transcript: {error}"))?;
    let mut messages = Vec::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let source = value.get("source").and_then(Value::as_str).unwrap_or("");
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let role = if source == "USER_EXPLICIT" && event_type == "USER_INPUT" {
            "user"
        } else if source == "MODEL" && matches!(event_type, "PLANNER_RESPONSE" | "GENERIC") {
            "assistant"
        } else {
            continue;
        };

        let Some(content) = value.get("content").and_then(Value::as_str) else {
            continue;
        };
        let content = clean_content(content);
        if content.is_empty() {
            continue;
        }

        messages.push(SessionMessage {
            role: role.to_string(),
            content,
            ts: value.get("created_at").and_then(parse_timestamp_to_ms),
        });
    }

    Ok(messages)
}

pub fn delete_session(root: &Path, transcript: &Path, session_id: &str) -> Result<bool, String> {
    let session_dir = transcript
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| "Invalid Antigravity transcript path".to_string())?;
    let actual_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid Antigravity session directory".to_string())?;
    if actual_id != session_id {
        return Err(format!(
            "Antigravity session ID mismatch: expected {session_id}, found {actual_id}"
        ));
    }

    let brain_root = root.join("brain");
    if !session_dir.starts_with(&brain_root) {
        return Err("Antigravity transcript is outside the brain directory".to_string());
    }

    fs::remove_dir_all(session_dir).map_err(|error| {
        format!(
            "Failed to delete Antigravity session directory {}: {error}",
            session_dir.display()
        )
    })?;

    remove_conversation_files(root, session_id)?;
    for directory in [
        "annotations",
        "code_tracker",
        "context_state",
        "html_artifacts",
        "implicit",
        "playground",
        "scratch",
    ] {
        let path = root.join(directory).join(session_id);
        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(|error| {
                format!(
                    "Failed to delete Antigravity session data {}: {error}",
                    path.display()
                )
            })?;
        } else if path.is_file() {
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "Failed to delete Antigravity session data {}: {error}",
                    path.display()
                )
            })?;
        }
    }

    Ok(true)
}

fn parse_session(transcript: &Path, session_id: &str) -> Option<SessionMeta> {
    if !transcript.is_file() {
        return None;
    }
    let file = fs::File::open(transcript).ok()?;
    let mut title = None;
    let mut created_at = None;
    let mut last_active_at = None;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let source = value.get("source").and_then(Value::as_str).unwrap_or("");
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let is_user = source == "USER_EXPLICIT" && event_type == "USER_INPUT";
        let is_assistant =
            source == "MODEL" && matches!(event_type, "PLANNER_RESPONSE" | "GENERIC");
        if !is_user && !is_assistant {
            continue;
        }

        let timestamp = value.get("created_at").and_then(parse_timestamp_to_ms);
        if created_at.is_none() {
            created_at = timestamp;
        }
        if timestamp.is_some() {
            last_active_at = timestamp;
        }

        if title.is_none() && is_user {
            if let Some(content) = value.get("content").and_then(Value::as_str) {
                let content = clean_content(content);
                if !content.is_empty() {
                    title = Some(truncate_summary(&content, 80));
                }
            }
        }
    }

    let last_active_at = last_active_at.or_else(|| {
        transcript
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
    });

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.to_string(),
        title: title.clone(),
        summary: title,
        project_dir: None,
        created_at,
        last_active_at,
        source_path: Some(transcript.to_string_lossy().to_string()),
        resume_command: Some(format!("agy --conversation {session_id}")),
    })
}

fn clean_content(content: &str) -> String {
    let content = content
        .replace("<USER_REQUEST>", "")
        .replace("</USER_REQUEST>", "");
    let content = content
        .split("<ADDITIONAL_METADATA>")
        .next()
        .unwrap_or(&content);
    content.trim().to_string()
}

fn remove_conversation_files(root: &Path, session_id: &str) -> Result<(), String> {
    let conversations = root.join("conversations");
    for suffix in [".db", ".db-shm", ".db-wal", ".pb"] {
        let path = conversations.join(format!("{session_id}{suffix}"));
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "Failed to delete Antigravity conversation file {}: {error}",
                    path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_user_and_model_messages_only() {
        let temp = tempdir().expect("tempdir");
        let transcript = temp.path().join("transcript.jsonl");
        fs::write(
            &transcript,
            concat!(
                "{\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"created_at\":\"2026-06-05T04:15:58Z\",\"content\":\"<USER_REQUEST>hello</USER_REQUEST>\"}\n",
                "{\"source\":\"MODEL\",\"type\":\"PLANNER_RESPONSE\",\"created_at\":\"2026-06-05T04:16:00Z\",\"content\":\"world\"}\n",
                "{\"source\":\"MODEL\",\"type\":\"RUN_COMMAND\",\"created_at\":\"2026-06-05T04:16:01Z\",\"content\":\"ignored\"}\n"
            ),
        )
        .expect("write transcript");

        let messages = load_messages(&transcript).expect("load transcript");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].role, "assistant");
    }
}
