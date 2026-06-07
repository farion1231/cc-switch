pub mod providers;
pub mod terminal;

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use providers::{claude, codex, gemini, hermes, openclaw, opencode};

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
pub struct DeleteSessionRequest {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionOutcome {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSessionRequest {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub format: SessionExportFormat,
    pub output_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionExportFormat {
    Markdown,
    Html,
    Text,
    Raw,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSessionExportRequest {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub format: SessionExportFormat,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let (r1, r2, r3, r4, r5, r6) = std::thread::scope(|s| {
        let h1 = s.spawn(codex::scan_sessions);
        let h2 = s.spawn(claude::scan_sessions);
        let h3 = s.spawn(opencode::scan_sessions);
        let h4 = s.spawn(openclaw::scan_sessions);
        let h5 = s.spawn(gemini::scan_sessions);
        let h6 = s.spawn(hermes::scan_sessions);
        (
            h1.join().unwrap_or_default(),
            h2.join().unwrap_or_default(),
            h3.join().unwrap_or_default(),
            h4.join().unwrap_or_default(),
            h5.join().unwrap_or_default(),
            h6.join().unwrap_or_default(),
        )
    });

    let mut sessions = Vec::new();
    sessions.extend(r1);
    sessions.extend(r2);
    sessions.extend(r3);
    sessions.extend(r4);
    sessions.extend(r5);
    sessions.extend(r6);

    sessions.sort_by(|a, b| {
        let a_ts = a.last_active_at.or(a.created_at).unwrap_or(0);
        let b_ts = b.last_active_at.or(b.created_at).unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    sessions
}

pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    // SQLite sessions use a "sqlite:" prefixed source_path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::load_messages_sqlite(source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::load_messages_sqlite(source_path);
    }

    let path = Path::new(source_path);
    match provider_id {
        "codex" => codex::load_messages(path),
        "claude" => claude::load_messages(path),
        "opencode" => opencode::load_messages(path),
        "openclaw" => openclaw::load_messages(path),
        "gemini" => gemini::load_messages(path),
        "hermes" => hermes::load_messages(path),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

pub fn delete_session(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<bool, String> {
    // SQLite sessions bypass the file-based deletion path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::delete_session_sqlite(session_id, source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::delete_session_sqlite(session_id, source_path);
    }

    let roots = provider_roots(provider_id)?;
    delete_session_with_roots(provider_id, session_id, Path::new(source_path), &roots)
}

pub fn delete_sessions(requests: &[DeleteSessionRequest]) -> Vec<DeleteSessionOutcome> {
    collect_delete_session_outcomes(requests, |request| {
        delete_session(
            &request.provider_id,
            &request.session_id,
            &request.source_path,
        )
    })
}

pub fn export_session(request: &ExportSessionRequest) -> Result<bool, String> {
    let meta = validate_export_request(
        &request.provider_id,
        &request.session_id,
        &request.source_path,
        &request.format,
    )?;

    let source_path = Path::new(&request.source_path);
    let output_path = Path::new(&request.output_path);
    if request.provider_id == "opencode" && request.source_path.starts_with("sqlite:") {
        reject_opencode_sqlite_export_target(&request.source_path, output_path)?;
    } else if !request.source_path.starts_with("sqlite:") {
        reject_same_export_target(source_path, output_path)?;
    }

    match request.format {
        SessionExportFormat::Raw => {
            std::fs::copy(source_path, output_path)
                .map_err(|e| format!("Failed to copy raw session file: {e}"))?;
        }
        SessionExportFormat::Markdown | SessionExportFormat::Html | SessionExportFormat::Text => {
            let content = render_session_export_with_meta(&meta, &request.format)?;
            std::fs::write(output_path, content)
                .map_err(|e| format!("Failed to write export file: {e}"))?;
        }
    }

    Ok(true)
}

pub fn render_session_export(request: &RenderSessionExportRequest) -> Result<String, String> {
    if request.format == SessionExportFormat::Raw {
        return Err("Raw export cannot be rendered as text".to_string());
    }

    let meta = validate_export_request(
        &request.provider_id,
        &request.session_id,
        &request.source_path,
        &request.format,
    )?;
    render_session_export_with_meta(&meta, &request.format)
}

fn reject_same_export_target(source_path: &Path, output_path: &Path) -> Result<(), String> {
    let output_exists = output_path.try_exists().map_err(|e| {
        format!(
            "Failed to inspect export target {}: {e}",
            output_path.display()
        )
    })?;

    if !output_exists {
        return Ok(());
    }

    let is_same_file = same_file::is_same_file(source_path, output_path)
        .map_err(|e| format!("Failed to compare export target with session source: {e}"))?;

    if is_same_file {
        return Err("Export target must be different from the source session file".to_string());
    }

    Ok(())
}

fn reject_opencode_sqlite_export_target(
    source_path: &str,
    output_path: &Path,
) -> Result<(), String> {
    let db_path = opencode::sqlite_source_db_path(source_path)
        .ok_or_else(|| format!("Invalid SQLite source reference: {source_path}"))?;

    reject_same_export_target(&db_path, output_path)
}

fn validate_export_request(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
    format: &SessionExportFormat,
) -> Result<SessionMeta, String> {
    let meta = find_session_meta(provider_id, session_id, source_path).ok_or_else(|| {
        format!("Session not found or session ID mismatch: {provider_id}/{session_id}")
    })?;

    if *format == SessionExportFormat::Raw && source_path.starts_with("sqlite:") {
        return Err("Raw export is not supported for SQLite-backed sessions".to_string());
    }

    if !source_path.starts_with("sqlite:") {
        let roots = provider_roots(provider_id)?;
        let validated_source =
            canonicalize_existing_path(Path::new(source_path), "session source")?;

        let mut saw_existing_root = false;
        let mut source_under_root = false;
        for root in &roots {
            if !root.exists() {
                continue;
            }

            saw_existing_root = true;
            let validated_root = canonicalize_existing_path(root, "session root")?;
            if validated_source.starts_with(&validated_root) {
                source_under_root = true;
                break;
            }
        }

        if !saw_existing_root {
            return Err(format!(
                "Session root not found for provider {provider_id}: {}",
                roots
                    .first()
                    .map(|root| root.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            ));
        }

        if !source_under_root {
            return Err(format!(
                "Session source path is outside provider roots: {}",
                Path::new(source_path).display()
            ));
        }

        if *format == SessionExportFormat::Raw && !validated_source.is_file() {
            return Err("Raw export is only supported for single-file session sources".to_string());
        }
    }

    Ok(meta)
}

fn find_session_meta(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Option<SessionMeta> {
    scan_sessions_for_provider(provider_id)
        .into_iter()
        .find(|session| {
            session.session_id == session_id
                && session.source_path.as_deref() == Some(source_path)
                && session.provider_id == provider_id
        })
}

fn scan_sessions_for_provider(provider_id: &str) -> Vec<SessionMeta> {
    match provider_id {
        "codex" => codex::scan_sessions(),
        "claude" => claude::scan_sessions(),
        "opencode" => opencode::scan_sessions(),
        "openclaw" => openclaw::scan_sessions(),
        "gemini" => gemini::scan_sessions(),
        "hermes" => hermes::scan_sessions(),
        _ => Vec::new(),
    }
}

fn render_session_export_with_meta(
    meta: &SessionMeta,
    format: &SessionExportFormat,
) -> Result<String, String> {
    let source_path = meta
        .source_path
        .as_deref()
        .ok_or_else(|| "Session has no source path".to_string())?;
    let messages = load_messages(&meta.provider_id, source_path)?;

    match format {
        SessionExportFormat::Markdown => Ok(render_markdown(meta, &messages)),
        SessionExportFormat::Html => Ok(render_html(meta, &messages)),
        SessionExportFormat::Text => Ok(render_text(meta, &messages)),
        SessionExportFormat::Raw => Err("Raw export cannot be rendered as text".to_string()),
    }
}

fn delete_session_with_roots(
    provider_id: &str,
    session_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<bool, String> {
    let validated_source = canonicalize_existing_path(source_path, "session source")?;

    let mut saw_existing_root = false;
    for root in roots {
        if !root.exists() {
            continue;
        }

        saw_existing_root = true;
        let validated_root = canonicalize_existing_path(root, "session root")?;
        if validated_source.starts_with(&validated_root) {
            return match provider_id {
                "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
                "claude" => claude::delete_session(&validated_root, &validated_source, session_id),
                "opencode" => {
                    opencode::delete_session(&validated_root, &validated_source, session_id)
                }
                "openclaw" => {
                    openclaw::delete_session(&validated_root, &validated_source, session_id)
                }
                "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
                "hermes" => hermes::delete_session(&validated_root, &validated_source, session_id),
                _ => Err(format!("Unsupported provider: {provider_id}")),
            };
        }
    }

    if !saw_existing_root {
        return Err(format!(
            "Session root not found for provider {provider_id}: {}",
            roots
                .first()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    Err(format!(
        "Session source path is outside provider roots: {}",
        source_path.display()
    ))
}

fn render_markdown(meta: &SessionMeta, messages: &[SessionMessage]) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&markdown_heading_text(&session_display_title(meta)));
    out.push_str("\n\n");
    append_markdown_metadata(&mut out, meta, messages.len());
    out.push_str("\n---\n\n");

    for message in messages {
        let role = export_role_label(&message.role);
        out.push_str("## ");
        out.push_str(role);
        if let Some(ts) = message.ts {
            out.push_str(" · ");
            out.push_str(&format_export_time(Some(ts)));
        }
        out.push_str("\n\n");

        if is_markdown_passthrough_role(&message.role) {
            out.push_str(message.content.trim_end());
            out.push_str("\n\n");
        } else {
            let fence = dynamic_markdown_fence(&message.content);
            out.push_str(&fence);
            out.push_str("text\n");
            out.push_str(message.content.trim_end());
            out.push('\n');
            out.push_str(&fence);
            out.push_str("\n\n");
        }
    }

    out
}

fn append_markdown_metadata(out: &mut String, meta: &SessionMeta, message_count: usize) {
    let fields = metadata_fields(meta, message_count);
    for (label, value) in fields {
        if !value.is_empty() {
            out.push_str("- **");
            out.push_str(label);
            out.push_str(":** ");
            out.push_str(&escape_markdown_metadata(&value));
            out.push('\n');
        }
    }
}

fn render_text(meta: &SessionMeta, messages: &[SessionMessage]) -> String {
    let mut out = String::new();
    out.push_str("CC Switch Session Export\n");
    out.push_str("Title: ");
    out.push_str(&session_display_title(meta));
    out.push('\n');
    for (label, value) in metadata_fields(meta, messages.len()) {
        if !value.is_empty() {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        }
    }
    out.push_str("\n============================================================\n\n");

    for message in messages {
        out.push('[');
        out.push_str(export_role_label(&message.role));
        out.push(']');
        if message.ts.is_some() {
            out.push(' ');
            out.push_str(&format_export_time(message.ts));
        }
        out.push_str("\n------------------------------------------------------------\n");
        out.push_str(message.content.trim_end());
        out.push_str("\n\n");
    }

    out
}

fn render_html(meta: &SessionMeta, messages: &[SessionMessage]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>");
    out.push_str(&escape_html(&session_display_title(meta)));
    out.push_str("</title>\n<style>\n");
    out.push_str("body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;line-height:1.55;margin:0;background:#f6f7f9;color:#15171a}main{max-width:980px;margin:0 auto;padding:32px 20px}.meta,.message{background:#fff;border:1px solid #dfe3e8;border-radius:12px;padding:16px 18px;margin-bottom:16px;box-shadow:0 1px 2px rgba(0,0,0,.04)}h1{font-size:28px;margin:0 0 16px}.meta dl{display:grid;grid-template-columns:max-content 1fr;gap:8px 16px;margin:0}.meta dt{font-weight:700;color:#555}.meta dd{margin:0;word-break:break-word}.message-header{display:flex;justify-content:space-between;gap:12px;margin-bottom:10px;font-size:13px;color:#667085}.role{font-weight:700}.content{white-space:pre-wrap;overflow-wrap:anywhere;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:13px}.role-user{color:#079455}.role-assistant{color:#1570ef}.role-tool{color:#7a5af8}.role-system,.role-developer{color:#dc6803}.role-unknown{color:#667085}\n");
    out.push_str("</style>\n</head>\n<body>\n<main>\n<h1>");
    out.push_str(&escape_html(&session_display_title(meta)));
    out.push_str("</h1>\n<section class=\"meta\"><dl>\n");
    for (label, value) in metadata_fields(meta, messages.len()) {
        if !value.is_empty() {
            out.push_str("<dt>");
            out.push_str(&escape_html(label));
            out.push_str("</dt><dd>");
            out.push_str(&escape_html(&value));
            out.push_str("</dd>\n");
        }
    }
    out.push_str("</dl></section>\n");

    for message in messages {
        let role = export_role_label(&message.role);
        let role_class = html_role_class(&message.role);
        out.push_str("<article class=\"message ");
        out.push_str(role_class);
        out.push_str("\"><header class=\"message-header\"><span class=\"role ");
        out.push_str(role_class);
        out.push_str("\">");
        out.push_str(&escape_html(role));
        out.push_str("</span><time>");
        out.push_str(&escape_html(&format_export_time(message.ts)));
        out.push_str("</time></header><div class=\"content\">");
        out.push_str(&escape_html(&message.content));
        out.push_str("</div></article>\n");
    }

    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn metadata_fields(meta: &SessionMeta, message_count: usize) -> Vec<(&'static str, String)> {
    vec![
        ("Provider", meta.provider_id.clone()),
        ("Session ID", meta.session_id.clone()),
        (
            "Project Directory",
            meta.project_dir.clone().unwrap_or_default(),
        ),
        ("Created At", format_export_time(meta.created_at)),
        ("Last Active At", format_export_time(meta.last_active_at)),
        (
            "Exported At",
            format_export_time(Some(Local::now().timestamp_millis())),
        ),
        ("Message Count", message_count.to_string()),
    ]
}

fn session_display_title(meta: &SessionMeta) -> String {
    meta.title
        .clone()
        .or_else(|| meta.project_dir.as_deref().and_then(local_path_basename))
        .unwrap_or_else(|| meta.session_id.chars().take(8).collect())
}

fn local_path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn format_export_time(value: Option<i64>) -> String {
    let Some(ms) = value else {
        return String::new();
    };
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

fn export_role_label(role: &str) -> &str {
    match role.to_ascii_lowercase().as_str() {
        "user" => "User",
        "assistant" => "Assistant",
        "tool" => "Tool",
        "system" => "System",
        "developer" => "Developer",
        "unknown" => "Unknown",
        _ => role,
    }
}

fn is_markdown_passthrough_role(role: &str) -> bool {
    matches!(role.to_ascii_lowercase().as_str(), "user" | "assistant")
}

fn dynamic_markdown_fence(content: &str) -> String {
    let mut max_run = 0usize;
    let mut current = 0usize;
    for ch in content.chars() {
        if ch == '`' {
            current += 1;
            max_run = max_run.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(std::cmp::max(3, max_run + 1))
}

fn markdown_heading_text(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

fn escape_markdown_metadata(value: &str) -> String {
    value.replace('\n', " ")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html_role_class(role: &str) -> &'static str {
    match role.to_ascii_lowercase().as_str() {
        "user" => "role-user",
        "assistant" => "role-assistant",
        "tool" => "role-tool",
        "system" => "role-system",
        "developer" => "role-developer",
        _ => "role-unknown",
    }
}

fn provider_roots(provider_id: &str) -> Result<Vec<PathBuf>, String> {
    let roots = match provider_id {
        "codex" => codex::session_roots(),
        "claude" => vec![crate::config::get_claude_config_dir().join("projects")],
        "opencode" => vec![opencode::get_opencode_data_dir()],
        "openclaw" => vec![crate::openclaw_config::get_openclaw_dir().join("agents")],
        "gemini" => vec![crate::gemini_config::get_gemini_dir().join("tmp")],
        "hermes" => vec![crate::hermes_config::get_hermes_dir().join("sessions")],
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };

    Ok(roots)
}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("{label} not found: {}", path.display()));
    }

    path.canonicalize()
        .map_err(|e| format!("Failed to resolve {label} {}: {e}", path.display()))
}

fn collect_delete_session_outcomes<F>(
    requests: &[DeleteSessionRequest],
    mut deleter: F,
) -> Vec<DeleteSessionOutcome>
where
    F: FnMut(&DeleteSessionRequest) -> Result<bool, String>,
{
    requests
        .iter()
        .map(|request| match deleter(request) {
            Ok(true) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: true,
                error: None,
            },
            Ok(false) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some("Session was not deleted".to_string()),
            },
            Err(error) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some(error),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    fn write_codex_session(path: &Path, session_id: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/tmp/project\"}}}}\n\
                 {{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}}}\n",
            ),
        )
        .expect("write source");
    }

    #[test]
    fn accepts_source_path_under_any_allowed_provider_root() {
        let active_root = tempdir().expect("active root");
        let archived_root = tempdir().expect("archived root");
        let source = archived_root.path().join("session.jsonl");
        write_codex_session(&source, "archived-session");

        let deleted = delete_session_with_roots(
            "codex",
            "archived-session",
            &source,
            &[
                active_root.path().to_path_buf(),
                archived_root.path().to_path_buf(),
            ],
        )
        .expect("delete archived session");

        assert!(deleted);
        assert!(!source.exists());
    }

    #[test]
    fn rejects_source_path_outside_provider_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let source = outside.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err =
            delete_session_with_roots("codex", "session-1", &source, &[root.path().to_path_buf()])
                .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider roots"));
    }

    #[test]
    fn rejects_missing_source_path() {
        let root = tempdir().expect("tempdir");
        let missing = root.path().join("missing.jsonl");

        let err =
            delete_session_with_roots("codex", "session-1", &missing, &[root.path().to_path_buf()])
                .expect_err("expected missing source path to fail");

        assert!(err.contains("session source not found"));
    }

    #[test]
    fn raw_export_target_may_be_new_file() {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("session.jsonl");
        let output = root.path().join("export.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        reject_same_export_target(&source, &output).expect("new output path should be accepted");
    }

    #[test]
    fn raw_export_rejects_source_as_target() {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err = reject_same_export_target(&source, &source)
            .expect_err("same source and target should be rejected");

        assert!(err.contains("different from the source session file"));
    }

    #[test]
    fn raw_export_rejects_hard_linked_target() {
        let root = tempdir().expect("tempdir");
        let source = root.path().join("session.jsonl");
        let output = root.path().join("export.jsonl");
        std::fs::write(&source, "{}").expect("write source");
        if let Err(error) = std::fs::hard_link(&source, &output) {
            eprintln!("skipping hard-link assertion: {error}");
            return;
        }

        let err = reject_same_export_target(&source, &output)
            .expect_err("hard-linked target should be rejected");

        assert!(err.contains("different from the source session file"));
    }

    #[test]
    fn sqlite_export_rejects_opencode_db_as_target() {
        let root = tempdir().expect("tempdir");
        let db_path = root.path().join("opencode.db");
        std::fs::write(&db_path, "sqlite").expect("write db");
        let source = format!("sqlite:{}:ses_123", db_path.display());

        let err = reject_opencode_sqlite_export_target(&source, &db_path)
            .expect_err("sqlite db target should be rejected");

        assert!(err.contains("different from the source session file"));
    }

    #[test]
    #[serial]
    fn markdown_export_rejects_source_as_target_without_overwriting_session() {
        let temp_home = tempdir().expect("tempdir");
        let session_dir = temp_home.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&session_dir).expect("create sessions dir");
        let source = session_dir.join("session.jsonl");
        let source_content = concat!(
            "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/tmp/project\"}}\n",
            "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n"
        );
        std::fs::write(&source, source_content).expect("write source");

        let original_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp_home.path());
        let result = export_session(&ExportSessionRequest {
            provider_id: "codex".to_string(),
            session_id: "session-1".to_string(),
            source_path: source.to_string_lossy().to_string(),
            format: SessionExportFormat::Markdown,
            output_path: source.to_string_lossy().to_string(),
        });
        match original_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }

        let err = result.expect_err("source target should be rejected");
        assert!(err.contains("different from the source session file"));
        assert_eq!(
            std::fs::read_to_string(&source).expect("read source"),
            source_content
        );
    }

    #[test]
    fn markdown_renderer_preserves_assistant_markdown_and_fences_tool_output() {
        let meta = test_meta();
        let messages = vec![
            SessionMessage {
                role: "assistant".to_string(),
                content: "```rust\nfn main() {}\n```".to_string(),
                ts: Some(1_700_000_000_000),
            },
            SessionMessage {
                role: "tool".to_string(),
                content: "output with ``` fence".to_string(),
                ts: Some(1_700_000_001_000),
            },
        ];

        let rendered = render_markdown(&meta, &messages);

        assert!(rendered.contains("- **Provider:** codex"));
        assert!(rendered.contains("## Assistant"));
        assert!(rendered.contains("```rust\nfn main() {}\n```"));
        assert!(rendered.contains("## Tool"));
        assert!(rendered.contains("````text\noutput with ``` fence\n````"));
    }

    #[test]
    fn html_renderer_escapes_content() {
        let meta = test_meta();
        let messages = vec![SessionMessage {
            role: "user".to_string(),
            content: "<script>alert(\"x\")</script>".to_string(),
            ts: None,
        }];

        let rendered = render_html(&meta, &messages);

        assert!(rendered.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"));
        assert!(!rendered.contains("<script>alert"));
        assert!(rendered.contains("<style>"));
    }

    #[test]
    fn text_renderer_includes_transcript_separators() {
        let meta = test_meta();
        let messages = vec![SessionMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            ts: None,
        }];

        let rendered = render_text(&meta, &messages);

        assert!(rendered.contains("CC Switch Session Export"));
        assert!(rendered.contains("[User]"));
        assert!(rendered.contains("------------------------------------------------------------"));
        assert!(rendered.contains("hello"));
    }

    fn test_meta() -> SessionMeta {
        SessionMeta {
            provider_id: "codex".to_string(),
            session_id: "session-1".to_string(),
            title: Some("Test Session".to_string()),
            summary: None,
            project_dir: Some("/tmp/project".to_string()),
            created_at: Some(1_700_000_000_000),
            last_active_at: Some(1_700_000_001_000),
            source_path: Some("/tmp/session.jsonl".to_string()),
            resume_command: None,
        }
    }

    #[test]
    fn batch_delete_collects_successes_and_failures_in_order() {
        let requests = vec![
            DeleteSessionRequest {
                provider_id: "codex".to_string(),
                session_id: "s1".to_string(),
                source_path: "/tmp/s1".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "claude".to_string(),
                session_id: "s2".to_string(),
                source_path: "/tmp/s2".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "gemini".to_string(),
                session_id: "s3".to_string(),
                source_path: "/tmp/s3".to_string(),
            },
        ];

        let outcomes = collect_delete_session_outcomes(&requests, |request| {
            match request.session_id.as_str() {
                "s1" => Ok(true),
                "s2" => Err("boom".to_string()),
                _ => Ok(false),
            }
        });

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].success);
        assert_eq!(outcomes[0].error, None);
        assert!(!outcomes[1].success);
        assert_eq!(outcomes[1].error.as_deref(), Some("boom"));
        assert!(!outcomes[2].success);
        assert_eq!(
            outcomes[2].error.as_deref(),
            Some("Session was not deleted")
        );
    }
}
