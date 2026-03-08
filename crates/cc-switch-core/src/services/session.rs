use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{get_claude_config_dir, get_codex_config_dir, get_home_dir};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::openclaw_config::get_openclaw_dir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

pub struct SessionService;

impl SessionService {
    pub fn list_sessions() -> Vec<SessionMeta> {
        let (codex, claude, opencode, openclaw, gemini) = std::thread::scope(|scope| {
            let codex = scope.spawn(scan_codex_sessions);
            let claude = scope.spawn(scan_claude_sessions);
            let opencode = scope.spawn(scan_opencode_sessions);
            let openclaw = scope.spawn(scan_openclaw_sessions);
            let gemini = scope.spawn(scan_gemini_sessions);

            (
                codex.join().unwrap_or_default(),
                claude.join().unwrap_or_default(),
                opencode.join().unwrap_or_default(),
                openclaw.join().unwrap_or_default(),
                gemini.join().unwrap_or_default(),
            )
        });

        let mut sessions = Vec::new();
        sessions.extend(codex);
        sessions.extend(claude);
        sessions.extend(opencode);
        sessions.extend(openclaw);
        sessions.extend(gemini);

        sessions.sort_by(|left, right| {
            let left_ts = left.last_active_at.or(left.created_at).unwrap_or(0);
            let right_ts = right.last_active_at.or(right.created_at).unwrap_or(0);
            right_ts.cmp(&left_ts)
        });

        sessions
    }

    pub fn get_session_messages(
        provider_id: &str,
        source_path: &str,
    ) -> Result<Vec<SessionMessage>, AppError> {
        let path = Path::new(source_path);
        match provider_id {
            "codex" => load_codex_messages(path),
            "claude" => load_claude_messages(path),
            "opencode" => load_opencode_messages(path),
            "openclaw" => load_openclaw_messages(path),
            "gemini" => load_gemini_messages(path),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported provider: {other}"
            ))),
        }
    }
}

const CODEX_PROVIDER_ID: &str = "codex";
const CLAUDE_PROVIDER_ID: &str = "claude";
const GEMINI_PROVIDER_ID: &str = "gemini";
const OPENCODE_PROVIDER_ID: &str = "opencode";
const OPENCLAW_PROVIDER_ID: &str = "openclaw";

static CODEX_UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("valid UUID regex")
});

fn scan_codex_sessions() -> Vec<SessionMeta> {
    let root = get_codex_config_dir().join("sessions");
    let mut files = Vec::new();
    collect_matching_files(&root, &mut files, "jsonl");

    files
        .into_iter()
        .filter_map(|path| parse_codex_session(&path))
        .collect()
}

fn load_codex_messages(path: &Path) -> Result<Vec<SessionMessage>, AppError> {
    let file = File::open(path).map_err(|e| AppError::io(path, e))?;
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

        if value.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }

        let payload = match value.get("payload") {
            Some(payload) => payload,
            None => continue,
        };

        if payload.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }

        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let content = payload.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

fn parse_codex_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;

    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if session_id.is_none() {
                    session_id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if project_dir.is_none() {
                    project_dir = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if let Some(ts) = payload.get("timestamp").and_then(parse_timestamp_to_ms) {
                    created_at.get_or_insert(ts);
                }
            }
        }
    }

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
        if summary.is_none() && value.get("type").and_then(Value::as_str) == Some("response_item") {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(Value::as_str) == Some("message") {
                    let text = payload.get("content").map(extract_text).unwrap_or_default();
                    if !text.trim().is_empty() {
                        summary = Some(text);
                    }
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_codex_session_id_from_filename(path))?;
    let title = project_dir
        .as_deref()
        .and_then(path_basename)
        .map(ToOwned::to_owned);

    Some(SessionMeta {
        provider_id: CODEX_PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary: summary.map(|text| truncate_summary(&text, 160)),
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("codex resume {session_id}")),
    })
}

fn infer_codex_session_id_from_filename(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    CODEX_UUID_RE
        .find(&file_name)
        .map(|matched| matched.as_str().to_string())
}

fn scan_claude_sessions() -> Vec<SessionMeta> {
    let root = get_claude_config_dir().join("projects");
    let mut files = Vec::new();
    collect_matching_files(&root, &mut files, "jsonl");

    files
        .into_iter()
        .filter_map(|path| parse_claude_session(&path))
        .collect()
}

