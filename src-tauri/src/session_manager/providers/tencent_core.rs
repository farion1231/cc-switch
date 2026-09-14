//! 腾讯 CodeBuddy / WorkBuddy 会话转录共享核心。
//!
//! 两者的磁盘布局与数据格式完全同构（与 Claude Code 类似，但事件字段在顶层）：
//!
//! ```text
//! ~/.codebuddy/projects/<workspace 编码目录>/<sessionId>.jsonl
//! ~/.workbuddy/projects/<workspace 编码目录>/<sessionId>.jsonl
//! ```
//!
//! 每行一个事件，字段在顶层：`sessionId` / `cwd` / `timestamp`（epoch ms）/
//! `type` / `role` / `content`。相关事件类型：
//! - `message`：`role` 为 `user` / `assistant`，`content` 为内容块数组
//! - `function_call`：工具调用（`name`），会话中展示为 assistant 的 `[Tool: …]`
//! - `function_call_result`：工具结果（`output`），展示为 `tool`
//! - `reasoning` / `summary` / `turn-metrics` / `file-history-snapshot`：忽略
//!
//! `<sessionId>/subagents/agent-*.jsonl` 是子代理转录，按文件名前缀 `agent-` 跳过
//! （与 claude.rs 的 `is_agent_session` 策略一致）。
//!
//! 本模块不直接暴露 provider，而是被 `codebuddy.rs` / `workbuddy.rs` 两个薄封装
//! 复用，避免两个相同布局的 provider 重复数百行解析代码。

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

/// 标记一段会被注入为 "user" 角色的系统上下文（避免污染标题与摘要）。
const SYSTEM_REMINDER_MARKER: &str = "<system-reminder";

/// 扫描单个 JSONL 会话文件，返回会话元数据。
pub fn scan_session(
    path: &Path,
    provider_id: &str,
    resume_prefix: Option<&str>,
) -> Option<SessionMeta> {
    if is_agent_session(path) {
        return None;
    }

    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;

    // 从头行提取元数据与首条真实用户消息
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
            project_dir = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if first_user_message.is_none() {
            let is_user = value.get("type").and_then(Value::as_str) == Some("message")
                && value.get("role").and_then(Value::as_str) == Some("user");
            if is_user {
                let text = value.get("content").map(extract_text).unwrap_or_default();
                let trimmed = text.trim();
                // 跳过系统注入的 user-context / 斜杠命令等噪音
                if !trimmed.is_empty()
                    && !trimmed.contains(SYSTEM_REMINDER_MARKER)
                    && !trimmed.starts_with("<command-name>")
                {
                    first_user_message = Some(trimmed.to_string());
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

    // 从尾行（倒序）提取最后活跃时间与摘要文本
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
        if summary.is_none()
            && value.get("type").and_then(Value::as_str) == Some("message")
            && value.get("content").is_some()
        {
            let text = value.get("content").map(extract_text).unwrap_or_default();
            let trimmed = text.trim();
            if !trimmed.is_empty() && !trimmed.contains(SYSTEM_REMINDER_MARKER) {
                summary = Some(trimmed.to_string());
            }
        }
        if last_active_at.is_some() && summary.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_session_id_from_filename(path));
    let session_id = session_id?;

    // 标题优先级：首条真实用户消息 > 工作目录 basename
    let title = first_user_message
        .map(|text| truncate_summary(&text, TITLE_MAX_CHARS))
        .or_else(|| {
            project_dir
                .as_deref()
                .and_then(path_basename)
                .map(|v| v.to_string())
        });

    let summary = summary.map(|text| truncate_summary(&text, 160));
    let resume_command = resume_prefix.map(|prefix| format!("{prefix} {session_id}"));

    Some(SessionMeta {
        provider_id: provider_id.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command,
    })
}

/// 载入单个会话文件的消息列表。
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

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

        match event_type {
            "message" => {
                let role = value
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let content = value.get("content").map(extract_text).unwrap_or_default();
                if content.trim().is_empty() || content.contains(SYSTEM_REMINDER_MARKER) {
                    continue;
                }
                messages.push(SessionMessage { role, content, ts });
            }
            "function_call" => {
                // 工具调用在独立事件中，作为 assistant 的 [Tool: name] 展示
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                messages.push(SessionMessage {
                    role: "assistant".to_string(),
                    content: format!("[Tool: {name}]"),
                    ts,
                });
            }
            "function_call_result" => {
                let content = value.get("output").map(extract_text).unwrap_or_default();
                if content.trim().is_empty() {
                    continue;
                }
                messages.push(SessionMessage {
                    role: "tool".to_string(),
                    content,
                    ts,
                });
            }
            // reasoning / summary / turn-metrics / file-history-snapshot / 其他：忽略
            _ => {}
        }
    }

    Ok(messages)
}

/// 删除会话文件及其伴随产物（`<sessionId>.meta.json` 与 `<sessionId>/` 侧车目录）。
pub fn delete_session(path: &Path, session_id: &str) -> Result<bool, String> {
    let file_session_id = read_session_id(path)
        .ok_or_else(|| format!("Failed to parse session metadata: {}", path.display()))?;

    if file_session_id != session_id {
        return Err(format!(
            "Session ID mismatch: expected {session_id}, found {file_session_id}"
        ));
    }

    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Err(format!("Invalid session file name: {}", path.display()));
    };
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    let sidecar_dir = parent.join(stem);
    remove_path_if_exists(&sidecar_dir).map_err(|e| {
        format!(
            "Failed to delete session sidecar directory {}: {e}",
            sidecar_dir.display()
        )
    })?;

    let meta_file = parent.join(format!("{stem}.meta.json"));
    remove_path_if_exists(&meta_file).map_err(|e| {
        format!(
            "Failed to delete session meta file {}: {e}",
            meta_file.display()
        )
    })?;

    std::fs::remove_file(path)
        .map_err(|e| format!("Failed to delete session file {}: {e}", path.display()))?;

    Ok(true)
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

