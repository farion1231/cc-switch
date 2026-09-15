//! OMP session browser.
//!
//! OMP stores conversation trees as JSONL under `<omp agent dir>/sessions/`
//! in a two-level layout: `sessions/<encoded-project-dir>/<file>.jsonl`.
//! The entry format follows Pi's tree protocol (header `type:"session"`,
//! entries with `id`/`parentId`), but OMP additionally prepends a
//! `{"type":"title"}` line before the session header, which Pi's strict
//! "header first" parser rejects — hence this separate provider.
//!
//! Sessions also own a sibling sidecar directory (`<file-stem>/`) holding
//! bash logs; deleting a session removes both the JSONL file and its sidecar.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::pi::{is_valid_tree_id, MAX_SESSION_BYTES, MAX_TREE_ENTRIES};
use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, truncate_summary, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "omp";

#[derive(Debug)]
struct SessionHeader {
    id: String,
    cwd: String,
    timestamp: Option<i64>,
    version: u64,
}

#[derive(Debug)]
struct SessionTree {
    header: SessionHeader,
    active_entry_indexes: HashSet<usize>,
    summary: SessionSummary,
}

#[derive(Debug, Default)]
struct SessionSummary {
    first_user_message: Option<String>,
    last_message: Option<String>,
    /// Last non-empty explicit title (`{"type":"title"}` / `session_info`).
    explicit_title: Option<String>,
    last_active_at: Option<i64>,
}

/// OMP's session root is fixed relative to the agent directory (honoring the
/// same `PI_CODING_AGENT_DIR` override as the provider config).
pub fn session_roots() -> Vec<PathBuf> {
    match crate::omp_config::get_omp_agent_dir() {
        Ok(agent_dir) => vec![agent_dir.join("sessions")],
        Err(error) => {
            log::warn!("OMP session root unavailable: {error}");
            Vec::new()
        }
    }
}

fn session_root() -> Result<PathBuf, String> {
    session_roots()
        .into_iter()
        .next()
        .ok_or_else(|| "OMP agent directory is not available".to_string())
}

/// Every OMP session JSONL under the current agent directory, for the usage
/// importer. Sorted so cursor bookkeeping is deterministic across scans.
pub(crate) fn session_files() -> Result<Vec<PathBuf>, String> {
    let root = session_root()?;
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);
    files.sort();
    Ok(files)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let Ok(root) = session_root() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);
    files
        .into_iter()
        .filter_map(|path| match parse_session(&path) {
            Ok(session) => Some(session),
            Err(error) => {
                log::debug!("Skipping invalid OMP session {}: {error}", path.display());
                None
            }
        })
        .collect()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let root = session_root()?;
    let (_, source) = validate_source_under_root(&root, path)?;
    let tree = read_tree(&source)?;
    read_active_messages(&source, &tree)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if !is_valid_tree_id(session_id) {
        return Err("Invalid OMP session ID".to_string());
    }
    let (_, source) = validate_source_under_root(root, path)?;
    let tree = read_tree(&source)?;
    if tree.header.id != session_id {
        return Err(format!(
            "OMP session ID mismatch: expected {session_id}, found {}",
            tree.header.id
        ));
    }
    fs::remove_file(&source)
        .map_err(|error| format!("Failed to delete OMP session {}: {error}", source.display()))?;
    // The sidecar log directory shares the JSONL file's stem. A failed sidecar
    // removal must not resurface the (already deleted) session, so log it.
    let sidecar = source.with_extension("");
    if sidecar.is_dir() {
        if let Err(error) = fs::remove_dir_all(&sidecar) {
            log::warn!(
                "Failed to delete OMP session sidecar {}: {error}",
                sidecar.display()
            );
        }
    }
    Ok(true)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve OMP session {}: {error}", path.display()))?;
    let source_path = source
        .to_str()
        .ok_or_else(|| "OMP session path is not valid UTF-8".to_string())?
        .to_string();
    let SessionTree {
        header, summary, ..
    } = read_tree(&source)?;
    let title = summary.explicit_title.or_else(|| {
        summary
            .first_user_message
            .as_deref()
            .map(|message| truncate_summary(message, TITLE_MAX_CHARS))
            .filter(|message| !message.is_empty())
            .or_else(|| path_basename(&header.cwd))
    });
    let summary_text = summary
        .last_message
        .as_deref()
        .map(|message| truncate_summary(message, 160))
        .filter(|message| !message.is_empty());
    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: header.id,
        title,
        summary: summary_text,
        project_dir: (!header.cwd.trim().is_empty()).then(|| header.cwd.clone()),
        created_at: header.timestamp,
        last_active_at: summary.last_active_at.or(header.timestamp),
        source_path: Some(source_path.clone()),
        resume_command: Some(format!(
            "omp --resume {}",
            crate::session_manager::terminal::shell_escape(&shell_friendly_path(&source_path))
        )),
    })
}

