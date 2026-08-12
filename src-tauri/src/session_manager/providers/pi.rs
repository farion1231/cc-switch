use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, truncate_summary, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "pi";
const MAX_TREE_ENTRIES: usize = 500_000;
const MAX_TREE_ID_BYTES: usize = 256;
const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionLayout {
    Flat,
    ProjectDirectories,
}

#[derive(Debug, PartialEq, Eq)]
enum SessionRootResolution {
    Available {
        root: PathBuf,
        layout: SessionLayout,
    },
    RequiresProjectContext {
        configured_path: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PiSessionDiscovery {
    Available,
    RequiresProjectContext {
        #[serde(rename = "configuredPath")]
        configured_path: String,
    },
    Unavailable {
        reason: String,
    },
}

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
    explicit_name: Option<Option<String>>,
    last_active_at: Option<i64>,
}

/// Pi keeps a relative `sessionDir` relative through SessionManager creation;
/// its file operations therefore depend on the launching process cwd. A global
/// session browser has no authoritative launch cwd, so relative values are
/// deliberately non-enumerable and never fall back to another root.
pub fn session_roots() -> Vec<PathBuf> {
    match resolve_session_root() {
        SessionRootResolution::Available { root, .. } => vec![root],
        SessionRootResolution::RequiresProjectContext { .. }
        | SessionRootResolution::Unavailable { .. } => Vec::new(),
    }
}

pub fn session_discovery() -> PiSessionDiscovery {
    match resolve_session_root() {
        SessionRootResolution::Available { .. } => PiSessionDiscovery::Available,
        SessionRootResolution::RequiresProjectContext { configured_path } => {
            PiSessionDiscovery::RequiresProjectContext { configured_path }
        }
        SessionRootResolution::Unavailable { reason } => PiSessionDiscovery::Unavailable { reason },
    }
}

fn resolve_session_root() -> SessionRootResolution {
    let home = crate::config::get_home_dir();
    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        if !raw.is_empty() {
            return classify_configured_session_dir(
                raw.to_string_lossy().as_ref(),
                &home,
                "environment",
            );
        }
    }

    match crate::pi_config::read_pi_native_defaults() {
        Ok(defaults) => {
            if let Some(value) = defaults.session_dir.filter(|value| !value.is_empty()) {
                return classify_configured_session_dir(&value, &home, "settings");
            }
        }
        Err(error) => {
            return SessionRootResolution::Unavailable {
                reason: error.to_string(),
            };
        }
    }

    match crate::pi_config::get_pi_agent_dir() {
        Ok(agent_dir) => SessionRootResolution::Available {
            root: agent_dir.join("sessions"),
            layout: SessionLayout::ProjectDirectories,
        },
        Err(error) => SessionRootResolution::Unavailable {
            reason: error.to_string(),
        },
    }
}

fn classify_configured_session_dir(
    value: &str,
    home: &Path,
    source: &'static str,
) -> SessionRootResolution {
    match resolve_global_session_dir(value, home) {
        Some(root) => match fs::metadata(&root) {
            Ok(metadata) if !metadata.is_dir() => SessionRootResolution::Unavailable {
                reason: format!(
                    "Configured Pi session directory from {source} is not a directory: {}",
                    root.display()
                ),
            },
            Ok(_) => match fs::read_dir(&root) {
                Ok(_) => SessionRootResolution::Available {
                    root,
                    layout: SessionLayout::Flat,
                },
                Err(error) => SessionRootResolution::Unavailable {
                    reason: format!(
                        "Configured Pi session directory from {source} is not readable ({}): {error}",
                        root.display()
                    ),
                },
            },
            Err(error) => SessionRootResolution::Unavailable {
                reason: format!(
                    "Configured Pi session directory from {source} is unavailable ({}): {error}",
                    root.display()
                ),
            },
        },
        None => SessionRootResolution::RequiresProjectContext {
            configured_path: value.to_string(),
        },
    }
}

fn resolve_global_session_dir(value: &str, home: &Path) -> Option<PathBuf> {
    let path = if value == "~" {
        home.to_path_buf()
    } else if let Some(suffix) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        home.join(suffix)
    } else {
        PathBuf::from(value)
    };
    path.is_absolute().then_some(path)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    match resolve_session_root() {
        SessionRootResolution::Available { root, layout, .. } => {
            scan_sessions_in_root(&root, layout)
        }
        SessionRootResolution::RequiresProjectContext {
            configured_path, ..
        } => {
            log::warn!(
                "Pi sessionDir '{configured_path}' requires a project cwd and cannot be globally enumerated"
            );
            Vec::new()
        }
        SessionRootResolution::Unavailable { reason } => {
            log::warn!("Pi session discovery unavailable: {reason}");
            Vec::new()
        }
    }
}