fn load_claude_messages(path: &Path) -> Result<Vec<SessionMessage>, AppError> {
    let file = File::open(path).map_err(|e| AppError::io(path, e))?;
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

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let content = message.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

fn parse_claude_session(path: &Path) -> Option<SessionMeta> {
    if is_claude_agent_session(path) {
        return None;
    }

    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;

    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if project_dir.is_none() {
            project_dir = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
    }

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
        if last_active_at.is_some() && summary.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
    })?;

    let title = project_dir
        .as_deref()
        .and_then(path_basename)
        .map(ToOwned::to_owned);

    Some(SessionMeta {
        provider_id: CLAUDE_PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary: summary.map(|text| truncate_summary(&text, 160)),
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("claude --resume {session_id}")),
    })
}

fn is_claude_agent_session(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("agent-"))
        .unwrap_or(false)
}

fn scan_gemini_sessions() -> Vec<SessionMeta> {
    let gemini_dir = get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let project_dirs = match std::fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return sessions,
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

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Some(meta) = parse_gemini_session(&path) {
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn load_gemini_messages(path: &Path) -> Result<Vec<SessionMessage>, AppError> {
    let data = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&data).map_err(|e| AppError::json(path, e))?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Message("No messages array found".to_string()))?;

    let mut result = Vec::new();
    for message in messages {
        let content = match message.get("content").and_then(Value::as_str) {
            Some(content) if !content.trim().is_empty() => content.to_string(),
            _ => continue,
        };

        let role = match message.get("type").and_then(Value::as_str) {
            Some("gemini") => "assistant".to_string(),
            Some("user") => "user".to_string(),
            Some(other) => other.to_string(),
            None => continue,
        };

        let ts = message.get("timestamp").and_then(parse_timestamp_to_ms);
        result.push(SessionMessage { role, content, ts });
    }

    Ok(result)
}

fn parse_gemini_session(path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("sessionId").and_then(Value::as_str)?.to_string();
    let created_at = value.get("startTime").and_then(parse_timestamp_to_ms);
    let last_active_at = value.get("lastUpdated").and_then(parse_timestamp_to_ms);
    let title = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages
                .iter()
                .find(|message| message.get("type").and_then(Value::as_str) == Some("user"))
                .and_then(|message| message.get("content").and_then(Value::as_str))
                .filter(|content| !content.trim().is_empty())
                .map(|content| truncate_summary(content, 160))
        });

    Some(SessionMeta {
        provider_id: GEMINI_PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None,
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("gemini --resume {session_id}")),
    })
}

fn scan_opencode_sessions() -> Vec<SessionMeta> {
    let storage = get_opencode_data_dir();
    let session_dir = storage.join("session");
    if !session_dir.exists() {
        return Vec::new();
    }

    let mut json_files = Vec::new();
    collect_matching_files(&session_dir, &mut json_files, "json");

    json_files
        .into_iter()
        .filter_map(|path| parse_opencode_session(&storage, &path))
        .collect()
}

fn load_opencode_messages(path: &Path) -> Result<Vec<SessionMessage>, AppError> {
    if !path.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "Message directory not found: {}",
            path.display()
        )));
    }

    let storage = path
        .parent()
        .and_then(|parent| parent.parent())
        .ok_or_else(|| {
            AppError::InvalidInput("Cannot determine storage root from message path".to_string())
        })?;

    let mut message_files = Vec::new();
    collect_matching_files(path, &mut message_files, "json");

    let mut entries: Vec<(i64, String, String, String)> = Vec::new();

    for message_path in &message_files {
        let data = match std::fs::read_to_string(message_path) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let message_id = match value.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let role = value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let created_ts = value
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(parse_timestamp_to_ms)
            .unwrap_or(0);

        let part_dir = storage.join("part").join(&message_id);
        let text = collect_opencode_parts_text(&part_dir);
        if text.trim().is_empty() {
            continue;
        }

        entries.push((created_ts, message_id, role, text));
    }

    entries.sort_by_key(|(created_ts, _, _, _)| *created_ts);

    Ok(entries
        .into_iter()
        .map(|(created_ts, _, role, content)| SessionMessage {
            role,
            content,
            ts: if created_ts > 0 {
                Some(created_ts)
            } else {
                None
            },
        })
        .collect())
}

