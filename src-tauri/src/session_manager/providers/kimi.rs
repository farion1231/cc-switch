//! Kimi Code CLI 会话支持
//!
//! Kimi 的会话按目录组织（官方 data-locations）：
//! ```text
//! $KIMI_CODE_HOME/sessions/<workDirKey>/<sessionId>/
//! ├── state.json               # id / cwd / createdAt / updatedAt
//! └── agents/main/wire.jsonl   # 会话消息记录（context.append_message）
//! ```

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::kimi_config::get_kimi_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "kimi";

pub fn session_roots() -> Vec<PathBuf> {
    vec![get_kimi_dir().join("sessions")]
}

/// 扫描 `sessions/<workDirKey>/<sessionId>/` 下的所有会话。
pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_kimi_dir().join("sessions");
    if !root.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    if let Ok(workdirs) = std::fs::read_dir(&root) {
        for workdir in workdirs.flatten() {
            let workdir_path = workdir.path();
            if !workdir_path.is_dir() {
                continue;
            }
            if let Ok(session_dirs) = std::fs::read_dir(&workdir_path) {
                for session_dir in session_dirs.flatten() {
                    let path = session_dir.path();
                    if path.is_dir() {
                        if let Some(meta) = scan_session_dir(&path) {
                            sessions.push(meta);
                        }
                    }
                }
            }
        }
    }

    sessions
}

/// 解析单个会话目录（state.json + wire.jsonl）。
fn scan_session_dir(dir: &Path) -> Option<SessionMeta> {
    let state_path = dir.join("state.json");
    let state: Value = if state_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(state_path).ok()?).ok()?
    } else {
        Value::Null
    };

    let session_id = state
        .get("id")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            dir.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })?;

    let cwd = state
        .get("cwd")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let created_at = state
        .get("createdAt")
        .or_else(|| state.get("created_at"))
        .and_then(parse_timestamp_to_ms);
    let updated_at = state
        .get("updatedAt")
        .or_else(|| state.get("updated_at"))
        .and_then(parse_timestamp_to_ms);

    // 标题：优先用第一条 user 消息（state.json 无 title 字段）
    let wire_path = dir.join("agents").join("main").join("wire.jsonl");
    let title = if wire_path.exists() {
        first_user_message(&wire_path)
    } else {
        None
    };

    let source_path = if wire_path.exists() {
        Some(wire_path.to_string_lossy().to_string())
    } else {
        Some(dir.to_string_lossy().to_string())
    };

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title: title
            .as_ref()
            .map(|t| truncate_summary(t, TITLE_MAX_CHARS).to_string()),
        summary: title,
        project_dir: cwd,
        created_at,
        last_active_at: updated_at.or(created_at),
        source_path,
        resume_command: None,
    })
}

/// 从 wire.jsonl 提取第一条用户消息作为标题。
fn first_user_message(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("context.append_message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let text = message.get("content").map(extract_text).unwrap_or_default();
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

/// 从 wire.jsonl 加载消息（`context.append_message` 行）。
pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value.get("type").and_then(Value::as_str) != Some("context.append_message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };

        let role = match message.get("role").and_then(Value::as_str) {
            Some(r) if !r.is_empty() => r.to_string(),
            _ => continue,
        };

        let content = message.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        // Kimi wire 的 time 为毫秒时间戳
        let ts = value.get("time").and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

/// 删除整个会话目录（source_path 指向 wire.jsonl 或会话目录）。
pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    // wire.jsonl 位于 <session>/agents/main/wire.jsonl —— 会话目录是其上两级
    let session_dir = if path.join("state.json").exists() {
        path.to_path_buf()
    } else {
        path.parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                format!(
                    "Failed to resolve Kimi session directory: {}",
                    path.display()
                )
            })?
    };

    // 防误删：确认目录名与会话 ID 一致
    let dir_name = session_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if dir_name != session_id {
        return Err(format!(
            "Kimi session directory mismatch: expected {session_id}, found {dir_name}"
        ));
    }

    std::fs::remove_dir_all(&session_dir).map_err(|e| {
        format!(
            "Failed to delete Kimi session directory {}: {e}",
            session_dir.display()
        )
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_wire(dir: &Path, lines: &[&str]) -> PathBuf {
        let wire = dir.join("agents").join("main").join("wire.jsonl");
        std::fs::create_dir_all(wire.parent().expect("parent")).expect("create dirs");
        let mut f = File::create(&wire).expect("create wire");
        for line in lines {
            writeln!(f, "{line}").expect("write line");
        }
        f.flush().expect("flush");
        wire
    }

    #[test]
    fn scan_session_dir_extracts_meta_and_title() {
        let dir = tempdir().expect("tempdir");
        let session_dir = dir.path().join("wd_x").join("session_1");
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        std::fs::write(
            session_dir.join("state.json"),
            r#"{"id":"session_1","cwd":"/tmp/proj","createdAt":1786436108393,"updatedAt":1786436158553}"#,
        )
        .expect("write state");

        write_wire(
            &session_dir,
            &[
                r#"{"type":"metadata","protocol_version":"1.5"}"#,
                r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Hello Kimi"}]},"time":1786436109000}"#,
                r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"Hi"}]},"time":1786436109500}"#,
            ],
        );

        let meta = scan_session_dir(&session_dir).expect("meta");
        assert_eq!(meta.session_id, "session_1");
        assert_eq!(meta.title.as_deref(), Some("Hello Kimi"));
        assert_eq!(meta.project_dir.as_deref(), Some("/tmp/proj"));
        assert_eq!(meta.created_at, Some(1786436108393));
        assert_eq!(meta.last_active_at, Some(1786436158553));
        assert!(meta
            .source_path
            .as_deref()
            .unwrap_or("")
            .ends_with("wire.jsonl"));
    }

    #[test]
    fn load_messages_parses_append_message_lines() {
        let dir = tempdir().expect("tempdir");
        let wire = write_wire(
            dir.path(),
            &[
                r#"{"type":"metadata"}"#,
                r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"What is Rust?"}]},"time":1786436109000}"#,
                r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"A language."}]},"time":1786436109500}"#,
                r#"{"type":"turn.ended"}"#,
            ],
        );

        let msgs = load_messages(&wire).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "What is Rust?");
        assert_eq!(msgs[0].ts, Some(1786436109000));
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn delete_session_removes_directory_and_guards_mismatch() {
        let dir = tempdir().expect("tempdir");
        let session_dir = dir.path().join("wd_x").join("session_9");
        std::fs::create_dir_all(session_dir.join("agents")).expect("create");
        std::fs::write(session_dir.join("state.json"), r#"{"id":"session_9"}"#)
            .expect("write state");
        let wire = write_wire(&session_dir, &[r#"{"type":"metadata"}"#]);

        // 用 wire.jsonl 路径删除 → 删除整个会话目录
        delete_session(dir.path(), &wire, "session_9").expect("delete");
        assert!(!session_dir.exists());

        // ID 不匹配应拒绝
        let session_dir2 = dir.path().join("wd_y").join("session_10");
        std::fs::create_dir_all(&session_dir2).expect("create");
        std::fs::write(session_dir2.join("state.json"), r#"{"id":"session_10"}"#)
            .expect("write state");
        let err = delete_session(dir.path(), &session_dir2.join("state.json"), "wrong-id")
            .expect_err("mismatch should fail");
        assert!(err.contains("mismatch"));
    }
}