fn scan_sessions_in_root(root: &Path, layout: SessionLayout) -> Vec<SessionMeta> {
    let mut files = Vec::new();
    collect_jsonl_files(root, layout, &mut files);
    files
        .into_iter()
        .filter_map(|path| match parse_session(&path) {
            Ok(session) => Some(session),
            Err(error) => {
                log::debug!("Skipping invalid Pi session {}: {error}", path.display());
                None
            }
        })
        .collect()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    match resolve_session_root() {
        SessionRootResolution::Available { root, layout, .. } => {
            load_messages_with_layout(&root, path, layout)
        }
        SessionRootResolution::RequiresProjectContext { .. } => {
            Err("Relative Pi sessionDir cannot be globally resolved".to_string())
        }
        SessionRootResolution::Unavailable { reason } => Err(reason),
    }
}

fn load_messages_with_layout(
    root: &Path,
    path: &Path,
    layout: SessionLayout,
) -> Result<Vec<SessionMessage>, String> {
    let (_, source) = validate_source_under_root(root, path, layout)?;
    let tree = read_tree(&source)?;
    read_active_messages(&source, &tree)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let layout = layout_for_current_root(root)?;
    delete_session_with_layout(root, path, session_id, layout)
}

fn delete_session_with_layout(
    root: &Path,
    path: &Path,
    session_id: &str,
    layout: SessionLayout,
) -> Result<bool, String> {
    if !is_valid_tree_id(session_id) {
        return Err("Invalid Pi session ID".to_string());
    }
    let (_, source) = validate_source_under_root(root, path, layout)?;
    let tree = read_tree(&source)?;
    if tree.header.id != session_id {
        return Err(format!(
            "Pi session ID mismatch: expected {session_id}, found {}",
            tree.header.id
        ));
    }
    fs::remove_file(&source)
        .map_err(|error| format!("Failed to delete Pi session {}: {error}", source.display()))?;
    Ok(true)
}

fn layout_for_current_root(root: &Path) -> Result<SessionLayout, String> {
    let (configured_root, layout) = match resolve_session_root() {
        SessionRootResolution::Available { root, layout, .. } => (root, layout),
        SessionRootResolution::RequiresProjectContext { .. } => {
            return Err("Relative Pi sessionDir cannot be globally resolved".to_string());
        }
        SessionRootResolution::Unavailable { reason } => return Err(reason),
    };
    let configured_root = configured_root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve Pi session root {}: {error}",
            configured_root.display()
        )
    })?;
    let requested_root = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve Pi session root {}: {error}",
            root.display()
        )
    })?;
    if configured_root != requested_root {
        return Err("Pi session root changed before deletion".to_string());
    }
    Ok(layout)
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Pi session {}: {error}", path.display()))?;
    let source_path = source
        .to_str()
        .ok_or_else(|| "Pi session path is not valid UTF-8".to_string())?
        .to_string();
    let SessionTree {
        header, summary, ..
    } = read_tree(&source)?;
    let title = summary.explicit_name.flatten().or_else(|| {
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
            "pi --session {}",
            crate::session_manager::terminal::shell_escape(&source_path)
        )),
    })
}