fn parse_opencode_session(storage: &Path, path: &Path) -> Option<SessionMeta> {
    let data = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;

    let session_id = value.get("id").and_then(Value::as_str)?.to_string();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .map(ToOwned::to_owned);
    let project_dir = value
        .get("directory")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let created_at = value
        .get("time")
        .and_then(|time| time.get("created"))
        .and_then(parse_timestamp_to_ms);
    let updated_at = value
        .get("time")
        .and_then(|time| time.get("updated"))
        .and_then(parse_timestamp_to_ms);

    let has_title = title.is_some();
    let display_title = title.or_else(|| {
        project_dir
            .as_deref()
            .and_then(path_basename)
            .map(ToOwned::to_owned)
    });

    let source_path = storage.join("message").join(&session_id);
    let summary = if has_title {
        display_title.clone()
    } else {
        get_first_opencode_user_summary(storage, &session_id)
    };

    Some(SessionMeta {
        provider_id: OPENCODE_PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: display_title,
        summary,
        project_dir,
        created_at,
        last_active_at: updated_at.or(created_at),
        source_path: Some(source_path.to_string_lossy().to_string()),
        resume_command: Some(format!("opencode session resume {session_id}")),
    })
}

fn get_first_opencode_user_summary(storage: &Path, session_id: &str) -> Option<String> {
    let message_dir = storage.join("message").join(session_id);
    if !message_dir.is_dir() {
        return None;
    }

    let mut message_files = Vec::new();
    collect_matching_files(&message_dir, &mut message_files, "json");

    let mut user_messages: Vec<(i64, String)> = Vec::new();
    for message_path in &message_files {
        let data = match std::fs::read_to_string(message_path) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if value.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }

        let message_id = match value.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let ts = value
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(parse_timestamp_to_ms)
            .unwrap_or(0);

        user_messages.push((ts, message_id));
    }

    user_messages.sort_by_key(|(ts, _)| *ts);

    let (_, first_message_id) = user_messages.first()?;
    let part_dir = storage.join("part").join(first_message_id);
    let text = collect_opencode_parts_text(&part_dir);
    if text.trim().is_empty() {
        return None;
    }

    Some(truncate_summary(&text, 160))
}

fn collect_opencode_parts_text(part_dir: &Path) -> String {
    if !part_dir.is_dir() {
        return String::new();
    }

    let mut part_files = Vec::new();
    collect_matching_files(part_dir, &mut part_files, "json");

    let mut texts = Vec::new();
    for part_path in &part_files {
        let data = match std::fs::read_to_string(part_path) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if value.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }

        if let Some(text) = value.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                texts.push(text.to_string());
            }
        }
    }

    texts.join("\n")
}

fn get_opencode_data_dir() -> PathBuf {
    if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
        let trimmed = xdg_data_home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("opencode").join("storage");
        }
    }

    get_home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("storage")
}

fn scan_openclaw_sessions() -> Vec<SessionMeta> {
    let agents_dir = get_openclaw_dir().join("agents");
    if !agents_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();
    let agent_entries = match std::fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(_) => return sessions,
    };

    for agent_entry in agent_entries.flatten() {
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }

        let sessions_dir = agent_path.join("sessions");
        if !sessions_dir.is_dir() {
            continue;
        }

        let session_entries = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in session_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == "sessions.json")
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(meta) = parse_openclaw_session(&path) {
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn load_openclaw_messages(path: &Path) -> Result<Vec<SessionMessage>, AppError> {
    let file = File::open(path).map_err(|e| AppError::io(path, e))?;
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

        if value.get("type").and_then(Value::as_str) != Some("message") {
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
        let role = match raw_role {
            "toolResult" => "tool".to_string(),
            other => other.to_string(),
        };

        let content = message.get("content").map(extract_text).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }

        let ts = value.get("timestamp").and_then(parse_timestamp_to_ms);
        messages.push(SessionMessage { role, content, ts });
    }

    Ok(messages)
}

fn parse_openclaw_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut summary: Option<String> = None;

    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }

        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

        if event_type == "session" {
            if session_id.is_none() {
                session_id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if cwd.is_none() {
                cwd = value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
                created_at.get_or_insert(ts);
            }
            continue;
        }

        if event_type == "message" && summary.is_none() {
            if let Some(message) = value.get("message") {
                let text = message.get("content").map(extract_text).unwrap_or_default();
                if !text.trim().is_empty() {
                    summary = Some(text);
                }
            }
        }
    }

    let mut last_active_at: Option<i64> = None;
    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if let Some(ts) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
            last_active_at = Some(ts);
            break;
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned)
    })?;

    let title = cwd
        .as_deref()
        .and_then(path_basename)
        .map(ToOwned::to_owned);

    Some(SessionMeta {
        provider_id: OPENCLAW_PROVIDER_ID.to_string(),
        session_id,
        title,
        summary: summary.map(|text| truncate_summary(&text, 160)),
        project_dir: cwd,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: None,
    })
}

