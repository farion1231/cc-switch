use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{extract_text, parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "copilot-cli";
const MAX_SESSION_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EVENTS: usize = 100_000;
const MAX_DELETE_ENTRIES: usize = 10_000;

pub fn session_roots() -> Vec<PathBuf> {
    crate::copilot_byok::copilot_cli_home()
        .map(|home| vec![home.join("session-state")])
        .unwrap_or_default()
}

pub fn session_files() -> Vec<PathBuf> {
    session_roots()
        .into_iter()
        .flat_map(|root| session_files_in_root(&root))
        .collect()
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    session_files()
        .into_iter()
        .filter_map(|path| parse_session_meta(&path))
        .collect()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    visit_events(path, |event| {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let role = match event_type {
            "user.message" => "user",
            "assistant.message" => "assistant",
            _ => return,
        };
        let data = event.get("data").unwrap_or(event);
        let content = data.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            return;
        }
        messages.push(SessionMessage {
            role: role.to_string(),
            content,
            ts: event_timestamp(event),
        });
    })?;
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if path.file_name().and_then(|name| name.to_str()) != Some("events.jsonl") {
        return Err(format!(
            "Unexpected Copilot CLI session source: {}",
            path.display()
        ));
    }
    let session_dir = path
        .parent()
        .ok_or_else(|| format!("Invalid Copilot CLI session path: {}", path.display()))?;
    if session_dir.parent() != Some(root) || !session_dir.starts_with(root) {
        return Err(format!(
            "Copilot CLI session source is outside its session root: {}",
            path.display()
        ));
    }
    if session_dir.file_name().and_then(|name| name.to_str()) != Some(session_id) {
        return Err(format!(
            "Copilot CLI session directory does not match session ID: {}",
            session_dir.display()
        ));
    }
    let actual_id = read_session_identity(path).unwrap_or_else(|| session_id.to_string());
    if actual_id != session_id {
        return Err(format!(
            "Copilot CLI session ID mismatch: expected {session_id}, found {actual_id}"
        ));
    }
    validate_delete_tree(session_dir)?;
    fs::remove_dir_all(session_dir).map_err(|error| {
        format!(
            "Failed to delete Copilot CLI session directory {}: {error}",
            session_dir.display()
        )
    })?;
    Ok(true)
}

fn session_files_in_root(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return None;
            }
            let path = entry.path().join("events.jsonl");
            let metadata = fs::symlink_metadata(&path).ok()?;
            (metadata.is_file() && !metadata.file_type().is_symlink()).then_some(path)
        })
        .collect()
}

fn parse_session_meta(path: &Path) -> Option<SessionMeta> {
    let mut session_id = None;
    let mut first_prompt = None;
    let mut project_dir = None;
    let mut created_at = None;
    let mut last_active_at = None;
    visit_events(path, |event| {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = event.get("data").unwrap_or(event);
        let timestamp = event_timestamp(event);
        if let Some(timestamp) = timestamp {
            last_active_at =
                Some(last_active_at.map_or(timestamp, |current: i64| current.max(timestamp)));
        }
        if event_type == "session.start" {
            session_id = data
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .or(session_id.take());
            created_at = data
                .get("startTime")
                .and_then(parse_timestamp_to_ms)
                .or(timestamp)
                .or(created_at);
            project_dir = data
                .get("context")
                .and_then(|context| context.get("cwd"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .or(project_dir.take());
        } else if event_type == "user.message" && first_prompt.is_none() {
            let text = data.get("content").map(extract_text).unwrap_or_default();
            if !text.trim().is_empty() {
                first_prompt = Some(text);
            }
        }
    })
    .ok()?;

    let session_id = session_id.or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_string)
    })?;
    let modified_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    let title = first_prompt
        .as_deref()
        .map(|value| truncate_summary(value, TITLE_MAX_CHARS));
    let summary = first_prompt
        .as_deref()
        .map(|value| truncate_summary(value, 160));

    let resume_command = session_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .then(|| format!("copilot --resume={session_id}"));

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        created_at,
        last_active_at: last_active_at.or(modified_at).or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command,
    })
}