fn read_tree(path: &Path) -> Result<SessionTree, String> {
    validate_file_size(path)?;
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("Failed to open Pi session: {error}"))?,
    );
    let mut header = None;
    let mut parents = HashMap::<String, (Option<String>, usize)>::new();
    let mut latest_id = None;
    let mut legacy_previous_id = None;
    let mut entry_index = 0usize;
    let mut summary = SessionSummary::default();
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read Pi session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if header.is_none() {
            header = Some(parse_header(&value)?);
            continue;
        }
        entry_index += 1;
        if entry_index > MAX_TREE_ENTRIES {
            return Err(format!(
                "Pi session exceeds the {MAX_TREE_ENTRIES}-entry safety limit"
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
            log::debug!("Pi session contains duplicate entry ID {id}; using the latest entry");
        }
        latest_id = Some(id.clone());
        legacy_previous_id = Some(id);
    }
    let header = header.ok_or_else(|| "Pi session has no valid header".to_string())?;
    let mut active_entry_indexes = HashSet::new();
    let mut visited_ids = HashSet::new();
    let mut current = latest_id;
    while let Some(id) = current {
        if !visited_ids.insert(id.clone()) {
            log::debug!("Pi session tree contains a cycle at entry {id}; stopping traversal");
            break;
        }
        let Some((parent_id, entry_index)) = parents.get(&id) else {
            log::debug!("Pi session tree references missing entry {id}; stopping traversal");
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

fn update_session_summary(summary: &mut SessionSummary, value: &Value) {
    if value.get("type").and_then(Value::as_str) == Some("session_info") {
        summary.explicit_name = Some(
            value
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        );
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
        File::open(path).map_err(|error| format!("Failed to open Pi session: {error}"))?,
    );
    let mut messages = Vec::new();
    let mut saw_header = false;
    let mut entry_index = 0usize;
    let mut legacy_previous_id = None;
    for line in reader.lines() {
        let line = line.map_err(|error| format!("Failed to read Pi session: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if !saw_header {
            if value.get("type").and_then(Value::as_str) == Some("session") {
                saw_header = true;
            }
            continue;
        }
        entry_index += 1;
        if entry_index > MAX_TREE_ENTRIES {
            return Err(format!(
                "Pi session exceeds the {MAX_TREE_ENTRIES}-entry safety limit"
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
        return Err("Pi session header must be the first valid JSON entry".to_string());
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_valid_tree_id(id))
        .ok_or_else(|| "Pi session header has an invalid ID".to_string())?
        .to_string();
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(1);
    if version == 0 {
        return Err(format!("Unsupported Pi session version: {version}"));
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

fn validate_source_under_root(
    root: &Path,
    path: &Path,
    layout: SessionLayout,
) -> Result<(PathBuf, PathBuf), String> {
    let root = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve Pi session root {}: {error}",
            root.display()
        )
    })?;
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Pi session {}: {error}", path.display()))?;
    if !source.starts_with(&root) {
        return Err(format!(
            "Pi session source is outside the session root: {}",
            path.display()
        ));
    }
    if !matches_session_layout(&root, &source, layout) {
        return Err(format!(
            "Pi session source does not match the active directory layout: {}",
            path.display()
        ));
    }
    let metadata = fs::symlink_metadata(&source)
        .map_err(|error| format!("Failed to inspect Pi session {}: {error}", source.display()))?;
    if !metadata.file_type().is_file()
        || source.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || metadata.len() > MAX_SESSION_BYTES
    {
        return Err(format!("Invalid Pi session file: {}", source.display()));
    }
    Ok((root, source))
}

fn matches_session_layout(root: &Path, source: &Path, layout: SessionLayout) -> bool {
    let Ok(relative) = source.strip_prefix(root) else {
        return false;
    };
    let depth = relative.components().count();
    match layout {
        SessionLayout::Flat => depth == 1,
        SessionLayout::ProjectDirectories => depth == 2,
    }
}

fn validate_file_size(path: &Path) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("Failed to inspect Pi session: {error}"))?;
    if metadata.len() > MAX_SESSION_BYTES {
        Err(format!(
            "Pi session exceeds the {MAX_SESSION_BYTES}-byte safety limit"
        ))
    } else {
        Ok(())
    }
}

fn is_valid_tree_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_TREE_ID_BYTES
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn collect_jsonl_files(root: &Path, layout: SessionLayout, output: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        match layout {
            SessionLayout::Flat if file_type.is_file() => {
                push_jsonl_file(&entry, output);
            }
            SessionLayout::ProjectDirectories if file_type.is_dir() => {
                let Ok(project_entries) = fs::read_dir(entry.path()) else {
                    continue;
                };
                for project_entry in project_entries.flatten() {
                    if project_entry
                        .file_type()
                        .is_ok_and(|file_type| file_type.is_file())
                    {
                        push_jsonl_file(&project_entry, output);
                    }
                }
            }
            SessionLayout::Flat | SessionLayout::ProjectDirectories => {}
        }
    }
}

fn push_jsonl_file(entry: &fs::DirEntry, output: &mut Vec<PathBuf>) {
    let path = entry.path();
    if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        && entry
            .metadata()
            .is_ok_and(|metadata| metadata.len() <= MAX_SESSION_BYTES)
    {
        output.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_session_header(path: &Path, id: &str) {
        fs::create_dir_all(path.parent().expect("session parent")).expect("create session parent");
        fs::write(
            path,
            format!("{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"cwd\":\"/work\"}}\n"),
        )
        .expect("write session");
    }

    #[test]
    fn latest_leaf_defines_the_active_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("tree.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"question\"}}\n\
             {\"type\":\"message\",\"id\":\"dead\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"abandoned\"}}\n\
             {\"type\":\"message\",\"id\":\"live\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"active\"}}\n",
        )
        .expect("session");
        let messages =
            load_messages_with_layout(&root, &path, SessionLayout::Flat).expect("messages");
        assert_eq!(
            messages
                .into_iter()
                .map(|message| message.content)
                .collect::<Vec<_>>(),
            vec!["question", "active"]
        );
    }

    #[test]
    fn future_session_versions_are_read_with_current_semantics() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("future.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"version\":4,\"id\":\"session-1\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"message-1\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"future\"}}\n",
        )
        .expect("session");

        let session = parse_session(&path).expect("parse future session");
        assert_eq!(session.summary.as_deref(), Some("future"));
    }

    #[test]
    fn duplicate_entry_ids_use_the_latest_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("duplicate.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"root\"}}\n\
             {\"type\":\"message\",\"id\":\"duplicate\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"stale\"}}\n\
             {\"type\":\"message\",\"id\":\"duplicate\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"latest\"}}\n",
        )
        .expect("session");

        let messages =
            load_messages_with_layout(&root, &path, SessionLayout::Flat).expect("messages");
        assert_eq!(
            messages
                .into_iter()
                .map(|message| message.content)
                .collect::<Vec<_>>(),
            vec!["root", "latest"]
        );
    }

    #[test]
    fn missing_parents_and_cycles_stop_at_the_readable_branch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");

        let missing = root.join("missing.jsonl");
        fs::write(
            &missing,
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"orphan\",\"parentId\":\"missing\",\"message\":{\"role\":\"assistant\",\"content\":\"visible orphan\"}}\n",
        )
        .expect("missing-parent session");
        let messages = load_messages_with_layout(&root, &missing, SessionLayout::Flat)
            .expect("missing parent is tolerated");
        assert_eq!(messages[0].content, "visible orphan");

        let cycle = root.join("cycle.jsonl");
        fs::write(
            &cycle,
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-2\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"a\",\"parentId\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"a\"}}\n\
             {\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"b\"}}\n",
        )
        .expect("cyclic session");
        let messages = load_messages_with_layout(&root, &cycle, SessionLayout::Flat)
            .expect("cycle is tolerated");
        assert_eq!(
            messages
                .into_iter()
                .map(|message| message.content)
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn global_name_and_malformed_line_follow_pi_semantics() {
        // Pi keeps the latest global session_info even when its branch is
        // inactive, and skips malformed JSONL lines when opening a session.
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("captured.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/work\"}\n\
             {\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"root\"}}\n\
             {not valid json\n\
             {\"type\":\"session_info\",\"id\":\"dead-name\",\"parentId\":\"root\",\"name\":\"Abandoned branch name\"}\n\
             {\"type\":\"message\",\"id\":\"dead\",\"parentId\":\"dead-name\",\"message\":{\"role\":\"assistant\",\"content\":\"abandoned\"}}\n\
             {\"type\":\"message\",\"id\":\"live\",\"parentId\":\"root\",\"message\":{\"role\":\"user\",\"content\":\"active branch\"}}\n",
        )
        .expect("captured session");

        let session = parse_session(&path).expect("parse capture semantics");
        assert_eq!(session.title.as_deref(), Some("Abandoned branch name"));
        assert_eq!(session.summary.as_deref(), Some("active branch"));
    }

    #[test]
    fn relative_root_is_explicitly_non_enumerable() {
        assert_eq!(
            resolve_global_session_dir(".pi/sessions", Path::new("/home/pi")),
            None
        );
        assert_eq!(
            classify_configured_session_dir(".pi/sessions", Path::new("/home/pi"), "settings"),
            SessionRootResolution::RequiresProjectContext {
                configured_path: ".pi/sessions".to_string(),
            }
        );
    }

    #[test]
    fn unavailable_explicit_roots_are_not_reported_as_empty_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("missing");
        assert!(matches!(
            classify_configured_session_dir(
                missing.to_string_lossy().as_ref(),
                temp.path(),
                "settings"
            ),
            SessionRootResolution::Unavailable { .. }
        ));

        let file = temp.path().join("sessions.jsonl");
        fs::write(&file, b"").expect("session file");
        assert!(matches!(
            classify_configured_session_dir(
                file.to_string_lossy().as_ref(),
                temp.path(),
                "settings"
            ),
            SessionRootResolution::Unavailable { .. }
        ));

        let directory = temp.path().join("sessions");
        fs::create_dir(&directory).expect("session directory");
        assert!(matches!(
            classify_configured_session_dir(
                directory.to_string_lossy().as_ref(),
                temp.path(),
                "settings"
            ),
            SessionRootResolution::Available { root, .. } if root == directory
        ));
    }

    #[test]
    fn v3_shape_round_trips_all_consumed_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("captured.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"version\":3,\"id\":\"cc-switch-capture-session\",\"timestamp\":\"2023-11-14T22:13:20.000Z\",\"cwd\":\"/work/captured\",\"parentSession\":null}\n\
             {\"type\":\"session_info\",\"id\":\"00000000-0000-7000-8000-000000000001\",\"parentId\":null,\"timestamp\":\"2023-11-14T22:13:20.100Z\",\"name\":\"Captured session\"}\n\
             {\"type\":\"message\",\"id\":\"00000000-0000-7000-8000-000000000002\",\"parentId\":\"00000000-0000-7000-8000-000000000001\",\"timestamp\":\"2023-11-14T22:13:20.200Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"captured question\"}],\"timestamp\":1700000000000}}\n\
             {\"type\":\"message\",\"id\":\"00000000-0000-7000-8000-000000000003\",\"parentId\":\"00000000-0000-7000-8000-000000000002\",\"timestamp\":\"2023-11-14T22:13:21.200Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"captured answer\"}],\"api\":\"openai-responses\",\"provider\":\"capture\",\"model\":\"capture-model\",\"usage\":{\"input\":1,\"output\":1,\"cacheRead\":0,\"cacheWrite\":0,\"totalTokens\":2,\"cost\":{\"input\":0,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0,\"total\":0}},\"stopReason\":\"stop\",\"timestamp\":1700000001000}}\n",
        )
        .expect("captured session");

        let session = parse_session(&path).expect("parse capture-generated session");
        assert_eq!(session.session_id, "cc-switch-capture-session");
        assert_eq!(session.title.as_deref(), Some("Captured session"));
        assert_eq!(session.summary.as_deref(), Some("captured answer"));
        assert_eq!(session.project_dir.as_deref(), Some("/work/captured"));
        assert_eq!(session.created_at, Some(1_700_000_000_000));
        // Pi's session picker prefers the message timestamp over the enclosing
        // entry timestamp when both are present.
        assert_eq!(session.last_active_at, Some(1_700_000_001_000));
        // Pi resumes a session by its exact file path.
        assert!(session
            .resume_command
            .as_deref()
            .is_some_and(|command| command.starts_with("pi --session ")));

        let messages =
            load_messages_with_layout(&root, &path, SessionLayout::Flat).expect("load messages");
        assert_eq!(
            messages
                .iter()
                .map(|message| (message.role.as_str(), message.content.as_str(), message.ts))
                .collect::<Vec<_>>(),
            vec![
                ("user", "captured question", Some(1_700_000_000_000)),
                ("assistant", "captured answer", Some(1_700_000_001_000)),
            ]
        );
    }

    #[test]
    fn deletion_requires_containment_and_matching_header_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).expect("root");
        let path = root.join("session.jsonl");
        write_session_header(&path, "session-1");
        assert!(delete_session_with_layout(&root, &path, "other", SessionLayout::Flat).is_err());
        assert!(path.exists());
        assert!(
            delete_session_with_layout(&root, &path, "session-1", SessionLayout::Flat)
                .expect("delete")
        );
    }

    #[test]
    fn scanning_respects_the_active_directory_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let flat = root.join("flat.jsonl");
        let project = root.join("project").join("project.jsonl");
        write_session_header(&flat, "flat-session");
        write_session_header(&project, "project-session");

        let flat_sessions = scan_sessions_in_root(&root, SessionLayout::Flat);
        assert_eq!(flat_sessions.len(), 1);
        assert_eq!(flat_sessions[0].session_id, "flat-session");

        let project_sessions = scan_sessions_in_root(&root, SessionLayout::ProjectDirectories);
        assert_eq!(project_sessions.len(), 1);
        assert_eq!(project_sessions[0].session_id, "project-session");
    }

    #[test]
    fn deletion_rejects_files_outside_the_active_directory_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("sessions");
        let flat = root.join("flat.jsonl");
        let project = root.join("project").join("project.jsonl");
        write_session_header(&flat, "flat-session");
        write_session_header(&project, "project-session");

        assert!(delete_session_with_layout(
            &root,
            &project,
            "project-session",
            SessionLayout::Flat
        )
        .is_err());
        assert!(project.exists());

        assert!(delete_session_with_layout(
            &root,
            &flat,
            "flat-session",
            SessionLayout::ProjectDirectories
        )
        .is_err());
        assert!(flat.exists());

        assert!(delete_session_with_layout(
            &root,
            &project,
            "project-session",
            SessionLayout::ProjectDirectories
        )
        .expect("delete project session"));
    }
}
