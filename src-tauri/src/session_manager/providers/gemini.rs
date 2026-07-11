use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, truncate_summary};

const PROVIDER_ID: &str = "gemini";
// Storage invariant: except for `tempmediaStorage` (temporary media cache),
// session directories and `.db`/`.pb` conversation files in these three roots
// use disjoint UUIDs. Cross-root de-duplication below is only defensive for
// transcript metadata and must not be used to infer duplicated usage records.
const ANTIGRAVITY_ROOTS: [&str; 3] = ["antigravity", "antigravity-cli", "antigravity-ide"];

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = scan_gemini_sessions();
    sessions.extend(scan_antigravity_sessions());
    sessions
}

pub fn session_roots() -> Vec<PathBuf> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let mut roots = vec![gemini_dir.join("tmp")];
    roots.extend(ANTIGRAVITY_ROOTS.iter().map(|root| gemini_dir.join(root)));
    roots
}

fn scan_gemini_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Iterate over project directories: tmp/<project_name>/chats/session-*.json
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
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
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
    if is_antigravity_transcript(path) {
        return load_antigravity_messages(path);
    }

    let data = std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    let value: Value =
        serde_json::from_str(&data).map_err(|e| format!("Failed to parse session JSON: {e}"))?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;

    let mut result = Vec::new();
    for msg in messages {
        let role = match msg.get("type").and_then(Value::as_str) {
            Some("gemini") => "assistant",
            Some("user") => "user",
            Some("info") | Some("error") => continue,
            Some(_) | None => continue,
        };

        // Gemini content may be a plain string or an array of {text: ...} objects
        let mut content = match msg.get("content") {
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };

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

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if is_antigravity_transcript(path) {
        return delete_antigravity_session(root, path, session_id);
    }

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
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("sessionId").and_then(Value::as_str)?.to_string();

    let created_at = value.get("startTime").and_then(parse_timestamp_to_ms);
    let last_active_at = value.get("lastUpdated").and_then(parse_timestamp_to_ms);

    // Derive title from first user message
    let title = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|msgs| {
            msgs.iter()
                .find(|m| m.get("type").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("content").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .map(|s| truncate_summary(s, 160))
        });

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

fn scan_antigravity_sessions() -> Vec<SessionMeta> {
    let mut by_id: HashMap<String, SessionMeta> = HashMap::new();
    for root in antigravity_roots() {
        let brain_dir = root.join("brain");
        let entries = match std::fs::read_dir(&brain_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let transcript = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if !transcript.is_file() {
                continue;
            }
            let Some(meta) = parse_antigravity_session(&transcript) else {
                continue;
            };
            let incoming_ts = meta.last_active_at.or(meta.created_at).unwrap_or(0);
            match by_id.get(&meta.session_id) {
                Some(existing)
                    if existing.last_active_at.or(existing.created_at).unwrap_or(0)
                        >= incoming_ts => {}
                _ => {
                    by_id.insert(meta.session_id.clone(), meta);
                }
            }
        }
    }

    by_id.into_values().collect()
}

fn antigravity_roots() -> Vec<PathBuf> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    ANTIGRAVITY_ROOTS
        .iter()
        .map(|root| gemini_dir.join(root))
        .collect()
}

fn is_antigravity_transcript(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("transcript.jsonl")
        && path
            .components()
            .any(|component| component.as_os_str() == ".system_generated")
}

fn antigravity_session_id_from_transcript(path: &Path) -> Option<String> {
    path.parent()?
        .parent()?
        .parent()?
        .file_name()?
        .to_str()
        .map(|value| value.to_string())
}

fn parse_antigravity_session(path: &Path) -> Option<SessionMeta> {
    let session_id = antigravity_session_id_from_transcript(path)?;
    let messages = load_antigravity_messages(path).ok()?;
    let created_at = messages.iter().filter_map(|message| message.ts).min();
    let last_active_at = messages.iter().filter_map(|message| message.ts).max();
    let title = messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| truncate_summary(&message.content, 160));

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None,
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("agy --conversation {session_id}")),
    })
}

