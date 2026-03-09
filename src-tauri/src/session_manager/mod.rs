pub mod providers;
pub mod terminal;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::config::{write_json_file, write_text_file};

use providers::{claude, codex, gemini, openclaw, opencode};

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExportTarget {
    pub provider_id: String,
    pub source_path: String,
    pub session_id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionExportData {
    schema_version: String,
    session: SessionExportSession,
    messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionExportSession {
    provider_id: String,
    source_path: String,
    session_id: Option<String>,
    title: Option<String>,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let (r1, r2, r3, r4, r5) = std::thread::scope(|s| {
        let h1 = s.spawn(codex::scan_sessions);
        let h2 = s.spawn(claude::scan_sessions);
        let h3 = s.spawn(opencode::scan_sessions);
        let h4 = s.spawn(openclaw::scan_sessions);
        let h5 = s.spawn(gemini::scan_sessions);
        (
            h1.join().unwrap_or_default(),
            h2.join().unwrap_or_default(),
            h3.join().unwrap_or_default(),
            h4.join().unwrap_or_default(),
            h5.join().unwrap_or_default(),
        )
    });

    let mut sessions = Vec::new();
    sessions.extend(r1);
    sessions.extend(r2);
    sessions.extend(r3);
    sessions.extend(r4);
    sessions.extend(r5);

    sessions.sort_by(|a, b| {
        let a_ts = a.last_active_at.or(a.created_at).unwrap_or(0);
        let b_ts = b.last_active_at.or(b.created_at).unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    sessions
}

pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    let path = Path::new(source_path);
    match provider_id {
        "codex" => codex::load_messages(path),
        "claude" => claude::load_messages(path),
        "opencode" => opencode::load_messages(path),
        "openclaw" => openclaw::load_messages(path),
        "gemini" => gemini::load_messages(path),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

pub fn export_session_to_file(
    provider_id: &str,
    source_path: &str,
    session_id: Option<&str>,
    title: Option<&str>,
    format: &str,
    file_path: &str,
) -> Result<(), String> {
    let messages = load_messages(provider_id, source_path)?;
    let payload = build_export_data(provider_id, source_path, session_id, title, messages);
    write_export_file(Path::new(file_path), format, &payload)
}

pub fn export_sessions_to_directory(
    sessions: Vec<SessionExportTarget>,
    format: &str,
    directory_path: &str,
) -> Result<usize, String> {
    let directory = Path::new(directory_path);
    if !directory.exists() {
        return Err(format!("Directory does not exist: {}", directory.display()));
    }
    if !directory.is_dir() {
        return Err(format!("Path is not a directory: {}", directory.display()));
    }

    let extension = match format {
        "md" => "md",
        "json" => "json",
        _ => return Err(format!("Unsupported export format: {format}")),
    };

    let mut used_names = HashSet::new();
    let mut exported = 0usize;

    for session in sessions {
        let messages = load_messages(&session.provider_id, &session.source_path)?;
        let payload = build_export_data(
            &session.provider_id,
            &session.source_path,
            session.session_id.as_deref(),
            session.title.as_deref(),
            messages,
        );

        let base = build_export_base_name(
            &session.provider_id,
            session.session_id.as_deref(),
            session.title.as_deref(),
            &session.source_path,
        );

        let unique_name = allocate_unique_file_name(directory, &base, extension, &mut used_names);
        let target = directory.join(unique_name);
        write_export_file(&target, format, &payload)?;
        exported += 1;
    }

    Ok(exported)
}

fn build_export_data(
    provider_id: &str,
    source_path: &str,
    session_id: Option<&str>,
    title: Option<&str>,
    messages: Vec<SessionMessage>,
) -> SessionExportData {
    SessionExportData {
        schema_version: "1.0".to_string(),
        session: SessionExportSession {
            provider_id: provider_id.to_string(),
            source_path: source_path.to_string(),
            session_id: session_id.map(ToOwned::to_owned),
            title: title.map(ToOwned::to_owned),
        },
        messages,
    }
}

fn write_export_file(path: &Path, format: &str, payload: &SessionExportData) -> Result<(), String> {
    match format {
        "json" => write_json_file(path, payload).map_err(|e| format!("Failed to write JSON export: {e}")),
        "md" => {
            let markdown = render_markdown(payload);
            write_text_file(path, &markdown).map_err(|e| format!("Failed to write Markdown export: {e}"))
        }
        _ => Err(format!("Unsupported export format: {format}")),
    }
}

fn render_markdown(payload: &SessionExportData) -> String {
    let title = payload
        .session
        .title
        .as_deref()
        .unwrap_or("Untitled Session");

    let mut out = String::new();
    out.push_str("# ");
    out.push_str(title);
    out.push_str("\n\n");

    out.push_str("## Metadata\n");
    out.push_str("- Provider: `");
    out.push_str(&payload.session.provider_id);
    out.push_str("`\n");

    if let Some(session_id) = payload.session.session_id.as_deref() {
        out.push_str("- Session ID: `");
        out.push_str(session_id);
        out.push_str("`\n");
    }

    out.push_str("- Source Path: `");
    out.push_str(&payload.session.source_path.replace('`', "\\`"));
    out.push_str("`\n");
    out.push_str("- Messages: ");
    out.push_str(&payload.messages.len().to_string());
    out.push_str("\n\n");

    out.push_str("## Messages\n\n");
    for (index, message) in payload.messages.iter().enumerate() {
        out.push_str("### ");
        out.push_str(&(index + 1).to_string());
        out.push_str(". ");
        out.push_str(&message.role);
        out.push_str("\n\n");

        if let Some(ts) = message.ts {
            out.push_str("_Timestamp: `");
            out.push_str(&ts.to_string());
            out.push_str("`_\n\n");
        }

        out.push_str(&message.content);
        out.push_str("\n\n");
    }

    out
}

fn build_export_base_name(
    provider_id: &str,
    session_id: Option<&str>,
    title: Option<&str>,
    source_path: &str,
) -> String {
    let preferred = title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| session_id.map(str::trim).filter(|s| !s.is_empty()))
        .or_else(|| {
            Path::new(source_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("session");

    let safe_provider = sanitize_file_component(provider_id);
    let safe_name = sanitize_file_component(preferred);
    format!("{safe_provider}-{safe_name}")
}

fn sanitize_file_component(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        let invalid = matches!(
            ch,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\u{0000}'..='\u{001F}'
        );
        if invalid {
            out.push('-');
        } else {
            out.push(ch);
        }
    }

    let collapsed = out
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .trim_matches(['.', '-', ' '])
        .to_string();

    if collapsed.is_empty() {
        "session".to_string()
    } else {
        collapsed
    }
}

fn allocate_unique_file_name(
    directory: &Path,
    base_name: &str,
    extension: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let mut index = 0usize;
    loop {
        let candidate = if index == 0 {
            format!("{base_name}.{extension}")
        } else {
            format!("{base_name}-{index}.{extension}")
        };

        let path = directory.join(&candidate);
        if !used_names.contains(&candidate) && !path.exists() {
            used_names.insert(candidate.clone());
            return candidate;
        }
        index += 1;
    }
}
