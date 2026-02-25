use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::openclaw_config::get_openclaw_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms};

const PROVIDER_ID: &str = "openclaw";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let agents_dir = get_openclaw_dir().join("agents");
    if !agents_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

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
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "sessions.json")
                .unwrap_or(false)
            {
                continue;
            }

            if let Some(meta) = parse_session_from_path(&path) {
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

        let role = match raw_role {
            "toolResult" => "tool".to_string(),
            other => other.to_string(),
        };

        let content = message.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);

        messages.push(SessionMessage {
            role,
            content,
            ts,
            tool_name: None,
        });
    }

    Ok(messages)
}

/// 从文件路径和 stat 信息推导会话元数据，不打开文件
fn parse_session_from_path(path: &Path) -> Option<SessionMeta> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())?;

    let (created_at, last_active_at) = get_file_timestamps(path);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title: None,
        summary: None,
        project_dir: None,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: None,
    })
}

/// 从文件 metadata 获取创建时间和修改时间（毫秒）
fn get_file_timestamps(path: &Path) -> (Option<i64>, Option<i64>) {
    let md = match std::fs::metadata(path) {
        Ok(md) => md,
        Err(_) => return (None, None),
    };

    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);

    let ctime = md
        .created()
  .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);

    (ctime, mtime)
}
