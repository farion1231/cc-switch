use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::codex_config::get_codex_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms};

const PROVIDER_ID: &str = "codex";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_codex_config_dir().join("sessions");
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);

    let mut sessions = Vec::new();
    for path in files {
        if let Some(meta) = parse_session_from_path(&path) {
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

        if value.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }

        let payload = match value.get("payload") {
            Some(payload) => payload,
            None => continue,
        };

        if payload.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let content = payload.get("content").map(extract_text).unwrap_or_default();
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
    let session_id = infer_session_id_from_filename(path)?;
    let (created_at, last_active_at) = get_file_timestamps(path);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: None,
        summary: None,
        project_dir: None,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("codex resume {session_id}")),
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

fn infer_session_id_from_filename(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let re =
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .ok()?;
    re.find(&file_name).map(|mat| mat.as_str().to_string())
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