/// `fs::canonicalize` prefixes Windows paths with the verbatim `\\?\` marker,
/// which the OMP CLI cannot resolve as a session path. Strip it at the shell
/// boundary while keeping the canonical identity for file operations.
fn shell_friendly_path(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = path.strip_prefix(r"\\?\") {
        local.to_string()
    } else {
        path.to_string()
    }
}

/// Parse an OMP session tree. Any line before the `type:"session"` header is
/// skipped: OMP prefixes files with a mutable `type:"title"` line and has been
/// observed writing other pre-session events (e.g. `session_exit` stubs).
/// Title entries may also appear later and never count as tree entries.
fn read_tree(path: &Path) -> Result<SessionTree, String> {
    validate_file_size(path)?;
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("Failed to open OMP session: {error}"))?,
    );
    let mut header = None;
    let mut parents = HashMap::<String, (Option<String>, usize)>::new();
    let mut latest_id = None;
    let mut legacy_previous_id = None;
    let mut entry_index = 0usize;
    let mut summary = SessionSummary::default();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read OMP session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("title") {
            if let Some(title) = title_entry_text(&value) {
                summary.explicit_title = Some(title);
            }
            continue;
        }
        if header.is_none() {
            if value.get("type").and_then(Value::as_str) == Some("session") {
                header = Some(parse_header(&value)?);
            }
            continue;
        }
        entry_index += 1;
        if entry_index > MAX_TREE_ENTRIES {
            return Err(format!(
                "OMP session exceeds the {MAX_TREE_ENTRIES}-entry safety limit"
            ));
        }
        update_session_summary(&mut summary, &value);
        let version = header
            .as_ref()
            .map_or(1, |item: &SessionHeader| item.version);
        let Some((id, parent_id)) =
            entry_identity(&value, version, entry_index, legacy_previous_id.as_deref())
        else {
            continue;
        };
        if parents
            .insert(id.clone(), (parent_id, entry_index))
            .is_some()
        {
            log::debug!("OMP session contains duplicate entry ID {id}; using the latest entry");
        }
        latest_id = Some(id.clone());
        legacy_previous_id = Some(id);
    }
    let header = header.ok_or_else(|| "OMP session has no valid header".to_string())?;
    let mut active_entry_indexes = HashSet::new();
    let mut visited_ids = HashSet::new();
    let mut current = latest_id;
    while let Some(id) = current {
        if !visited_ids.insert(id.clone()) {
            log::debug!("OMP session tree contains a cycle at entry {id}; stopping traversal");
            break;
        }
        let Some((parent_id, entry_index)) = parents.get(&id) else {
            log::debug!("OMP session tree references missing entry {id}; stopping traversal");
            break;
        };
        active_entry_indexes.insert(*entry_index);
        current = parent_id.clone();
    }
    Ok(SessionTree {
        header,
        active_entry_indexes,
        summary,
    })
}

fn title_entry_text(value: &Value) -> Option<String> {
    value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
}

