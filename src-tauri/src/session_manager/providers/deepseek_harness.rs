use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, path_basename, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "deepseek-harness";
const SESSION_FILE_NAME: &str = "session.jsonl.zstd";
/// Compressed size guard; decompressed content is bounded by the event limit.
const MAX_SESSION_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 500_000;

pub fn session_roots() -> Vec<PathBuf> {
    vec![crate::deepseek_harness_config::get_dsh_home().join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = crate::deepseek_harness_config::get_dsh_home().join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for project in entries.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(session_dirs) = fs::read_dir(&project_path) else {
            continue;
        };
        for session_dir in session_dirs.flatten() {
            let file = session_dir.path().join(SESSION_FILE_NAME);
            if !file.is_file() {
                continue;
            }
            match parse_session(&file) {
                Ok(meta) => sessions.push(meta),
                Err(error) => {
                    log::debug!("Skipping invalid DSH session {}: {error}", file.display())
                }
            }
        }
    }
    sessions
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let events = read_events(path)?;
    let mut messages = Vec::new();
    for value in &events {
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if !content.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "user".to_string(),
                        content,
                        ts: event_time(value),
                    });
                }
            }
            Some("assistant/message") => {
                let content = assistant_text(value);
                if !content.trim().is_empty() {
                    messages.push(SessionMessage {
                        role: "assistant".to_string(),
                        content,
                        ts: event_time(value),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if session_id.contains(['/', '\\']) || session_id == "." || session_id == ".." {
        return Err("Invalid DSH session ID".to_string());
    }
    if path.file_name().and_then(|n| n.to_str()) != Some(SESSION_FILE_NAME) {
        return Err(format!(
            "DSH session source must be a {SESSION_FILE_NAME} file: {}",
            path.display()
        ));
    }
    let meta = parse_session(path)?;
    if meta.session_id != session_id {
        return Err(format!(
            "DSH session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }
    let dir = path
        .parent()
        .ok_or_else(|| "DSH session has no parent directory".to_string())?;
    if !dir.starts_with(root) {
        return Err("DSH session is outside the sessions root".to_string());
    }
    fs::remove_dir_all(dir)
        .map_err(|error| format!("Failed to delete DSH session {}: {error}", dir.display()))?;
    Ok(true)
}

fn event_time(value: &Value) -> Option<i64> {
    value
        .get("time")
        .or_else(|| value.get("time0"))
        .and_then(Value::as_i64)
}

fn assistant_text(value: &Value) -> String {
    value
        .pointer("/data/message/content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn read_events(path: &Path) -> Result<Vec<Value>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to stat DSH session {}: {error}", path.display()))?;
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(format!(
            "DSH session exceeds the {MAX_SESSION_BYTES}-byte safety limit"
        ));
    }
    let file = File::open(path).map_err(|error| format!("Failed to open DSH session: {error}"))?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .map_err(|error| format!("Failed to decompress DSH session: {error}"))?;
    let reader = BufReader::new(decoder);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read DSH session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if events.len() >= MAX_EVENTS {
            return Err(format!(
                "DSH session exceeds the {MAX_EVENTS}-event safety limit"
            ));
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            events.push(value);
        }
    }
    Ok(events)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve DSH session {}: {error}", path.display()))?;
    let events = read_events(&source)?;
    let header = events
        .iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session"))
        .ok_or_else(|| "DSH session has no header".to_string())?;
    let id = header
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH session header has no id".to_string())?
        .to_string();
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = header.get("createdAt").and_then(Value::as_i64);

    let mut title = events
        .iter()
        .rev()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session/title"))
        .and_then(|value| value.pointer("/data/title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut first_user: Option<String> = None;
    let mut last_text: Option<String> = None;
    let mut last_active_at = created_at;
    for value in &events {
        if let Some(time) = event_time(value) {
            last_active_at = Some(last_active_at.map_or(time, |current| current.max(time)));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("user/message") => {
                let content = value
                    .pointer("/data/content")
                    .map(extract_text)
                    .unwrap_or_default();
                if first_user.is_none() && !content.trim().is_empty() {
                    first_user = Some(content.clone());
                }
                if !content.trim().is_empty() {
                    last_text = Some(content);
                }
            }
            Some("assistant/message") => {
                let text = assistant_text(value);
                if !text.trim().is_empty() {
                    last_text = Some(text);
                }
            }
            _ => {}
        }
    }
    if title.is_none() {
        title = first_user
            .as_deref()
            .map(|message| truncate_summary(message, TITLE_MAX_CHARS))
            .filter(|message| !message.is_empty())
            .or_else(|| cwd.as_deref().and_then(path_basename));
    }
    let summary = last_text
        .as_deref()
        .map(|message| truncate_summary(message, 160))
        .filter(|message| !message.is_empty());
    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: id,
        title,
        summary,
        project_dir: cwd.filter(|value| !value.trim().is_empty()),
        created_at,
        last_active_at,
        source_path: source.to_str().map(str::to_string),
        resume_command: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn write_session(home: &Path, project: &str, id: &str, events: &[Value]) -> PathBuf {
        let dir = home.join("sessions").join(project).join(id);
        fs::create_dir_all(&dir).unwrap();
        let payload = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let compressed = zstd::stream::encode_all(payload.as_bytes(), 3).unwrap();
        let file = dir.join(SESSION_FILE_NAME);
        fs::write(&file, compressed).unwrap();
        file
    }

    fn fixture_events() -> Vec<Value> {
        vec![
            serde_json::json!({"type":"session","version":0,"id":"session-abc","createdAt":1787104521333_i64,"cwd":"/Users/test/project"}),
            serde_json::json!({"type":"user/message","seq":8,"time":1787220655606_i64,"data":{"role":"user","content":[{"type":"text","text":"hello dsh"}]}}),
            serde_json::json!({"type":"session/title","seq":11,"time":1787220655607_i64,"data":{"title":"Greeting"}}),
            serde_json::json!({"type":"text-chunks","seq0":69,"time0":1787220662295_i64,"data":{"turn":1,"step":1,"index":1,"texts":["stream"," delta"]}}),
            serde_json::json!({"type":"assistant/message","seq":352,"time":1787220668317_i64,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[{"type":"reasoning","text":"thinking"},{"type":"text","text":"hi there"}]}}}),
        ]
    }

    fn with_temp_home(test: impl FnOnce(&Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    #[test]
    #[serial]
    fn scan_reads_zstd_sessions_with_titles() {
        with_temp_home(|home| {
            let file = write_session(
                home,
                "--Users-test-project--",
                "session-abc",
                &fixture_events(),
            );
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
            let meta = &sessions[0];
            assert_eq!(meta.provider_id, "deepseek-harness");
            assert_eq!(meta.session_id, "session-abc");
            assert_eq!(meta.title.as_deref(), Some("Greeting"));
            assert_eq!(meta.project_dir.as_deref(), Some("/Users/test/project"));
            assert_eq!(meta.created_at, Some(1787104521333));
            assert_eq!(meta.last_active_at, Some(1787220668317));
            assert_eq!(
                meta.source_path.as_deref(),
                file.canonicalize().unwrap().to_str()
            );
        });
    }

    #[test]
    #[serial]
    fn load_messages_skips_stream_deltas_and_reasoning() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let messages = load_messages(&file).unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].role, "user");
            assert_eq!(messages[0].content, "hello dsh");
            assert_eq!(messages[0].ts, Some(1787220655606));
            assert_eq!(messages[1].role, "assistant");
            assert_eq!(messages[1].content, "hi there");
        });
    }

    #[test]
    #[serial]
    fn corrupt_sessions_are_skipped_not_fatal() {
        with_temp_home(|home| {
            let dir = home.join("sessions").join("proj").join("session-bad");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(SESSION_FILE_NAME), b"not zstd at all").unwrap();
            write_session(home, "proj", "session-abc", &fixture_events());
            let sessions = scan_sessions();
            assert_eq!(sessions.len(), 1);
        });
    }

    #[test]
    #[serial]
    fn delete_removes_the_session_directory() {
        with_temp_home(|home| {
            let file = write_session(home, "proj", "session-abc", &fixture_events());
            let root = home.join("sessions");
            let deleted = delete_session(&root, &file, "session-abc").unwrap();
            assert!(deleted);
            assert!(!file.exists());
            assert!(!file.parent().unwrap().exists());
            assert!(delete_session(&root, &file, "wrong-id").is_err());
        });
    }
}