fn collect_matching_files(root: &Path, files: &mut Vec<PathBuf>, extension: &str) {
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
            collect_matching_files(&path, files, extension);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            files.push(path);
        }
    }
}

fn read_head_tail_lines(
    path: &Path,
    head_n: usize,
    tail_n: usize,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();

    if file_len < 16_384 {
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let head = all_lines.iter().take(head_n).cloned().collect();
        let skip = all_lines.len().saturating_sub(tail_n);
        let tail = all_lines.into_iter().skip(skip).collect();
        return Ok((head, tail));
    }

    let reader = BufReader::new(file);
    let head: Vec<String> = reader.lines().take(head_n).map_while(Result::ok).collect();

    let seek_pos = file_len.saturating_sub(16_384);
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(seek_pos))?;
    let tail_reader = BufReader::new(file);
    let all_tail: Vec<String> = tail_reader.lines().map_while(Result::ok).collect();

    let skip_first = if seek_pos > 0 { 1 } else { 0 };
    let usable: Vec<String> = all_tail.into_iter().skip(skip_first).collect();
    let skip = usable.len().saturating_sub(tail_n);
    let tail = usable.into_iter().skip(skip).collect();

    Ok((head, tail))
}

fn parse_timestamp_to_ms(value: &Value) -> Option<i64> {
    let raw = value.as_str()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt: DateTime<FixedOffset>| dt.timestamp_millis())
}

fn extract_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(extract_text_from_item)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_text_from_item(item: &Value) -> Option<String> {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("input_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    if let Some(text) = item.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    if let Some(content) = item.get("content") {
        let text = extract_text(content);
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut result = trimmed.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

fn path_basename(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches(['/', '\\']);
    normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|segment| !segment.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    #[serial]
    fn list_sessions_and_load_messages_for_claude() {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let session_path = temp
            .path()
            .join(".claude/projects/demo-project/session-1.jsonl");
        write_file(
            &session_path,
            concat!(
                "{\"sessionId\":\"session-1\",\"cwd\":\"/work/demo-project\",\"timestamp\":\"2026-03-08T10:00:00Z\",\"isMeta\":true}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello from claude\"},\"timestamp\":\"2026-03-08T10:01:00Z\"}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"timestamp\":\"2026-03-08T10:02:00Z\"}\n"
            ),
        );

        let sessions = SessionService::list_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_id, "claude");
        assert_eq!(sessions[0].session_id, "session-1");
        assert_eq!(sessions[0].title.as_deref(), Some("demo-project"));
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some("claude --resume session-1")
        );

        let messages =
            SessionService::get_session_messages("claude", &session_path.to_string_lossy())
                .expect("load claude messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from claude");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    #[serial]
    fn list_sessions_and_load_messages_for_opencode() {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::env::set_var("XDG_DATA_HOME", temp.path().join(".local/share"));

        let storage = temp.path().join(".local/share/opencode/storage");

        write_file(
            &storage.join("session/session-a.json"),
            concat!(
                "{",
                "\"id\":\"session-a\",",
                "\"directory\":\"/repo/opencode-demo\",",
                "\"time\":{\"created\":\"2026-03-08T09:00:00Z\",\"updated\":\"2026-03-08T09:05:00Z\"}",
                "}"
            ),
        );
        write_file(
            &storage.join("message/session-a/msg-1.json"),
            concat!(
                "{",
                "\"id\":\"msg-1\",",
                "\"role\":\"user\",",
                "\"time\":{\"created\":\"2026-03-08T09:01:00Z\"}",
                "}"
            ),
        );
        write_file(
            &storage.join("part/msg-1/part-1.json"),
            "{\"type\":\"text\",\"text\":\"hello from opencode\"}",
        );

        let sessions = SessionService::list_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_id, "opencode");
        assert_eq!(sessions[0].session_id, "session-a");
        assert_eq!(sessions[0].summary.as_deref(), Some("hello from opencode"));

        let messages = SessionService::get_session_messages(
            "opencode",
            &storage.join("message/session-a").to_string_lossy(),
        )
        .expect("load opencode messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from opencode");
    }

    #[test]
    fn rejects_unsupported_provider() {
        let error = SessionService::get_session_messages("unknown", "/tmp/nope")
            .expect_err("unsupported provider should fail");
        assert!(error.to_string().contains("Unsupported provider"));
    }
}
