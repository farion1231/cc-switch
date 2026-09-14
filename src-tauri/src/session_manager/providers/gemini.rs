use std::path::Path;

use serde_json::Value;

use crate::gemini_session::{self, GeminiSessionFile};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, truncate_summary};

const PROVIDER_ID: &str = "gemini";

pub fn scan_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Iterate over project directories: tmp/<project_name>/chats/session-*.{json,jsonl}
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

        let project_root_file = entry.path().join(".project_root");
        let project_dir = std::fs::read_to_string(project_root_file).ok();

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if !gemini_session::has_session_extension(&path) {
                continue;
            }
            if let Some(meta) = parse_session(&path) {
                sessions.push(SessionMeta {
                    project_dir: project_dir.clone(),
                    ..meta
                });
            }
        }
    }

    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let session = gemini_session::load(path)?;

    let mut result = Vec::new();
    for msg in &session.messages {
        let role = match msg.get("type").and_then(Value::as_str) {
            Some("gemini") => "assistant",
            Some("user") => "user",
            Some("info") | Some("error") => continue,
            Some(_) | None => continue,
        };

        // Gemini content may be a plain string or an array of {text: ...} objects
        let mut content = gemini_session::message_text(msg).unwrap_or_default();

        // Append tool call names from the optional toolCalls array
        if let Some(Value::Array(calls)) = msg.get("toolCalls") {
            for call in calls {
                if let Some(name) = call.get("name").and_then(Value::as_str) {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&format!("[Tool: {name}]"));
                }
            }
        }

        if content.trim().is_empty() {
            continue;
        }

        let ts = msg.get("timestamp").and_then(parse_timestamp_to_ms);

        result.push(SessionMessage {
            role: role.to_string(),
            content,
            ts,
        });
    }

    Ok(result)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Gemini session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Gemini session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Gemini session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let session: GeminiSessionFile = gemini_session::load(path).ok()?;

    let session_id = session.session_id.clone()?;

    let created_at = session.start_time.as_ref().and_then(parse_timestamp_to_ms);
    let last_active_at = session
        .last_updated
        .as_ref()
        .and_then(parse_timestamp_to_ms);

    // Derive title from first user message
    let title = session
        .first_user_text()
        .map(|text| truncate_summary(&text, 160));

    let source_path = path.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None, // (optionally) populated later
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(source_path),
        resume_command: Some(format!("gemini --resume {session_id}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_json_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-03-06T10-17-test.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "gemini-session-123",
              "startTime": "2026-03-06T10:17:58.000Z",
              "lastUpdated": "2026-03-06T10:20:00.000Z",
              "messages": [
                {
                  "id": "msg-1",
                  "timestamp": "2026-03-06T10:17:58.000Z",
                  "type": "user",
                  "content": "hello"
                }
              ]
            }"#,
        )
        .expect("write session");

        delete_session(temp.path(), &path, "gemini-session-123").expect("delete session");

        assert!(!path.exists());
    }

    #[test]
    fn load_messages_handles_array_content() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-06T10:00:00Z","type":"user","content":[{"text":"hello"}]},
                {"id":"2","timestamp":"2026-03-06T10:00:01Z","type":"gemini","content":"world"},
                {"id":"3","timestamp":"2026-03-06T10:00:02Z","type":"info","content":"system info"},
                {"id":"4","timestamp":"2026-03-06T10:00:03Z","type":"error","content":"MCP ERROR"}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "world");
    }

    #[test]
    fn load_messages_reads_incremental_jsonl() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-09-14T03-16-abcd1234.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"sessionId":"s-1","startTime":"2026-09-14T03:16:00.000Z","kind":"main"}"#,
                "\n",
                r#"{"$set":{"messages":[{"id":"m1","type":"user","content":[{"text":"hello"}]}]}}"#,
                "\n",
                r#"{"id":"m2","type":"gemini","content":"world"}"#,
                "\n",
                r#"{"id":"m2","type":"gemini","content":"world","toolCalls":[{"name":"web_search"}]}"#,
                "\n",
                r#"{"id":"m3","type":"info","content":"system info"}"#,
                "\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2, "同 id 重写不应产生重复消息");
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("world"));
        assert!(msgs[1].content.contains("[Tool: web_search]"));
    }

    #[test]
    fn parse_session_reads_jsonl_metadata() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-09-14T03-16-abcd1234.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"sessionId":"s-2","startTime":"2026-09-14T03:16:00.000Z","lastUpdated":"2026-09-14T03:16:00.000Z","kind":"main"}"#,
                "\n",
                r#"{"id":"m1","type":"user","content":[{"text":"<session_context>ctx</session_context>"}]}"#,
                "\n",
                r#"{"$set":{"lastUpdated":"2026-09-14T03:20:00.000Z"}}"#,
                "\n",
                r#"{"id":"m2","type":"user","content":[{"text":"real question"}]}"#,
                "\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).expect("parse");
        assert_eq!(meta.session_id, "s-2");
        assert_eq!(
            meta.title.as_deref(),
            Some("real question"),
            "标题应跳过 <session_context> 引导块"
        );
        assert!(meta.last_active_at > meta.created_at);
        assert_eq!(meta.resume_command.as_deref(), Some("gemini --resume s-2"));
    }

    #[test]
    fn delete_session_removes_jsonl_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-09-14T03-16-abcd1234.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"sessionId":"s-3","startTime":"2026-09-14T03:16:00.000Z","kind":"main"}"#,
                "\n",
                r#"{"id":"m1","type":"user","content":"hello"}"#,
                "\n",
            ),
        )
        .expect("write");

        delete_session(temp.path(), &path, "s-3").expect("delete session");

        assert!(!path.exists());
    }

    #[test]
    fn load_messages_includes_tool_calls() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"","toolCalls":[{"id":"call_1","name":"web_search","args":{"query":"test"}}]},
                {"id":"2","timestamp":"2026-03-10T08:25:00Z","type":"gemini","content":"Here are the results.","toolCalls":[{"id":"call_2","name":"web_fetch","args":{"url":"http://example.com"}}]}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: web_search]"));
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("Here are the results."));
        assert!(msgs[1].content.contains("[Tool: web_fetch]"));
    }
}
