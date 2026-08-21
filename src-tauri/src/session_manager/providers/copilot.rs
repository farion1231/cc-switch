use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{Map, Value};

use crate::session_manager::{SessionMessage, SessionMeta};

use super::utils::{parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS};

const PROVIDER_ID: &str = "copilot-byok";
const MAX_SESSION_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_COLLECTION_ITEMS: usize = 10_000;

pub fn session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for user_dir in default_vscode_user_dirs() {
        collect_session_roots(&user_dir, &mut roots);
    }
    roots.sort();
    roots.dedup();
    roots
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    session_roots()
        .into_iter()
        .flat_map(|root| session_files(&root))
        .filter_map(|path| parse_session_meta(&path))
        .collect()
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let state = replay_session(path)?;
    let requests = state
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| "VS Code Copilot session has no requests".to_string())?;
    let mut messages = Vec::new();

    for request in requests.iter().take(MAX_COLLECTION_ITEMS) {
        let user_content = request
            .get("message")
            .map(extract_message_text)
            .unwrap_or_default();
        let request_ts = request.get("timestamp").and_then(parse_timestamp_to_ms);
        if !user_content.trim().is_empty() {
            messages.push(SessionMessage {
                role: "user".to_string(),
                content: user_content,
                ts: request_ts,
            });
        }

        let assistant_content = request
            .get("response")
            .map(extract_response_text)
            .unwrap_or_default();
        if !assistant_content.trim().is_empty() {
            let response_ts = request
                .get("responseTimestamp")
                .and_then(parse_timestamp_to_ms)
                .or(request_ts);
            messages.push(SessionMessage {
                role: "assistant".to_string(),
                content: assistant_content,
                ts: response_ts,
            });
        }
    }

    Ok(messages)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if !path.starts_with(root) {
        return Err(format!(
            "VS Code Copilot session source is outside its session root: {}",
            path.display()
        ));
    }
    if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return Err(format!(
            "Unexpected VS Code Copilot session source: {}",
            path.display()
        ));
    }
    let parent_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    if !matches!(
        parent_name,
        Some("chatSessions" | "emptyWindowChatSessions")
    ) {
        return Err(format!(
            "Unexpected VS Code Copilot session directory: {}",
            path.display()
        ));
    }

    let state = replay_session(path)?;
    let actual_id = session_id_from_state(&state, path);
    if actual_id != session_id {
        return Err(format!(
            "VS Code Copilot session ID mismatch: expected {session_id}, found {actual_id}"
        ));
    }

    fs::remove_file(path).map_err(|error| {
        format!(
            "Failed to delete VS Code Copilot session file {}: {error}",
            path.display()
        )
    })?;
    Ok(true)
}

fn parse_session_meta(path: &Path) -> Option<SessionMeta> {
    let state = replay_session(path).ok()?;
    let session_id = session_id_from_state(&state, path);
    let first_prompt = state
        .get("requests")
        .and_then(Value::as_array)
        .and_then(|requests| {
            requests.iter().find_map(|request| {
                let text = request.get("message").map(extract_message_text)?;
                (!text.trim().is_empty()).then_some(text)
            })
        });
    let title = state
        .get("customTitle")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| truncate_summary(value, TITLE_MAX_CHARS))
        .or_else(|| {
            first_prompt
                .as_deref()
                .map(|value| truncate_summary(value, TITLE_MAX_CHARS))
        });
    let summary = first_prompt
        .as_deref()
        .map(|value| truncate_summary(value, 160));
    let created_at = state.get("creationDate").and_then(parse_timestamp_to_ms);
    let last_request_at = state
        .get("requests")
        .and_then(Value::as_array)
        .and_then(|requests| {
            requests
                .iter()
                .filter_map(|request| {
                    request
                        .get("responseTimestamp")
                        .or_else(|| request.get("timestamp"))
                        .and_then(parse_timestamp_to_ms)
                })
                .max()
        });
    let modified_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id,
        title,
        summary,
        project_dir: workspace_project_dir(path),
        created_at,
        last_active_at: last_request_at.or(modified_at).or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        // VS Code does not expose a stable command-line session resume API.
        resume_command: None,
    })
}

fn default_vscode_user_dirs() -> Vec<PathBuf> {
    let Some(config_dir) = dirs::config_dir() else {
        return Vec::new();
    };
    vec![
        config_dir.join("Code").join("User"),
        config_dir.join("Code - Insiders").join("User"),
    ]
}

fn collect_session_roots(user_dir: &Path, roots: &mut Vec<PathBuf>) {
    collect_workspace_session_roots(&user_dir.join("workspaceStorage"), roots);
    push_existing_dir(
        &user_dir
            .join("globalStorage")
            .join("emptyWindowChatSessions"),
        roots,
    );

    let Ok(profiles) = fs::read_dir(user_dir.join("profiles")) else {
        return;
    };
    for profile in profiles.flatten() {
        let Ok(file_type) = profile.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let profile_dir = profile.path();
        collect_workspace_session_roots(&profile_dir.join("workspaceStorage"), roots);
        push_existing_dir(
            &profile_dir
                .join("globalStorage")
                .join("emptyWindowChatSessions"),
            roots,
        );
    }
}