fn update_session_summary(summary: &mut SessionSummary, value: &Value) {
    if value.get("type").and_then(Value::as_str) == Some("session_info") {
        if let Some(name) = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            summary.explicit_title = Some(name.to_string());
        }
        return;
    }
    let Some((role, content)) = value
        .get("message")
        .filter(|_| value.get("type").and_then(Value::as_str) == Some("message"))
        .and_then(parse_message)
    else {
        return;
    };
    if !matches!(role.as_str(), "user" | "assistant") {
        return;
    }
    let timestamp = value
        .get("message")
        .and_then(|message| message.get("timestamp"))
        .and_then(parse_timestamp_to_ms)
        .or_else(|| value.get("timestamp").and_then(parse_timestamp_to_ms));
    if let Some(timestamp) = timestamp {
        summary.last_active_at = Some(
            summary
                .last_active_at
                .map_or(timestamp, |current| current.max(timestamp)),
        );
    }
    if role == "user" && summary.first_user_message.is_none() {
        summary.first_user_message = Some(content.clone());
    }
    summary.last_message = Some(content);
}

fn read_active_messages(path: &Path, tree: &SessionTree) -> Result<Vec<SessionMessage>, String> {
    validate_file_size(path)?;
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("Failed to open OMP session: {error}"))?,
    );
    let mut messages = Vec::new();
    let mut saw_header = false;
    let mut entry_index = 0usize;
    let mut legacy_previous_id = None;
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read OMP session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // Same skip rule as read_tree: title lines never consume an entry index.
        if value.get("type").and_then(Value::as_str) == Some("title") {
            continue;
        }
        if !saw_header {
            if value.get("type").and_then(Value::as_str) == Some("session") {
                saw_header = true;
            }
            continue;
        }
        entry_index += 1;
        if entry_index > MAX_TREE_ENTRIES {
            return Err(format!(
                "OMP session exceeds the {MAX_TREE_ENTRIES}-entry safety limit"
            ));
        }
        let Some((id, _)) = entry_identity(
            &value,
            tree.header.version,
            entry_index,
            legacy_previous_id.as_deref(),
        ) else {
            continue;
        };
        legacy_previous_id = Some(id.clone());
        if !tree.active_entry_indexes.contains(&entry_index) {
            continue;
        }
        let entry_timestamp = value.get("timestamp").and_then(parse_timestamp_to_ms);
        match value.get("type").and_then(Value::as_str) {
            Some("session_info") => {}
            Some("message") => {
                let Some((role, content)) = value.get("message").and_then(parse_message) else {
                    continue;
                };
                let timestamp = value
                    .get("message")
                    .and_then(|message| message.get("timestamp"))
                    .and_then(parse_timestamp_to_ms)
                    .or(entry_timestamp);
                messages.push(SessionMessage {
                    role,
                    content,
                    ts: timestamp,
                });
            }
            Some("compaction") | Some("branch_summary") => {
                push_system(
                    &mut messages,
                    value
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    entry_timestamp,
                );
            }
            Some("custom_message")
                if value.get("display").and_then(Value::as_bool) != Some(false) =>
            {
                push_system(
                    &mut messages,
                    &value.get("content").map(extract_text).unwrap_or_default(),
                    entry_timestamp,
                );
            }
            _ => {}
        }
    }
    Ok(messages)
}

fn push_system(messages: &mut Vec<SessionMessage>, content: &str, ts: Option<i64>) {
    if !content.trim().is_empty() {
        messages.push(SessionMessage {
            role: "system".to_string(),
            content: content.to_string(),
            ts,
        });
    }
}

fn parse_header(value: &Value) -> Result<SessionHeader, String> {
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return Err("OMP session header must be a type:\"session\" entry".to_string());
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_valid_tree_id(id))
        .ok_or_else(|| "OMP session header has an invalid ID".to_string())?
        .to_string();
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(1);
    if version == 0 {
        return Err(format!("Unsupported OMP session version: {version}"));
    }
    Ok(SessionHeader {
        id,
        cwd: value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        timestamp: value.get("timestamp").and_then(parse_timestamp_to_ms),
        version,
    })
}

fn entry_identity(
    value: &Value,
    version: u64,
    entry_index: usize,
    legacy_previous_id: Option<&str>,
) -> Option<(String, Option<String>)> {
    if version < 2 {
        return Some((
            format!("legacy-{entry_index}"),
            legacy_previous_id.map(str::to_string),
        ));
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_valid_tree_id(id))?
        .to_string();
    let parent_id = match value.get("parentId") {
        None | Some(Value::Null) => None,
        Some(Value::String(parent)) if is_valid_tree_id(parent) => Some(parent.clone()),
        _ => return None,
    };
    Some((id, parent_id))
}