fn load_antigravity_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read Antigravity transcript: {e}"))?;
    let mut result = Vec::new();

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let source = value.get("source").and_then(Value::as_str);
        let message_type = value.get("type").and_then(Value::as_str);
        let role = match (source, message_type) {
            (Some("USER_EXPLICIT"), Some("USER_INPUT")) => "user",
            (Some("MODEL"), Some("PLANNER_RESPONSE") | Some("GENERIC")) => "assistant",
            _ => continue,
        };

        let content = clean_antigravity_content(extract_antigravity_content(&value));
        if content.trim().is_empty() {
            continue;
        }

        result.push(SessionMessage {
            role: role.to_string(),
            content,
            ts: value.get("created_at").and_then(parse_timestamp_to_ms),
        });
    }

    Ok(result)
}

fn extract_antigravity_content(value: &Value) -> String {
    match value.get("content") {
        Some(Value::String(text)) => text.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(map)) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn clean_antigravity_content(content: String) -> String {
    let mut cleaned = content;
    if let Some(after) = cleaned.split("<USER_REQUEST>").nth(1) {
        cleaned = after.to_string();
        if let Some((before, _)) = cleaned.split_once("</USER_REQUEST>") {
            cleaned = before.to_string();
        }
    }
    if let Some((before, _)) = cleaned.split_once("<ADDITIONAL_METADATA>") {
        cleaned = before.to_string();
    }
    cleaned.trim().to_string()
}

fn delete_antigravity_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let parsed_id = antigravity_session_id_from_transcript(path).ok_or_else(|| {
        format!(
            "Failed to parse Antigravity session ID from {}",
            path.display()
        )
    })?;
    if parsed_id != session_id {
        return Err(format!(
            "Antigravity session ID mismatch: expected {session_id}, found {parsed_id}"
        ));
    }

    // Delete auxiliary conversation artifacts first and keep the transcript-bearing
    // brain directory as the final discovery entry. If an auxiliary deletion fails,
    // the session remains visible and the user can retry, matching OpenCode's
    // deletion ordering.
    let conversation_base = root.join("conversations").join(session_id);
    for suffix in ["db", "db-shm", "db-wal", "pb"] {
        let file = conversation_base.with_extension(suffix);
        if file.exists() {
            std::fs::remove_file(&file).map_err(|e| {
                format!(
                    "Failed to delete Antigravity conversation file {}: {e}",
                    file.display()
                )
            })?;
        }
    }

    let brain_dir = root.join("brain").join(session_id);
    if brain_dir.exists() {
        std::fs::remove_dir_all(&brain_dir).map_err(|e| {
            format!(
                "Failed to delete Antigravity brain directory {}: {e}",
                brain_dir.display()
            )
        })?;
    }

    Ok(true)
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

    #[test]
    fn clean_antigravity_content_removes_wrappers_and_metadata() {
        let content =
            "<USER_REQUEST>hello</USER_REQUEST>\n<ADDITIONAL_METADATA>secret</ADDITIONAL_METADATA>";
        assert_eq!(clean_antigravity_content(content.to_string()), "hello");

        let content = "visible\n<ADDITIONAL_METADATA>secret</ADDITIONAL_METADATA>";
        assert_eq!(clean_antigravity_content(content.to_string()), "visible");
    }

    #[test]
    fn delete_antigravity_session_keeps_brain_when_conversation_cleanup_fails() {
        let temp = tempdir().expect("tempdir");
        let session_id = "agy-session-123";
        let transcript = temp
            .path()
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().expect("transcript parent"))
            .expect("create brain");
        std::fs::write(&transcript, "{}\n").expect("write transcript");

        // A directory at the expected .db path makes remove_file fail reliably.
        let blocking_db = temp
            .path()
            .join("conversations")
            .join(format!("{session_id}.db"));
        std::fs::create_dir_all(&blocking_db).expect("create blocking db directory");

        delete_antigravity_session(temp.path(), &transcript, session_id)
            .expect_err("conversation cleanup should fail");

        assert!(
            transcript.is_file(),
            "brain transcript must remain retryable"
        );
    }
}