/// 从头几行解析出文件记录的 `sessionId`。
fn read_session_id(path: &Path) -> Option<String> {
    let (head, _) = read_head_tail_lines(path, 10, 1).ok()?;
    for line in &head {
        let value: Value = serde_json::from_str(line).ok()?;
        if let Some(id) = value.get("sessionId").and_then(Value::as_str) {
            return Some(id.to_string());
        }
    }
    None
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

/// 递归收集 `root` 下的 `.jsonl` 文件（与 claude.rs 同款遍历）。
pub fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 构造一行 flat 结构的 codebuddy/workbuddy 事件
    fn message_line(event_type: &str, role: &str, session_id: &str, ts: i64, text: &str) -> String {
        format!(
            r#"{{"type":"{event_type}","role":"{role}","content":[{{"type":"input_text","text":"{text}"}}],"sessionId":"{session_id}","cwd":"C:\\ws\\proj","timestamp":{ts}}}"#
        )
    }

    #[test]
    fn scan_session_extracts_metadata_and_title() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-abc.jsonl");
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                message_line(
                    "message",
                    "user",
                    "session-abc",
                    1_771_000_000_000,
                    "你好，帮我看下这个 bug"
                ),
                message_line(
                    "message",
                    "assistant",
                    "session-abc",
                    1_771_000_001_000,
                    "好的"
                )
            ),
        )
        .unwrap();

        let meta = scan_session(&path, "codebuddy", Some("codebuddy --resume")).unwrap();
        assert_eq!(meta.provider_id, "codebuddy");
        assert_eq!(meta.session_id, "session-abc");
        assert_eq!(meta.title.as_deref(), Some("你好，帮我看下这个 bug"));
        assert_eq!(meta.created_at, Some(1_771_000_000_000));
        assert_eq!(meta.last_active_at, Some(1_771_000_001_000));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("codebuddy --resume session-abc")
        );
    }

    #[test]
    fn scan_session_skips_system_reminder_and_uses_fallback_title() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-def.jsonl");
        let injected =
            "<system-reminder data-role=\"user-context\">\\n<user_info>OS Windows</user_info>";
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                message_line(
                    "message",
                    "user",
                    "session-def",
                    1_771_000_000_000,
                    injected
                ),
                message_line(
                    "message",
                    "assistant",
                    "session-def",
                    1_771_000_001_000,
                    "hello"
                )
            ),
        )
        .unwrap();

        let meta = scan_session(&path, "workbuddy", None).unwrap();
        // 系统注入行被跳过 → 无真实用户消息 → 回退到 cwd basename（proj）
        assert_eq!(meta.title.as_deref(), Some("proj"));
        assert!(meta.resume_command.is_none());
    }

    #[test]
    fn scan_session_skips_agent_subfiles() {
        let temp = tempdir().unwrap();
        let agent = temp.path().join("agent-deadbeef.jsonl");
        std::fs::write(&agent, message_line("message", "user", "x", 1, "sub")).unwrap();
        assert!(scan_session(&agent, "codebuddy", Some("codebuddy --resume")).is_none());
    }

    #[test]
    fn load_messages_maps_flat_events() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-msg.jsonl");
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n{}\n",
                message_line("message", "user", "s", 1, "请执行搜索"),
                r#"{"type":"function_call","name":"Grep","callId":"c1","sessionId":"s","timestamp":2}"#,
                r#"{"type":"function_call_result","output":{"type":"text","text":"matched 3"},"sessionId":"s","timestamp":3}"#,
                message_line("message", "assistant", "s", 4, "结果如上")
            ),
        )
        .unwrap();

        let msgs = load_messages(&path).unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "请执行搜索");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "[Tool: Grep]");
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].content, "matched 3");
        assert_eq!(msgs[3].role, "assistant");
    }

    #[test]
    fn load_messages_skips_reasoning_and_system_noise() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-reason.jsonl");
        let injected = "<system-reminder data-role=\"user-context\">context";
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                message_line("message", "user", "s", 1, injected),
                r#"{"type":"reasoning","content":[{"type":"text","text":"thinking..."}],"sessionId":"s","timestamp":2}"#
            ),
        )
        .unwrap();

        let msgs = load_messages(&path).unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn delete_session_removes_file_meta_and_sidecar_dir() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-del.jsonl");
        let sidecar = temp.path().join("session-del");
        let meta = temp.path().join("session-del.meta.json");
        std::fs::write(
            &path,
            message_line("message", "user", "session-del", 1, "hi"),
        )
        .unwrap();
        std::fs::create_dir_all(sidecar.join("subagents")).unwrap();
        std::fs::write(sidecar.join("subagents").join("agent-1.jsonl"), "{}").unwrap();
        std::fs::write(&meta, "{}").unwrap();

        delete_session(&path, "session-del").unwrap();
        assert!(!path.exists());
        assert!(!sidecar.exists());
        assert!(!meta.exists());
    }

    #[test]
    fn delete_session_rejects_mismatched_id() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("session-mm.jsonl");
        std::fs::write(
            &path,
            message_line("message", "user", "session-mm", 1, "hi"),
        )
        .unwrap();

        let err = delete_session(&path, "session-other").unwrap_err();
        assert!(err.contains("Session ID mismatch"));
    }
}
