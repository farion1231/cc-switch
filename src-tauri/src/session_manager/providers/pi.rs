use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
};

const PROVIDER_ID: &str = "pi";
const DEFAULT_SESSION_RELATIVE_DIR: &str = ".pi/agent/sessions";

struct PiMessageEntry {
    entry_id: String,
    parent_id: Option<String>,
    message: Option<SessionMessage>,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_pi_session_dir();
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
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(value) => value,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        let entry_id = match value.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let parent_id = value
            .get("parentId")
            .and_then(Value::as_str)
            .map(|id| id.to_string());

        let message = match value.get("type").and_then(Value::as_str) {
            Some("message") => extract_session_message(&value),
            Some("custom_message") => extract_custom_message(&value),
            _ => None,
        };

        entries.push(PiMessageEntry {
            entry_id,
            parent_id,
            message,
        });
    }

    Ok(active_branch_messages(entries))
}

fn get_pi_session_dir() -> PathBuf {
    if let Ok(value) = std::env::var("PI_CODING_AGENT_SESSION_DIR") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }

    if let Ok(value) = std::env::var("PI_CODING_AGENT_DIR") {
        if !value.trim().is_empty() {
            return PathBuf::from(value).join("sessions");
        }
    }

    dirs::home_dir()
        .map(|home| home.join(DEFAULT_SESSION_RELATIVE_DIR))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_RELATIVE_DIR))
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 20, 50).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut title: Option<String> = None;

    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if value.get("type").and_then(Value::as_str) == Some("session") {
            session_id = session_id.or_else(|| {
                value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            });
            project_dir = project_dir.or_else(|| {
                value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
            });
            created_at =
                created_at.or_else(|| value.get("timestamp").and_then(parse_timestamp_to_ms));
        }

        if title.is_none() && value.get("type").and_then(Value::as_str) == Some("session_info") {
            title = value
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
        }
    }

    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;

    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }

        if title.is_none() && value.get("type").and_then(Value::as_str) == Some("session_info") {
            title = value
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
        }

        if summary.is_none() {
            summary = extract_summary_from_entry(&value);
        }

        if last_active_at.is_some() && summary.is_some() && title.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_session_id_from_filename(path))?;
    let title = title.or_else(|| {
        project_dir
            .as_deref()
            .and_then(path_basename)
            .map(|value| value.to_string())
    });
    let summary = summary.or_else(|| title.clone());

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("pi --session {session_id}")),
    })
}

fn extract_summary_from_entry(value: &Value) -> Option<String> {
    let entry_type = value.get("type").and_then(Value::as_str)?;
    match entry_type {
        "message" => {
            let message = value.get("message")?;
            if !should_include_message(message) {
                return None;
            }
            let text = extract_message_content(message);
            (!text.trim().is_empty()).then(|| truncate_summary(&text, 160))
        }
        "compaction" | "branch_summary" => value
            .get("summary")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(|s| truncate_summary(s, 160)),
        "custom_message" => {
            if value.get("display").and_then(Value::as_bool) == Some(false) {
                return None;
            }
            let text = value.get("content").map(extract_text).unwrap_or_default();
            (!text.trim().is_empty()).then(|| truncate_summary(&text, 160))
        }
        _ => None,
    }
}

fn extract_message_content(message: &Value) -> String {
    match message.get("role").and_then(Value::as_str) {
        Some("bashExecution") => {
            let command = message
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let output = message
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or_default();
            [command, output]
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        }
        Some("branchSummary") | Some("compactionSummary") => message
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => message.get("content").map(extract_text).unwrap_or_default(),
    }
}

fn extract_session_message(value: &Value) -> Option<SessionMessage> {
    let message = value.get("message")?;
    if !should_include_message(message) {
        return None;
    }

    let role = normalize_role(
        message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    );
    let content = extract_message_content(message);
    if content.trim().is_empty() {
        return None;
    }

    let ts = message
        .get("timestamp")
        .and_then(parse_numeric_or_string_timestamp)
        .or_else(|| value.get("timestamp").and_then(parse_timestamp_to_ms));

    Some(SessionMessage { role, content, ts })
}

fn extract_custom_message(value: &Value) -> Option<SessionMessage> {
    if value.get("display").and_then(Value::as_bool) == Some(false) {
        return None;
    }

    let content = value.get("content").map(extract_text).unwrap_or_default();
    if content.trim().is_empty() {
        return None;
    }

    let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);

    Some(SessionMessage {
        role: "system".to_string(),
        content,
        ts,
    })
}

fn should_include_message(message: &Value) -> bool {
    let role = message.get("role").and_then(Value::as_str);
    !matches!(
        role,
        Some("toolResult")
            | Some("bashExecution")
            | Some("custom")
            | Some("branchSummary")
            | Some("compactionSummary")
    )
}

fn normalize_role(role: &str) -> String {
    match role {
        "toolResult" => "tool".to_string(),
        "branchSummary" | "compactionSummary" | "custom" => "system".to_string(),
        "bashExecution" => "tool".to_string(),
        other => other.to_string(),
    }
}

fn parse_numeric_or_string_timestamp(value: &Value) -> Option<i64> {
    if let Some(timestamp) = value.as_i64() {
        return Some(timestamp);
    }
    parse_timestamp_to_ms(value)
}

fn active_branch_messages(entries: Vec<PiMessageEntry>) -> Vec<SessionMessage> {
    let Some(leaf) = entries.last() else {
        return Vec::new();
    };

    let mut active_ids = std::collections::HashSet::new();
    let mut current = Some(leaf.entry_id.as_str());

    while let Some(entry_id) = current {
        active_ids.insert(entry_id.to_string());
        current = entries
            .iter()
            .find(|entry| entry.entry_id == entry_id)
            .and_then(|entry| entry.parent_id.as_deref());
    }

    entries
        .into_iter()
        .filter(|entry| active_ids.contains(&entry.entry_id))
        .filter_map(|entry| entry.message)
        .collect()
}

fn infer_session_id_from_filename(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.rsplit_once('_').map(|(_, id)| id).or(Some(stem)))
        .filter(|id| !id.trim().is_empty())
        .map(|id| id.to_string())
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
