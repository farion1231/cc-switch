use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "qodercli";

/// qoder CLI 的会话记录与 Claude Code 同构：
/// `~/.qoder/projects/<encoded-cwd>/<session-id>.jsonl`，逐行 JSON，
/// 消息行为 `{"type":"user"|"assistant","message":{"role","content"},"timestamp",...}`。
/// 差异点：
/// - 工作目录记录在 `{"type":"workspace-directories","directories":[...]}` 条目里
///   （Claude 用每条消息的 `cwd` 字段），这里取首个目录并保留 `cwd` 兜底；
/// - 恢复命令为 `qodercli --resume <session-id>`。
pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = crate::qodercli_config::get_qodercli_dir().join("projects");
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

        // workspace-directories / runtime-config 等非消息行没有 message 字段，自然跳过
        let message = match value.get("message") {
            Some(message) => message,
            None => continue,
        };

        let mut role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        // tool_result 包在 user 消息里；重归类为 "tool" 角色（与 Claude 适配器一致）
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
            "Failed to parse qodercli session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "qodercli session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    if let Some(stem) = path.file_stem() {
        let sibling = path.parent().unwrap_or_else(|| Path::new("")).join(stem);
        remove_path_if_exists(&sibling).map_err(|e| {
            format!(
                "Failed to delete qodercli session sidecar {}: {e}",
                sibling.display()
            )
        })?;
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete qodercli session file {}: {e}",
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
            // qoder CLI: {"type":"workspace-directories","directories":["C:\\..."]}
            // 兜底：个别条目也可能带 cwd（Claude 风格）
            project_dir = value
                .get("directories")
                .and_then(Value::as_array)
                .and_then(|dirs| dirs.first())
                .and_then(Value::as_str)
                .map(|s| s.to_string())
                .or_else(|| {
                    value
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string())
                });
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
        resume_command: Some(format!("qodercli --resume {session_id}")),
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
    fn parse_session_reads_workspace_directories_as_project_dir() {
        let temp = tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("00000000-0000-4000-8000-000000000001.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"workspace-directories\",\"sessionId\":\"00000000-0000-4000-8000-000000000001\",\"directories\":[\"C:\\\\Users\\\\example-user\\\\project\"]}\n",
                "{\"type\":\"runtime-config\",\"sessionId\":\"00000000-0000-4000-8000-000000000001\",\"model\":\"provider/test-model\",\"timestamp\":1785163445103}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"test prompt\"},\"timestamp\":\"2026-07-27T14:44:11.443Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"test response\"},\"timestamp\":\"2026-07-27T14:44:12.000Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.session_id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(
            meta.project_dir.as_deref(),
            Some("C:\\Users\\example-user\\project")
        );
        assert_eq!(meta.title.as_deref(), Some("test prompt"));
        assert_eq!(meta.summary.as_deref(), Some("test response"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("qodercli --resume 00000000-0000-4000-8000-000000000001")
        );
        assert!(meta.created_at.is_some());
        assert!(meta.last_active_at.is_some());
    }

    #[test]
    fn load_messages_skips_non_message_entries() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"workspace-directories\",\"sessionId\":\"s1\",\"directories\":[\"/tmp\"]}\n",
                "{\"type\":\"runtime-config\",\"sessionId\":\"s1\",\"model\":\"p/m\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"},\"timestamp\":\"2026-07-27T10:00:00Z\"}\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hi");
    }

    #[test]
    fn load_messages_tool_result_reclassified_as_tool() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"done\"}]},\"timestamp\":\"2026-07-27T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "tool");
    }

    #[test]
    fn delete_session_removes_file_and_sidecar() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("abc-session.jsonl");
        let sidecar = temp.path().join("abc-session");
        std::fs::create_dir_all(&sidecar).expect("create sidecar");
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"sessionId\":\"abc-session\",\"message\":{\"role\":\"user\",\"content\":\"hi\"},\"timestamp\":\"2026-07-27T10:00:00Z\"}\n",
        )
        .expect("write session");

        delete_session(temp.path(), &path, "abc-session").expect("delete session");

        assert!(!path.exists());
        assert!(!sidecar.exists());
    }

    #[test]
    fn parse_session_skips_agent_files() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("agent-abc123.jsonl");
        std::fs::write(&path, "{\"sessionId\":\"x\"}\n").expect("write");
        assert!(parse_session(&path).is_none());
    }
}
