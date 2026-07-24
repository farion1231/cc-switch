use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "claude";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_claude_config_dir().join("projects");
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

        let message = match value.get("message") {
            Some(message) => message,
            None => continue,
        };

        let mut role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        // Claude wraps tool_result inside user messages; reclassify as "tool" role
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
            "Failed to parse Claude session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Claude session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    // Guard against path traversal before any filesystem mutation:
    // session_id must be a single safe component (no separators, not
    // `.` or `..`) before we join it onto the jobs directory below.
    if !is_safe_path_component(session_id) {
        return Err(format!(
            "Refusing to clean up jobs for unsafe session ID: {session_id}"
        ));
    }

    if let Some(stem) = path.file_stem() {
        let sibling = path.parent().unwrap_or_else(|| Path::new("")).join(stem);
        remove_path_if_exists(&sibling).map_err(|e| {
            format!(
                "Failed to delete Claude session sidecar {}: {e}",
                sibling.display()
            )
        })?;
    }

    // Clean up Claude Code jobs directory entries associated with this
    // session so the built-in agents panel (← key) does not show stale
    // entries after the session has been deleted.
    let jobs_dir = get_claude_config_dir().join("jobs");
    let jobs_subdir = jobs_dir.join(session_id);
    let jobs_file = jobs_dir.join(format!("{session_id}.json"));
    remove_path_if_exists(&jobs_subdir).map_err(|e| {
        format!(
            "Failed to delete Claude jobs directory {}: {e}",
            jobs_subdir.display()
        )
    })?;
    remove_path_if_exists(&jobs_file).map_err(|e| {
        format!(
            "Failed to delete Claude jobs file {}: {e}",
            jobs_file.display()
        )
    })?;

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Claude session file {}: {e}",
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
    let mut custom_title: Option<String> = None;

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
            project_dir = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Extract custom-title from head region as well; when a session
        // is renamed early and the conversation grows large the entry may
        // be beyond the tail window altogether.
        // NOTE: we intentionally do NOT guard with custom_title.is_none()
        // here — the head lines are in chronological order, so a later
        // rename within the head should overwrite an earlier one.  The
        // tail pass (reverse order, guarded) will still override this
        // value when a more recent rename falls in the tail window.
        if value.get("type").and_then(Value::as_str) == Some("custom-title") {
            let new_title = value
                .get("customTitle")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if new_title.is_some() {
                custom_title = new_title;
            }
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
        // Note: we intentionally do not break early in the head loop even
        // when all fields are populated — a custom-title rename may appear
        // after the first user message, and with head_n = 10 the overhead
        // of scanning a few extra lines is negligible.
    }

    // Extract last_active_at, summary, and custom-title from tail lines (reverse order).
    // We use a separate tail_found_title flag (instead of guarding on
    // custom_title.is_none()) so the tail can override a custom-title
    // value that was set from the head — the most recent rename near
    // EOF should always win.
    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;
    let mut tail_found_title = false;

    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Look for custom-title entry (take the last one, i.e. first in reverse).
        // Only the first non-empty title found in the tail (most recent
        // chronologically) wins; empty entries are skipped.
        if !tail_found_title && value.get("type").and_then(Value::as_str) == Some("custom-title") {
            let new_title = value
                .get("customTitle")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if new_title.is_some() {
                custom_title = new_title;
                tail_found_title = true;
            }
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
        // No early break — tail_n is only 30 lines and we must scan all
        // of them in case a custom-title rename falls behind the most
        // recent last_active_at / summary lines.
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
        resume_command: Some(format!("claude --resume {session_id}")),
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

/// Returns true when `component` is a single safe filesystem name — no path
/// separators, not `.` or `..`, and not empty.  Used to guard against path-
/// traversal when joining a caller-supplied session id onto a directory.
fn is_safe_path_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && !component.contains('/')
        && !component.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_main_file_and_sidecar_directory() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("abc123-session.jsonl");
        let sidecar = temp.path().join("abc123-session");
        let subagents = sidecar.join("subagents");
        let tool_results = sidecar.join("tool-results");

        std::fs::create_dir_all(&subagents).expect("create subagents");
        std::fs::create_dir_all(&tool_results).expect("create tool-results");
        std::fs::write(subagents.join("agent-1.jsonl"), "{}").expect("write subagent");
        std::fs::write(tool_results.join("tool-1.txt"), "result").expect("write tool result");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-123\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write session");

        delete_session(temp.path(), &path, "session-123").expect("delete session");

        assert!(!path.exists());
        assert!(!sidecar.exists());
    }

    #[test]
    fn load_messages_tool_use_shows_as_assistant() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"a.txt\"}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"File written\"}]},\"timestamp\":\"2026-03-06T10:00:01Z\"}\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: Write]"));
        assert_eq!(msgs[1].role, "tool");
        assert_eq!(msgs[1].content, "File written");
    }

    #[test]
    fn load_messages_mixed_text_and_tool_use() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Let me help.\"},{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Read\",\"input\":{}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("Let me help."));
        assert!(msgs[0].content.contains("[Tool: Read]"));
    }

    #[test]
    fn load_messages_mixed_user_tool_result_and_text_stays_user() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"result\"},{\"type\":\"text\",\"text\":\"Please continue\"}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert!(msgs[0].content.contains("Please continue"));
    }

    #[test]
    fn parse_session_uses_first_user_message_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-abc.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"How do I deploy?\"},\"sessionId\":\"session-abc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Here is how...\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("How do I deploy?"));
    }

    #[test]
    fn parse_session_custom_title_overrides_first_message() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-def.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-def\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"fix something\"},\"sessionId\":\"session-def\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"fix-login-bug\",\"sessionId\":\"session-def\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("fix-login-bug"));
    }

    #[test]
    fn parse_session_falls_back_to_dir_basename() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-ghi.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-ghi\",\"cwd\":\"/tmp/my-project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // No user message and no custom-title → falls back to dir basename
        assert_eq!(meta.title.as_deref(), Some("my-project"));
    }

    #[test]
    fn parse_session_truncates_long_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-trunc.jsonl");
        let long_msg = "a".repeat(200);
        std::fs::write(
            &path,
            format!(
                "{{\"sessionId\":\"session-trunc\",\"cwd\":\"/tmp/p\",\"timestamp\":\"2026-03-06T10:00:00Z\"}}\n\
                 {{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{long_msg}\"}},\"sessionId\":\"session-trunc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        let title = meta.title.unwrap();
        assert!(title.len() <= TITLE_MAX_CHARS + 3); // +3 for "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn parse_session_new_format_with_snapshot() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-new.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"请帮我重构这个函数\"},\"sessionId\":\"session-new\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"OK\"},\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("请帮我重构这个函数"));
    }

    #[test]
    fn parse_session_skips_command_caveat_and_slash_commands() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-clear.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\\n<command-message>clear</command-message>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:01Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:00:02Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"帮我看看工作区的改动\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("帮我看看工作区的改动"));
    }

    #[test]
    fn parse_session_custom_title_in_head_region_of_large_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-head-custom.jsonl");

        // custom-title in the head region (line 2, within first 10 lines),
        // then enough padding to push the file past TAIL_WINDOW_BYTES (128 KB)
        // so that read_head_tail_lines exercises its seek-and-read-tail path
        // rather than the full-file-read path.
        let header = concat!(
            "{\"sessionId\":\"session-big\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"my-rename\",\"sessionId\":\"session-big\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"sessionId\":\"session-big\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
        );
        let padding_line = concat!(
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",",
            "\"content\":\"padding padding padding padding padding padding ",
            "padding padding padding padding padding\"},",
            "\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
        );
        // Generate > 128 KB so the seek path is taken (TAIL_WINDOW_BYTES = 131_072)
        let needed = (140_000 - header.len()) / padding_line.len() + 5;
        let padding: String = std::iter::repeat_n(padding_line, needed)
            .collect::<Vec<_>>()
            .concat();

        std::fs::write(&path, format!("{}{}", header, padding)).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("my-rename"));
    }

    #[test]
    fn parse_session_custom_title_beyond_old_tail_window() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-far-custom.jsonl");

        // A stubby line repeated to bulk the file past the old 16 KB threshold
        // while keeping line count low enough for the custom-title line to
        // remain within the tail_n=30 window of read_head_tail_lines.
        // Custom-title sits ~20 KB from EOF so the old 16 KB tail window
        // would miss it — but the new 128 KB window (or full-file read for
        // files under the threshold) catches it.
        let bulk = "x".repeat(999); // ~1 KB per line
        let bulk_line = |i: usize| -> String {
            format!("{{\"type\":\"bulk\",\"i\":{i},\"pad\":\"{bulk}\"}}\n",)
        };

        let mut file = String::new();
        file.push_str(concat!(
            "{\"sessionId\":\"session-far\",\"cwd\":\"/tmp/project\",",
            "\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",",
            "\"content\":\"start\"},\"sessionId\":\"session-far\",",
            "\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
        ));
        // ~20 bulk lines @ ~1 KB each → ~20 KB before the custom-title
        for i in 0..20 {
            file.push_str(&bulk_line(i));
        }
        file.push_str(concat!(
            "{\"type\":\"custom-title\",\"customTitle\":\"late-rename\",",
            "\"sessionId\":\"session-far\"}\n",
        ));
        // 20 bulk lines after custom-title (~20 KB) so the old 16 KB
        // seek window would land past it. File stays < 128 KB so
        // read_head_tail_lines takes the full-file path.
        for i in 20..40 {
            file.push_str(&bulk_line(i));
        }

        std::fs::write(&path, file).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("late-rename"));
    }

    #[test]
    fn parse_session_tail_custom_title_overrides_head() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-tail-override.jsonl");

        // Build a ~50-line file so head (first 10) and tail (last 30) are
        // disjoint.  Head has an early rename at line 3; tail has a later
        // rename at line 42 (within the last 30).  The tail rename must win.
        let mut file = String::new();
        file.push_str(concat!(
            "{\"sessionId\":\"s\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"early-rename\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start\"},\"sessionId\":\"s\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
        ));
        // ~40 padding lines between head and the late rename
        for i in 0..38 {
            file.push_str(&format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"pad {i}\"}},\"sessionId\":\"s\",\"timestamp\":\"2026-03-06T10:{:02}:00Z\"}}\n",
                (i + 2) % 60
            ));
        }
        // Late rename — chronologically after the early one, within tail window
        file.push_str(
            "{\"type\":\"custom-title\",\"customTitle\":\"late-rename\",\"sessionId\":\"s\"}\n",
        );
        // A few more lines to push it into the tail region
        for i in 38..46 {
            file.push_str(&format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"pad {i}\"}},\"sessionId\":\"s\",\"timestamp\":\"2026-03-06T10:{:02}:00Z\"}}\n",
                (i + 2) % 60
            ));
        }

        std::fs::write(&path, file).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(
            meta.title.as_deref(),
            Some("late-rename"),
            "tail custom-title should override head custom-title"
        );
    }

    #[test]
    fn parse_session_custom_title_in_tail_of_large_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-tail-large.jsonl");

        // custom-title only in the tail region of a file > 128 KB so that
        // read_head_tail_lines uses the seek path to read the tail window.
        let bulk = "x".repeat(999); // ~1 KB per line
        let bulk_line = |i: usize| -> String {
            format!("{{\"type\":\"bulk\",\"i\":{i},\"pad\":\"{bulk}\"}}\n")
        };

        let mut file = String::new();
        file.push_str(concat!(
            "{\"sessionId\":\"tail-big\",\"cwd\":\"/tmp/project\",",
            "\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",",
            "\"content\":\"start\"},\"sessionId\":\"tail-big\",",
            "\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
        ));
        // ~135 KB of bulk lines, then custom-title, then ~5 KB more
        for i in 0..135 {
            file.push_str(&bulk_line(i));
        }
        file.push_str(concat!(
            "{\"type\":\"custom-title\",\"customTitle\":\"tail-only-rename\",",
            "\"sessionId\":\"tail-big\"}\n",
        ));
        for i in 135..140 {
            file.push_str(&bulk_line(i));
        }

        std::fs::write(&path, file).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(
            meta.title.as_deref(),
            Some("tail-only-rename"),
            "custom-title in tail of >128KB file (seek path) should be found"
        );
    }

    #[test]
    #[serial]
    fn delete_session_cleans_up_jobs_directory() {
        let temp = tempdir().expect("tempdir");
        let sessions_dir = temp.path().join("projects");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        // Redirect Claude config dir under temp via CC_SWITCH_TEST_HOME so
        // that get_claude_config_dir() returns temp/.claude.
        let original = std::env::var_os("CC_SWITCH_TEST_HOME");
        unsafe {
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        }

        // Lay out ~/.claude/jobs/{session_id}/ and ~/.claude/jobs/{session_id}.json
        let jobs_dir = temp.path().join(".claude").join("jobs");
        let jobs_subdir = jobs_dir.join("test-session-jobs");
        std::fs::create_dir_all(&jobs_subdir).expect("create jobs subdir");
        std::fs::write(jobs_subdir.join("state.json"), "{}").expect("write state.json");
        let jobs_file = jobs_dir.join("test-session-jobs.json");
        std::fs::write(&jobs_file, "{}").expect("write jobs file");

        // Create session JSONL
        let path = sessions_dir.join("test-session-jobs.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"test-session-jobs\",\"cwd\":\"/tmp/project\",",
                "\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},",
                "\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write session");

        delete_session(&sessions_dir, &path, "test-session-jobs").expect("delete session");

        assert!(!path.exists(), "session JSONL should be deleted");
        assert!(!jobs_subdir.exists(), "jobs subdirectory should be deleted");
        assert!(!jobs_file.exists(), "jobs JSON file should be deleted");

        // Restore env
        match original {
            Some(v) => unsafe { std::env::set_var("CC_SWITCH_TEST_HOME", v) },
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }

    #[test]
    fn is_safe_path_component_accepts_normal_names() {
        assert!(is_safe_path_component("abc"));
        assert!(is_safe_path_component("session-12345"));
        assert!(is_safe_path_component(
            "48ed2288-f025-4d54-8b69-10b40d97b006"
        ));
        assert!(is_safe_path_component("a"));
    }

    #[test]
    fn is_safe_path_component_rejects_traversal() {
        assert!(!is_safe_path_component(""));
        assert!(!is_safe_path_component("."));
        assert!(!is_safe_path_component(".."));
        assert!(!is_safe_path_component("../etc"));
        assert!(!is_safe_path_component("a/b"));
        assert!(!is_safe_path_component("c:\\windows"));
        assert!(!is_safe_path_component("/etc/passwd"));
    }

    #[test]
    fn parse_session_multiple_renames_in_head_uses_latest() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-multi-rename.jsonl");

        // Two custom-title entries in the head region (lines 2 and 4);
        // line 4 is chronologically later so its title should win.
        let file = concat!(
            "{\"sessionId\":\"session-renamed\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"old-name\",\"sessionId\":\"session-renamed\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"sessionId\":\"session-renamed\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"new-name\",\"sessionId\":\"session-renamed\"}\n",
        );

        std::fs::write(&path, file).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(
            meta.title.as_deref(),
            Some("new-name"),
            "the most recent custom-title (new-name at line 4) should win"
        );
    }

    #[test]
    fn delete_session_rejects_unsafe_session_id() {
        let temp = tempdir().expect("tempdir");
        let sessions_dir = temp.path().join("projects");
        std::fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        // Create a JSONL whose sessionId contains path traversal.
        // The file itself has a normal name — the traversal lives in
        // the JSONL content.
        let path = sessions_dir.join("safe-name.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"../../etc\",\"cwd\":\"/tmp/project\",",
                "\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},",
                "\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write session");

        let result = delete_session(&sessions_dir, &path, "../../etc");
        assert!(
            result.is_err(),
            "should reject session_id with path traversal"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("unsafe"),
            "error should mention unsafe session ID, got: {err}"
        );
    }

    #[test]
    fn parse_session_empty_custom_title_does_not_clear_previous() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-empty-rename.jsonl");

        // custom-title at line 2 with valid name, then another custom-title
        // at line 4 with an empty/whitespace title.  The empty one must NOT
        // overwrite the valid one.
        let file = concat!(
            "{\"sessionId\":\"session-empty\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"valid-name\",\"sessionId\":\"session-empty\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"sessionId\":\"session-empty\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"   \",\"sessionId\":\"session-empty\"}\n",
        );

        std::fs::write(&path, file).expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(
            meta.title.as_deref(),
            Some("valid-name"),
            "empty custom-title must not overwrite a previously valid one"
        );
    }
}
