//! Oh My Pi session discovery (JSONL transcripts under `~/.omp/agent/sessions/`).
//!
//! Oh My Pi is a Pi descendant: sessions are JSONL files named `<session-id>.jsonl`
//! with entries of `{"type":"message","message":{"role":...,"content":...},...}`.
//! The exact field set is finalized against the local Oh My Pi source during apply;
//! this provider consumes the stable envelope (filename id + message role/content).

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, truncate_summary, TITLE_MAX_CHARS,
};
use crate::session_manager::{SessionMessage, SessionMeta};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "ohmypi";
const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;

pub fn session_roots() -> Vec<PathBuf> {
    match crate::ohmypi_config::get_ohmypi_agent_dir() {
        Ok(dir) => vec![dir.join("sessions")],
        Err(error) => {
            log::warn!("Oh My Pi session root unavailable: {error}");
            Vec::new()
        }
    }
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    for root in session_roots() {
        collect_sessions_in_root(&root, &mut sessions);
    }
    sessions
}

fn collect_sessions_in_root(root: &Path, output: &mut Vec<SessionMeta>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sessions_in_root(&path, output);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Ok(meta) = parse_session(&path) {
                output.push(meta);
            }
        }
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    validate_file_size(path)?;
    let file = fs::File::open(path)
        .map_err(|error| format!("Failed to open Oh My Pi session: {error}"))?;
    let mut messages = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("Failed to read Oh My Pi session: {error}"))?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let Some(message) = value.get("message").and_then(|v| v.as_object()) else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let content = message.get("content").map(extract_text).unwrap_or_default();
        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }
    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Oh My Pi session: {error}"))?;
    if !source.starts_with(root) {
        return Err("Oh My Pi session source is outside the session root".to_string());
    }
    let file_id = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    // File names are `<timestamp>_<uuid>.jsonl`; match on the uuid suffix.
    let id_matches = file_id == session_id
        || file_id
            .rsplit_once('_')
            .map(|(_, suffix)| suffix == session_id)
            .unwrap_or(false);
    if !id_matches {
        return Err(format!(
            "Oh My Pi session ID mismatch: expected {session_id}, found {file_id}"
        ));
    }
    fs::remove_file(&source)
        .map_err(|error| format!("Failed to delete Oh My Pi session: {error}"))?;
    Ok(true)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Oh My Pi session: {error}"))?;
    let source_path = source
        .to_str()
        .ok_or_else(|| "Oh My Pi session path is not valid UTF-8".to_string())?
        .to_string();
    // Fallback id: derived from the file stem (`<timestamp>_<uuid>.jsonl`),
    // overridden by the real `id` from the `{"type":"session",...}` header line.
    // The stem is also omp's resume key: `--resume` matches the
    // `<timestamp>_<uuid>` filename prefix (session-listing.ts
    // sessionMatchesResumeArg), so it — not the header id/path — must back the
    // resume command.
    let file_stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Oh My Pi session has no id".to_string())?
        .to_string();
    let mut session_id = file_stem.clone();

    validate_file_size(&source)?;
    let file = fs::File::open(&source)
        .map_err(|error| format!("Failed to open Oh My Pi session: {error}"))?;

    let mut created_at: Option<i64> = None;
    let mut last_active_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;
    let mut last_message: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut session_title: Option<String> = None;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
            if created_at.is_none() {
                created_at = Some(ts);
            }
            last_active_at = Some(ts);
        }
        match value.get("type").and_then(|v| v.as_str()) {
            Some("session") => {
                if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
                    let id = id.trim();
                    if !id.is_empty() {
                        session_id = id.to_string();
                    }
                }
            }
            Some("title") if session_title.is_none() => {
                if let Some(t) = value.get("title").and_then(|v| v.as_str()) {
                    if !t.is_empty() {
                        session_title = Some(t.to_string());
                    }
                }
            }
            _ => {}
        }
        if value.get("type").and_then(|v| v.as_str()) == Some("message") {
            let Some(message) = value.get("message").and_then(|v| v.as_object()) else {
                continue;
            };
            let role = message.get("role").and_then(|v| v.as_str());
            let text = message.get("content").map(extract_text).unwrap_or_default();
            if role == Some("user") && first_user_message.is_none() {
                first_user_message = Some(text.clone());
            }
            if !text.trim().is_empty() {
                last_message = Some(text);
            }
        }
        if let Some(cwd_value) = value.get("cwd").and_then(|v| v.as_str()) {
            if cwd.is_none() && !cwd_value.is_empty() {
                cwd = Some(cwd_value.to_string());
            }
        }
    }

    let title = session_title
        .as_deref()
        .map(|t| truncate_summary(t, TITLE_MAX_CHARS))
        .filter(|t| !t.is_empty())
        .or_else(|| {
            first_user_message
                .as_deref()
                .map(|message| truncate_summary(message, TITLE_MAX_CHARS))
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    cwd.as_deref()
                        .and_then(path_basename)
                        .filter(|s| !s.is_empty())
                })
        });

    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title,
        summary: last_message.map(|m| truncate_summary(&m, 160)),
        project_dir: cwd,
        created_at,
        last_active_at,
        source_path: Some(source_path.clone()),
        resume_command: Some(format!("omp --resume {file_stem}")),
    })
}