fn collect_workspace_session_roots(workspace_storage: &Path, roots: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(workspace_storage) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() {
            push_existing_dir(&entry.path().join("chatSessions"), roots);
        }
    }
}

fn push_existing_dir(path: &Path, roots: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        roots.push(path.to_path_buf());
    }
}

fn session_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let path = entry.path();
            (file_type.is_file()
                && !file_type.is_symlink()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("jsonl")))
            .then_some(path)
        })
        .collect()
}

fn replay_session(path: &Path) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect VS Code Copilot session: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "VS Code Copilot session is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_SESSION_FILE_BYTES {
        return Err(format!(
            "VS Code Copilot session exceeds {} MiB: {}",
            MAX_SESSION_FILE_BYTES / 1024 / 1024,
            path.display()
        ));
    }

    let file = File::open(path)
        .map_err(|error| format!("Failed to open VS Code Copilot session: {error}"))?;
    let mut state = Value::Null;
    let mut saw_snapshot = false;
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            format!(
                "Failed to read VS Code Copilot session {}: {error}",
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: Value = serde_json::from_str(&line).map_err(|error| {
            format!(
                "Failed to parse VS Code Copilot session {} at JSONL line {}: {error}",
                path.display(),
                line_index + 1
            )
        })?;
        match record.get("kind").and_then(Value::as_u64) {
            Some(0) => {
                if let Some(value) = record.get("v") {
                    state = value.clone();
                    saw_snapshot = true;
                }
            }
            Some(1) if saw_snapshot => {
                if let (Some(keys), Some(value)) =
                    (record.get("k").and_then(Value::as_array), record.get("v"))
                {
                    set_path(&mut state, keys, value.clone());
                }
            }
            Some(2) if saw_snapshot => {
                if let Some(keys) = record.get("k").and_then(Value::as_array) {
                    let insertion_index = record
                        .get("i")
                        .and_then(Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok());
                    append_path(&mut state, keys, insertion_index, record.get("v"));
                }
            }
            Some(3) if saw_snapshot => {
                if let Some(keys) = record.get("k").and_then(Value::as_array) {
                    delete_path(&mut state, keys);
                }
            }
            _ => {}
        }
    }

    if !saw_snapshot {
        return Err(format!(
            "VS Code Copilot session has no initial snapshot: {}",
            path.display()
        ));
    }
    Ok(state)
}

fn path_index(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < MAX_COLLECTION_ITEMS)
}

fn set_path(target: &mut Value, path: &[Value], value: Value) -> bool {
    let Some((head, tail)) = path.split_first() else {
        *target = value;
        return true;
    };
    if let Some(key) = head.as_str() {
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let child = target
            .as_object_mut()
            .expect("object initialized above")
            .entry(key.to_string())
            .or_insert(Value::Null);
        return set_path(child, tail, value);
    }
    let Some(index) = path_index(head) else {
        return false;
    };
    if !target.is_array() {
        *target = Value::Array(Vec::new());
    }
    let array = target.as_array_mut().expect("array initialized above");
    if array.len() <= index {
        array.resize(index + 1, Value::Null);
    }
    set_path(&mut array[index], tail, value)
}

fn value_at_path_mut<'a>(target: &'a mut Value, path: &[Value]) -> Option<&'a mut Value> {
    let Some((head, tail)) = path.split_first() else {
        return Some(target);
    };
    if let Some(key) = head.as_str() {
        return value_at_path_mut(target.as_object_mut()?.get_mut(key)?, tail);
    }
    let index = path_index(head)?;
    value_at_path_mut(target.as_array_mut()?.get_mut(index)?, tail)
}

fn append_path(
    target: &mut Value,
    path: &[Value],
    insertion_index: Option<usize>,
    value: Option<&Value>,
) {
    if value_at_path_mut(target, path).is_none()
        && !set_path(target, path, Value::Array(Vec::new()))
    {
        return;
    }
    let Some(array) = value_at_path_mut(target, path).and_then(Value::as_array_mut) else {
        return;
    };
    if let Some(index) = insertion_index.filter(|index| *index <= MAX_COLLECTION_ITEMS) {
        array.truncate(index.min(array.len()));
    }
    let Some(value) = value else {
        return;
    };
    let available = MAX_COLLECTION_ITEMS.saturating_sub(array.len());
    match value {
        Value::Array(values) => array.extend(values.iter().take(available).cloned()),
        _ if available > 0 => array.push(value.clone()),
        _ => {}
    }
}

