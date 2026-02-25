use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, path_basename};

const PROVIDER_ID: &str = "opencode";

/// Return the OpenCode data directory.
///
/// Respects `XDG_DATA_HOME` on all platforms; falls back to
/// `~/.local/share/opencode/storage/`.
fn get_opencode_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("opencode").join("storage");
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".local/share/opencode/storage"))
        .unwrap_or_else(|| PathBuf::from(".local/share/opencode/storage"))
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let storage = get_opencode_data_dir();
    let session_dir = storage.join("session");
    if !session_dir.exists() {
        return Vec::new();
    }

    let mut json_files = Vec::new();
    collect_json_files(&session_dir, &mut json_files);

    let mut sessions = Vec::new();
    for path in json_files {
        if let Some(meta) = parse_session(&storage, &path) {
            sessions.push(meta);
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    // `path` is the message directory: storage/message/{sessionID}/
    if !path.is_dir() {
        return Err(format!("Message directory not found: {}", path.display()));
    }

    let storage = path
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| "Cannot determine storage root from message path".to_string())?;

    let mut msg_files = Vec::new();
    collect_json_files(path, &mut msg_files);

    let mut entries: Vec<(i64, String, String, String)> = Vec::new();

    for msg_path in &msg_files {
        let data = match std::fs::read_to_string(msg_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let msg_id = match value.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let created_ts = value
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(parse_timestamp_to_ms)
            .unwrap_or(0);

        let part_dir = storage.join("part").join(&msg_id);
        let text = collect_parts_text(&part_dir);
        if text.trim().is_empty() {
            continue;
        }

        entries.push((created_ts, msg_id, role, text));
    }

    entries.sort_by_key(|(ts, _, _, _)| *ts);

    let messages = entries
        .into_iter()
        .map(|(ts, _, role, content)| SessionMessage {
            role,
            content,
            ts: if ts > 0 { Some(ts) } else { None },
            tool_name: None,
        })
        .collect();

    Ok(messages)
}

/// 读取 session JSON 提取元数据，不读取消息内容（跳过 summary）
fn parse_session(storage: &Path, path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("id").and_then(Value::as_str)?.to_string();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let directory = value
        .get("directory")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let created_at = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(parse_timestamp_to_ms);
    let updated_at = value
        .get("time")
        .and_then(|t| t.get("updated"))
        .and_then(parse_timestamp_to_ms);

    let display_title = title.or_else(|| {
        directory
            .as_deref()
            .and_then(path_basename)
            .map(|s| s.to_string())
    });

    let msg_dir = storage.join("message").join(&session_id);
    let source_path = msg_dir.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: display_title,
        summary: None,
        project_dir: directory,
        created_at,
        last_active_at: updated_at.or(created_at),
        source_path: Some(source_path),
        resume_command: Some(format!("opencode session resume {session_id}")),
    })
}

/// Collect text content from all parts in a part directory.
fn collect_parts_text(part_dir: &Path) -> String {
    if !part_dir.is_dir() {
        return String::new();
    }

    let mut parts = Vec::new();
    collect_json_files(part_dir, &mut parts);

    let mut texts = Vec::new();
    for part_path in &parts {
        let data = match std::fs::read_to_string(part_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }

        if let Some(text) = value.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                texts.push(text.to_string());
            }
        }
    }

    texts.join("\n")
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) {
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
            collect_json_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
}