fn validate_file_size(path: &Path) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("Failed to stat Oh My Pi session: {error}"))?;
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(format!(
            "Oh My Pi session exceeds the 128 MiB limit: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohmypi_config::test_support::TestAgentDir;
    use serial_test::serial;

    fn write_session_jsonl(dir: &Path, id: &str) {
        fs::create_dir_all(dir).expect("create sessions dir");
        fs::write(
            dir.join(format!("{id}.jsonl")),
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "type": "message",
                    "message": {"role": "user", "content": [{"type": "text", "text": "hello"}], "id": "u1"},
                    "timestamp": 1700000000000i64
                }),
                serde_json::json!({
                    "type": "message",
                    "message": {"role": "assistant", "content": [{"type": "text", "text": "hi there"}], "id": "a1"},
                    "timestamp": 1700000001000i64
                })
            ),
        )
        .expect("write session");
    }

    fn write_session_file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent dir");
        fs::write(path, content).expect("write session");
    }

    #[test]
    #[serial]
    fn scans_nested_jsonl_sessions_recursively() {
        let _agent = TestAgentDir::new();
        let dir = session_roots().first().expect("session root").clone();
        // Real Oh My Pi layout: <branch-dir>/<timestamp>_<uuid>.jsonl.
        let nested = dir
            .join("branch-main")
            .join("2026-08-08T13-49-08-014Z_019fe1a2-0000.jsonl");
        write_session_file(
            &nested,
            &format!(
                "{}\n{}\n",
                serde_json::json!({
                    "type": "session",
                    "version": 1,
                    "id": "019fe1a2-0000",
                    "cwd": "/home/user/project"
                }),
                serde_json::json!({
                    "type": "message",
                    "message": {"role": "user", "content": [{"type": "text", "text": "nested hello"}], "id": "u1"},
                    "timestamp": 1700000000000i64
                })
            ),
        );

        let sessions = scan_sessions();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, "019fe1a2-0000");
        assert_eq!(session.project_dir.as_deref(), Some("/home/user/project"));
    }

    #[test]
    #[serial]
    fn parses_real_session_id_cwd_and_title_from_header_lines() {
        let _agent = TestAgentDir::new();
        let dir = session_roots().first().expect("session root").clone();
        let nested = dir
            .join("branch-x")
            .join("2026-08-08T13-49-08-014Z_019fe1a2-1111.jsonl");
        write_session_file(
            &nested,
            &format!(
                "{}\n{}\n{}\n",
                serde_json::json!({
                    "type": "session",
                    "version": 1,
                    "id": "019fe1a2-1111",
                    "cwd": "/repo/backend"
                }),
                serde_json::json!({
                    "type": "title",
                    "v": 1,
                    "title": "Fix the ohmypi scanner"
                }),
                serde_json::json!({
                    "type": "message",
                    "message": {"role": "user", "content": [{"type": "text", "text": "hello"}], "id": "u1"},
                    "timestamp": 1700000000000i64
                })
            ),
        );

        let sessions = scan_sessions();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, "019fe1a2-1111");
        assert_eq!(session.project_dir.as_deref(), Some("/repo/backend"));
        assert_eq!(session.title.as_deref(), Some("Fix the ohmypi scanner"));
        assert_eq!(
            session.resume_command.as_deref(),
            Some("omp --resume 2026-08-08T13-49-08-014Z_019fe1a2-1111")
        );
    }

    #[test]
    #[serial]
    fn deletes_session_matching_uuid_suffix_of_file_stem() {
        let _agent = TestAgentDir::new();
        let dir = session_roots().first().expect("session root").clone();
        fs::create_dir_all(&dir).expect("create sessions dir");
        let root = dir.canonicalize().expect("canonical root");
        let file_path = root.join("2026-08-08T13-49-08-014Z_019fe1a2-2222.jsonl");
        write_session_jsonl(&root, "2026-08-08T13-49-08-014Z_019fe1a2-2222");

        let deleted = delete_session(&root, &file_path, "019fe1a2-2222").expect("delete");
        assert!(deleted);
        assert!(!file_path.exists());
    }
}
