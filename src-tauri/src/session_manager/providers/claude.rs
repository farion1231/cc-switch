use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms};

const PROVIDER_ID: &str = "claude";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_claude_config_dir().join("projects");
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

/// 从文件路径和 stat 信息推导会话元数据，不打开文件
fn parse_session_from_path(path: &Path) -> Option<SessionMeta> {
    if is_agent_session(path) {
        return None;
    }

    let session_id = infer_session_id_from_filename(path)?;

    // Claude 目录结构: ~/.claude/projects/<project-dir-encoded>/<session-id>.jsonl
    // 父目录名即为编码后的项目路径，如 -Users-sam-Documents-myproject
    let project_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(decode_project_dir);

    let title = project_dir
        .as_deref()
        .and_then(|d| d.split(['/', '\\']).filter(|s| !s.is_empty()).last())
        .map(|s| s.to_string());

    let (created_at, last_active_at) = get_file_timestamps(path);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary: None,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("claude --resume {session_id}")),
    })
}

/// 将编码后的目录名还原为路径，如 "-Users-sam-Documents-myproject" -> "/Users/sam/Documents/myproject"
fn decode_project_dir(encoded: &str) -> String {
    if encoded.starts_with('-') {
        format!("/{}", &encoded[1..]).replace('-', "/")
    } else {
        encoded.replace('-', "/")
    }
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

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();
    // tool_use_id -> tool name 映射
    let mut tool_name_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

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

        let raw_role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        // 从 assistant 消息的 tool_use 块中收集工具名称
        if raw_role == "assistant" {
            if let Some(Value::Array(items)) = message.get("content") {
                for item in items {
                    if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                        if let (Some(id), Some(name)) = (
                            item.get("id").and_then(Value::as_str),
                            item.get("name").and_then(Value::as_str),
                        ) {
                            tool_name_map.insert(id.to_string(), name.to_string());
                        }
                    }
                }
            }
        }

        // content 数组全是 tool_result 时，标记为 "tool" 并提取工具名称
        let (role, tool_name) = if raw_role == "user" {
            if let Some(Value::Array(items)) = message.get("content") {
                let all_tool_result = !items.is_empty()
                    && items.iter().all(|item| {
                        item.get("type").and_then(Value::as_str) == Some("tool_result")
                    });
                if all_tool_result {
                    let name = items
                        .first()
                        .and_then(|item| item.get("tool_use_id"))
                        .and_then(Value::as_str)
                        .and_then(|id| tool_name_map.get(id))
                        .cloned();
                    ("tool".to_string(), name)
                } else {
                    (raw_role.to_string(), None)
                }
            } else {
                (raw_role.to_string(), None)
            }
        } else {
            (raw_role.to_string(), None)
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
            tool_name,
        });
    }

    Ok(messages)
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