fn parse_message(message: &Value) -> Option<(String, String)> {
    let role = message.get("role").and_then(Value::as_str)?;
    let (display_role, content) = match role {
        "user" | "assistant" => (
            role.to_string(),
            message.get("content").map(extract_text).unwrap_or_default(),
        ),
        "toolResult" => (
            "tool".to_string(),
            message.get("content").map(extract_text).unwrap_or_default(),
        ),
        "bashExecution" => (
            "tool".to_string(),
            format!(
                "$ {}\n{}",
                message
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                message
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            ),
        ),
        "branchSummary" | "compactionSummary" => (
            "system".to_string(),
            message
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ),
        _ => return None,
    };
    (!content.trim().is_empty()).then_some((display_role, content))
}

/// OMP keeps sessions in `<root>/<encoded-project-dir>/<file>.jsonl` only.
fn validate_source_under_root(root: &Path, path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve OMP session root {}: {error}",
            root.display()
        )
    })?;
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve OMP session {}: {error}", path.display()))?;
    if !source.starts_with(&root) {
        return Err(format!(
            "OMP session source is outside the session root: {}",
            path.display()
        ));
    }
    let relative = source.strip_prefix(&root).map_err(|_| {
        format!(
            "OMP session source does not match the project directory layout: {}",
            path.display()
        )
    })?;
    if relative.components().count() != 2 {
        return Err(format!(
            "OMP session source does not match the project directory layout: {}",
            path.display()
        ));
    }
    let metadata = fs::symlink_metadata(&source).map_err(|error| {
        format!(
            "Failed to inspect OMP session {}: {error}",
            source.display()
        )
    })?;
    if !metadata.file_type().is_file()
        || source.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || metadata.len() > MAX_SESSION_BYTES
    {
        return Err(format!("Invalid OMP session file: {}", source.display()));
    }
    Ok((root, source))
}

fn validate_file_size(path: &Path) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("Failed to inspect OMP session: {error}"))?;
    if metadata.len() > MAX_SESSION_BYTES {
        Err(format!(
            "OMP session exceeds the {MAX_SESSION_BYTES}-byte safety limit"
        ))
    } else {
        Ok(())
    }
}

