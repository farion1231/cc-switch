use std::path::Path;

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::parse_timestamp_to_ms;

const PROVIDER_ID: &str = "gemini";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

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

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
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
    let data =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    let value: Value =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse session JSON: {e}"))?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;

    let mut result = Vec::new();
    for msg in messages {
        let content = match msg.get("content").and_then(Value::as_str) {
            Some(c) if !c.trim().is_empty() => c.to_string(),
            _ => continue,
        };

        let role = match msg.get("type").and_then(Value::as_str) {
            Some("gemini") => "assistant".to_string(),
            Some("user") => "user".to_string(),
            Some(other) => other.to_string(),
            None => continue,
        };

        let ts = msg.get("timestamp").and_then(parse_timestamp_to_ms);

        result.push(SessionMessage {
            role,
            content,
            ts,
            tool_name: None,
        });
    }

    Ok(result)
}

/// 从文件路径和 stat 信息推导会话元数据，不读文件内容
fn parse_session_from_path(path: &Path) -> Option<SessionMeta> {
    // 文件名格式: session-<sessionId>.json
    let file_name = path.file_stem()?.to_str()?;
    let session_id = file_name.strip_prefix("session-")
        .unwrap_or(file_name)
        .to_string();

    if session_id.is_empty() {
        return None;
    }

    let (created_at, last_active_at) = get_file_timestamps(path);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: None,
        summary: None,
        project_dir: None,
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("gemini --resume {session_id}")),
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