fn delete_path(target: &mut Value, path: &[Value]) -> bool {
    let Some((head, tail)) = path.split_first() else {
        *target = Value::Null;
        return true;
    };
    if tail.is_empty() {
        if let Some(key) = head.as_str() {
            return target
                .as_object_mut()
                .and_then(|object| object.remove(key))
                .is_some();
        }
        let Some(index) = path_index(head) else {
            return false;
        };
        let Some(array) = target.as_array_mut() else {
            return false;
        };
        if index < array.len() {
            array.remove(index);
            return true;
        }
        return false;
    }
    if let Some(key) = head.as_str() {
        return target
            .as_object_mut()
            .and_then(|object| object.get_mut(key))
            .is_some_and(|child| delete_path(child, tail));
    }
    let Some(index) = path_index(head) else {
        return false;
    };
    target
        .as_array_mut()
        .and_then(|array| array.get_mut(index))
        .is_some_and(|child| delete_path(child, tail))
}

fn extract_message_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => join_nonempty(values.iter().map(extract_message_text)),
        Value::Object(object) => {
            for key in ["text", "value", "content", "parts"] {
                if let Some(value) = object.get(key) {
                    let text = extract_message_text(value);
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn extract_response_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => join_nonempty(values.iter().map(extract_response_text)),
        Value::Object(object) => {
            match object.get("kind").and_then(Value::as_str) {
                Some(
                    "autoModeResolution" | "inlineReference" | "mcpServersStarting" | "thinking",
                ) => {
                    return String::new();
                }
                Some("toolInvocationSerialized") => {
                    return object
                        .get("toolId")
                        .and_then(Value::as_str)
                        .map(|tool| format!("[Tool: {tool}]"))
                        .unwrap_or_default();
                }
                _ => {}
            }
            for key in ["text", "value", "content"] {
                if let Some(value) = object.get(key) {
                    let text = extract_response_text(value);
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn join_nonempty(values: impl Iterator<Item = String>) -> String {
    values
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn session_id_from_state(state: &Value, path: &Path) -> String {
    state
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| path.file_stem().and_then(|value| value.to_str()))
        .unwrap_or("unknown")
        .to_string()
}

fn workspace_project_dir(path: &Path) -> Option<String> {
    let session_dir = path.parent()?;
    if session_dir.file_name().and_then(|value| value.to_str()) != Some("chatSessions") {
        return None;
    }
    let workspace_dir = session_dir.parent()?;
    let value: Value =
        serde_json::from_str(&fs::read_to_string(workspace_dir.join("workspace.json")).ok()?)
            .ok()?;
    let raw = value
        .get("folder")
        .or_else(|| value.get("workspace"))?
        .as_str()?;
    if let Ok(url) = url::Url::parse(raw) {
        if url.scheme() == "file" {
            return url
                .to_file_path()
                .ok()
                .map(|path| path.to_string_lossy().to_string());
        }
    }
    (!raw.trim().is_empty()).then(|| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_session(path: &Path) {
        fs::write(
            path,
            concat!(
                r#"{"kind":0,"v":{"sessionId":"session-1","creationDate":1000,"requests":[{"timestamp":2000,"message":{"text":"Hello"},"response":[{"value":"Hi"}]}]}}"#,
                "\n",
                r#"{"kind":2,"k":["requests",0,"response"],"v":[{"value":"there"}]}"#,
                "\n",
                r#"{"kind":1,"k":["customTitle"],"v":"Custom title"}"#,
                "\n",
            ),
        )
        .expect("write session");
    }

    #[test]
    fn replays_jsonl_patches_and_loads_messages() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        write_session(&path);

        let messages = load_messages(&path).expect("load messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "Hi\nthere");

        let metadata = parse_session_meta(&path).expect("metadata");
        assert_eq!(metadata.title.as_deref(), Some("Custom title"));
        assert_eq!(metadata.summary.as_deref(), Some("Hello"));
        assert!(metadata.resume_command.is_none());
    }

    #[test]
    fn deletion_requires_matching_session_id() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("chatSessions");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join("session-1.jsonl");
        write_session(&path);

        let error = delete_session(&root, &path, "wrong").expect_err("mismatch");
        assert!(error.contains("session ID mismatch"));
        assert!(path.exists());

        delete_session(&root, &path, "session-1").expect("delete session");
        assert!(!path.exists());
    }

    #[test]
    fn reads_workspace_file_uri_as_project_dir() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace-id");
        let sessions = workspace.join("chatSessions");
        fs::create_dir_all(&sessions).expect("create sessions");
        let project = temp.path().join("project");
        fs::create_dir_all(&project).expect("create project");
        let uri = url::Url::from_file_path(&project).expect("file URI");
        fs::write(
            workspace.join("workspace.json"),
            serde_json::json!({ "folder": uri.as_str() }).to_string(),
        )
        .expect("write workspace");
        let path = sessions.join("session.jsonl");
        write_session(&path);

        assert_eq!(
            workspace_project_dir(&path),
            Some(project.to_string_lossy().to_string())
        );
    }
}