fn collect_jsonl_files(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(project_entries) = fs::read_dir(entry.path()) else {
            continue;
        };
        for project_entry in project_entries.flatten() {
            if !project_entry
                .file_type()
                .is_ok_and(|file_type| file_type.is_file())
            {
                continue;
            }
            let path = project_entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && project_entry
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() <= MAX_SESSION_BYTES)
            {
                output.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_session(project_dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        fs::create_dir_all(project_dir).expect("create project dir");
        let path = project_dir.join(format!("{name}.jsonl"));
        fs::write(&path, lines.join("\n")).expect("write session");
        path
    }

    fn sample_lines(session_id: &str) -> Vec<String> {
        vec![
            format!(
                r#"{{"type":"title","v":1,"title":"重构登录模块","updatedAt":"2026-09-05T12:00:00.000Z"}}"#
            ),
            format!(
                r#"{{"type":"session","version":3,"id":"{session_id}","timestamp":"2026-09-05T12:00:00.000Z","cwd":"D:\\work\\proj"}}"#
            ),
            r#"{"type":"message","id":"aa11bb22","parentId":null,"timestamp":"2026-09-05T12:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"第一条请求"}]}}"#.to_string(),
            r#"{"type":"message","id":"cc33dd44","parentId":"aa11bb22","timestamp":"2026-09-05T12:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"好的"}]}}"#.to_string(),
        ]
    }

    #[test]
    fn scan_accepts_leading_title_line_before_header() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let path = write_session(&root.join("-proj"), "s1", &sample_lines("01a0717c-e4e7"));
        let meta = parse_session(&path).expect("parse session");
        assert_eq!(meta.provider_id, "omp");
        assert_eq!(meta.session_id, "01a0717c-e4e7");
        // The OMP title entry wins over the first user message.
        assert_eq!(meta.title.as_deref(), Some("重构登录模块"));
        assert_eq!(meta.project_dir.as_deref(), Some("D:\\work\\proj"));
    }

    #[test]
    fn scan_tolerates_custom_event_lines_before_header() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let mut lines = sample_lines("sess-tol");
        lines.insert(
            0,
            r#"{"type":"custom","customType":"session_exit","id":"0420d19e","data":{"reason":"sighup"}}"#
                .to_string(),
        );
        let path = write_session(&root.join("-proj"), "st", &lines);
        let meta = parse_session(&path).expect("parse session");
        assert_eq!(meta.session_id, "sess-tol");
        assert_eq!(meta.title.as_deref(), Some("重构登录模块"));
        assert!(meta.resume_command.unwrap().starts_with("omp --resume "));
    }

    #[test]
    fn empty_title_entry_falls_back_to_first_user_message() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let mut lines = sample_lines("sess-2");
        lines[0] =
            r#"{"type":"title","v":1,"title":"","updatedAt":"2026-09-05T12:00:00.000Z","pad":"  "}"#
                .to_string();
        let path = write_session(&root.join("-proj"), "s2", &lines);
        let meta = parse_session(&path).expect("parse session");
        assert_eq!(meta.title.as_deref(), Some("第一条请求"));
    }

    #[test]
    fn load_messages_returns_active_branch_only() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let mut lines = sample_lines("sess-3");
        // Insert an abandoned side branch BEFORE the active tip entry so the
        // last-written entry (assistant "好的") still defines the active path.
        lines.splice(
            3..3,
            [
                r#"{"type":"message","id":"ee55ff66","parentId":"aa11bb22","timestamp":"2026-09-05T12:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"废弃分支"}]}}"#.to_string(),
            ],
        );
        let path = write_session(&root.join("-proj"), "s3", &lines);
        let messages = load_messages_from_root(&root, &path).expect("load messages");
        let contents: Vec<&str> = messages.iter().map(|m| m.content.as_str()).collect();
        assert!(contents.contains(&"第一条请求"));
        assert!(contents.contains(&"好的"));
        assert!(!contents.contains(&"废弃分支"));
    }

    #[test]
    fn delete_removes_jsonl_and_sidecar() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let project = root.join("-proj");
        let path = write_session(&project, "s4", &sample_lines("sess-4"));
        let sidecar = project.join("s4");
        fs::create_dir_all(&sidecar).expect("create sidecar");
        fs::write(sidecar.join("6.bash.log"), "log").expect("write sidecar");

        delete_session(&root, &path, "sess-4").expect("delete session");
        assert!(!path.exists());
        assert!(!sidecar.exists());
    }

    #[test]
    fn delete_rejects_id_mismatch_and_outside_root() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let path = write_session(&root.join("-proj"), "s5", &sample_lines("sess-5"));
        assert!(delete_session(&root, &path, "other-id").is_err());
        let outside = write_session(
            temp.path().join("elsewhere").as_path(),
            "s6",
            &sample_lines("sess-6"),
        );
        assert!(delete_session(&root, &outside, "sess-6").is_err());
    }

    fn load_messages_from_root(root: &Path, path: &Path) -> Result<Vec<SessionMessage>, String> {
        let (_, source) = validate_source_under_root(root, path)?;
        let tree = read_tree(&source)?;
        read_active_messages(&source, &tree)
    }

    #[test]
    fn shell_friendly_path_strips_windows_verbatim_prefix() {
        assert_eq!(
            shell_friendly_path(r"\\?\C:\Users\x\.omp\agent\sessions\s.jsonl"),
            r"C:\Users\x\.omp\agent\sessions\s.jsonl"
        );
        assert_eq!(
            shell_friendly_path(r"\\?\UNC\server\share\s.jsonl"),
            r"\\server\share\s.jsonl"
        );
        assert_eq!(shell_friendly_path("/home/x/s.jsonl"), "/home/x/s.jsonl");
    }
}