fn visit_events<F>(path: &Path, mut visitor: F) -> Result<(), String>
where
    F: FnMut(&Value),
{
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect Copilot CLI session: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "Copilot CLI session is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_SESSION_FILE_BYTES {
        return Err(format!(
            "Copilot CLI session exceeds {} MiB: {}",
            MAX_SESSION_FILE_BYTES / 1024 / 1024,
            path.display()
        ));
    }
    let file =
        File::open(path).map_err(|error| format!("Failed to open Copilot CLI session: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut index = 0usize;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| format!("Failed to read Copilot CLI session: {error}"))?;
        if bytes == 0 {
            break;
        }
        if index >= MAX_EVENTS {
            return Err(format!("Copilot CLI session exceeds {MAX_EVENTS} events"));
        }
        index += 1;
        if line.trim().is_empty() {
            continue;
        }
        let terminated = line.ends_with('\n');
        let event: Value = match serde_json::from_str(&line) {
            Ok(event) => event,
            // Copilot CLI can be writing the final JSONL record while CC
            // Switch scans an active session. A malformed earlier record is a
            // real corruption, but an unterminated final record is transient.
            Err(_) if !terminated => break,
            Err(error) => {
                return Err(format!(
                    "Failed to parse Copilot CLI session {} at line {}: {error}",
                    path.display(),
                    index
                ))
            }
        };
        visitor(&event);
    }
    Ok(())
}

fn read_session_identity(path: &Path) -> Option<String> {
    let mut identity = None;
    visit_events(path, |event| {
        if identity.is_some() || event.get("type").and_then(Value::as_str) != Some("session.start")
        {
            return;
        }
        identity = event
            .get("data")
            .and_then(|data| data.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_string);
    })
    .ok()?;
    identity
}

fn event_timestamp(event: &Value) -> Option<i64> {
    event
        .get("timestamp")
        .or_else(|| event.get("ts"))
        .and_then(parse_timestamp_to_ms)
        .or_else(|| {
            event
                .get("data")
                .and_then(|data| data.get("timestamp"))
                .and_then(parse_timestamp_to_ms)
        })
}

fn validate_delete_tree(root: &Path) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];
    let mut seen = 0usize;
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)
            .map_err(|error| format!("Failed to inspect Copilot CLI session directory: {error}"))?
        {
            let entry = entry.map_err(|error| {
                format!("Failed to inspect Copilot CLI session directory entry: {error}")
            })?;
            seen += 1;
            if seen > MAX_DELETE_ENTRIES {
                return Err(format!(
                    "Copilot CLI session directory exceeds {MAX_DELETE_ENTRIES} entries"
                ));
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect Copilot CLI session entry: {error}"))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "Refusing to delete Copilot CLI session containing a symlink: {}",
                    entry.path().display()
                ));
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages_metadata_and_resume_command() {
        let temp = tempfile::tempdir().expect("temp directory");
        let session_id = "session-1";
        let session_dir = temp.path().join(session_id);
        fs::create_dir_all(&session_dir).expect("session directory");
        let path = session_dir.join("events.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"type":"session.start","timestamp":"2026-08-18T01:00:00Z","data":{"sessionId":"session-1","context":{"cwd":"/work"}}}"#,
                "\n",
                r#"{"type":"user.message","timestamp":"2026-08-18T01:00:01Z","data":{"content":"hello"}}"#,
                "\n",
                r#"{"type":"assistant.message","timestamp":"2026-08-18T01:00:02Z","data":{"content":"world"}}"#,
                "\n",
            ),
        )
        .expect("session file");

        let meta = parse_session_meta(&path).expect("metadata");
        assert_eq!(meta.provider_id, PROVIDER_ID);
        assert_eq!(meta.title.as_deref(), Some("hello"));
        assert_eq!(meta.project_dir.as_deref(), Some("/work"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("copilot --resume=session-1")
        );
        let messages = load_messages(&path).expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "world");
    }

    #[test]
    fn ignores_an_incomplete_final_event_from_an_active_session() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("events.jsonl");
        fs::write(
            &path,
            concat!(
                r#"{"type":"user.message","data":{"content":"complete"}}"#,
                "\n",
                r#"{"type":"assistant.message","data":{"content":"partial"}"#,
            ),
        )
        .expect("session file");

        let messages = load_messages(&path).expect("active session remains readable");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "complete");
    }

    #[test]
    fn unsafe_session_identity_is_never_embedded_in_a_shell_command() {
        let temp = tempfile::tempdir().expect("temp directory");
        let session_dir = temp.path().join("session-1");
        fs::create_dir_all(&session_dir).expect("session directory");
        let path = session_dir.join("events.jsonl");
        fs::write(
            &path,
            r#"{"type":"session.start","data":{"sessionId":"$(touch injected)"}}"#,
        )
        .expect("session file");

        let meta = parse_session_meta(&path).expect("metadata remains visible");
        assert!(meta.resume_command.is_none());
    }
}
