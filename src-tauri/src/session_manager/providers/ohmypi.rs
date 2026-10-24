//! Native OMP session buckets, active journal branches and transcript blocks.

use super::pi::{is_valid_tree_id, MAX_SESSION_BYTES, MAX_TREE_ENTRIES};
use super::pi_blocks::PiTranscript;
use super::utils::{
    for_each_jsonl_value, parse_timestamp_to_ms, path_basename, truncate_summary, TITLE_MAX_CHARS,
};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "ohmypi";

#[derive(Debug, Clone, Copy)]
enum SessionLayout {
    Flat,
    ProjectBuckets,
}

fn resolve_session_root() -> Result<(PathBuf, SessionLayout), String> {
    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|raw| !raw.is_empty())
    {
        let root = PathBuf::from(raw);
        if !root.is_absolute() {
            return Err(format!(
                "Relative Oh My Pi session directory requires a project cwd: {}",
                root.display()
            ));
        }
        return Ok((root, SessionLayout::Flat));
    }
    crate::ohmypi_config::get_ohmypi_data_dir()
        .map(|dir| (dir.join("sessions"), SessionLayout::ProjectBuckets))
        .map_err(|error| error.to_string())
}

pub fn session_roots() -> Vec<PathBuf> {
    match resolve_session_root() {
        Ok((root, _)) => vec![root],
        Err(error) => {
            log::warn!("Oh My Pi session discovery unavailable: {error}");
            Vec::new()
        }
    }
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let (root, layout) = match resolve_session_root() {
        Ok(resolution) => resolution,
        Err(error) => {
            log::warn!("Oh My Pi session discovery unavailable: {error}");
            return Vec::new();
        }
    };
    collect_session_files(&root, layout)
        .into_iter()
        .filter_map(|path| {
            validate_source(&root, &path, layout)
                .and_then(|(_, path)| parse_session(&path))
                .map_err(|error| {
                    log::debug!("Skipping Oh My Pi session {}: {error}", path.display())
                })
                .ok()
        })
        .collect()
}

fn collect_session_files(root: &Path, layout: SessionLayout) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let candidates = match layout {
            SessionLayout::Flat if kind.is_file() => vec![entry],
            SessionLayout::ProjectBuckets if kind.is_dir() => fs::read_dir(entry.path())
                .map(|entries| entries.flatten().collect())
                .unwrap_or_default(),
            _ => continue,
        };
        files.extend(candidates.into_iter().filter_map(|entry| {
            (entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl"))
            .then(|| entry.path())
        }));
    }
    files
}

fn validate_source(
    root: &Path,
    source: &Path,
    layout: SessionLayout,
) -> Result<(PathBuf, PathBuf), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Oh My Pi session root: {error}"))?;
    let source = source
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Oh My Pi session: {error}"))?;
    let relative = source
        .strip_prefix(&root)
        .map_err(|_| "Oh My Pi session source is outside the session root".to_string())?;
    let expected_depth = match layout {
        SessionLayout::Flat => 1,
        SessionLayout::ProjectBuckets => 2,
    };
    if relative.components().count() != expected_depth
        || source
            .extension()
            .is_none_or(|extension| extension != "jsonl")
    {
        return Err(
            "Oh My Pi session source does not match the active directory layout".to_string(),
        );
    }
    let metadata = fs::metadata(&source)
        .map_err(|error| format!("Failed to inspect Oh My Pi session: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_SESSION_BYTES {
        return Err("Invalid or oversized Oh My Pi session file".to_string());
    }
    Ok((root, source))
}

struct SessionTree {
    header: Value,
    title: Option<String>,
    active_indexes: HashSet<usize>,
    last_active_at: Option<i64>,
}

fn read_tree(path: &Path) -> Result<SessionTree, String> {
    let mut header = None;
    let mut title_slot = None;
    let mut parents = HashMap::<String, (Option<String>, usize)>::new();
    let mut latest_id = None;
    let mut legacy_previous_id = None;
    let mut last_active_at = None;
    let mut index = 0usize;
    for_each_jsonl_value(path, |_, value| {
        index += 1;
        if index > MAX_TREE_ENTRIES {
            return Err("Oh My Pi session exceeds the entry limit".to_string());
        }
        if header.is_none() {
            if index == 1 && value.get("type").and_then(Value::as_str) == Some("title") {
                title_slot = value
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                return Ok(());
            }
            if value.get("type").and_then(Value::as_str) != Some("session")
                || !value
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(is_valid_tree_id)
            {
                return Err("Oh My Pi session has no valid header".to_string());
            }
            header = Some(value);
            return Ok(());
        }
        let version = header
            .as_ref()
            .and_then(|header| header.get("version"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let identity = if version < 2 {
            Some((format!("legacy-{index}"), legacy_previous_id.clone()))
        } else {
            value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| is_valid_tree_id(id))
                .and_then(|id| {
                    let parent = match value.get("parentId") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(parent)) if is_valid_tree_id(parent) => {
                            Some(parent.clone())
                        }
                        _ => return None,
                    };
                    Some((id.to_string(), parent))
                })
        };
        if let Some((id, parent)) = identity {
            parents.insert(id.clone(), (parent, index));
            latest_id = Some(id.clone());
            legacy_previous_id = Some(id);
        }
        if let Some(timestamp) = value.get("timestamp").and_then(parse_timestamp_to_ms) {
            last_active_at =
                Some(last_active_at.map_or(timestamp, |current: i64| current.max(timestamp)));
        }
        Ok(())
    })?;
    let header = header.ok_or_else(|| "Oh My Pi session has no valid header".to_string())?;
    let title = title_slot.or_else(|| {
        header
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let mut active_indexes = HashSet::new();
    let mut visited = HashSet::new();
    while let Some(id) = latest_id {
        if !visited.insert(id.clone()) {
            break;
        }
        let Some((parent, index)) = parents.get(&id) else {
            break;
        };
        active_indexes.insert(*index);
        latest_id = parent.clone();
    }
    Ok(SessionTree {
        header,
        title,
        active_indexes,
        last_active_at,
    })
}

fn read_active_messages(path: &Path, tree: &SessionTree) -> Result<Vec<SessionMessage>, String> {
    let mut transcript = PiTranscript::new();
    let mut index = 0usize;
    for_each_jsonl_value(path, |span, mut value| {
        index += 1;
        if !tree.active_indexes.contains(&index) {
            return Ok(());
        }
        // OMP stores a complete model selector; Pi's block mapper uses two fields.
        if value.get("type").and_then(Value::as_str) == Some("model_change") {
            if let Some(model) = value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                if let Some((provider, model_id)) = model.split_once('/') {
                    value["provider"] = Value::String(provider.to_string());
                    value["modelId"] = Value::String(model_id.to_string());
                } else {
                    value["modelId"] = Value::String(model);
                }
            }
        }
        let id = value.get("id").and_then(Value::as_str).map(str::to_string);
        transcript.push_entry(&value, span, id);
        Ok(())
    })?;
    Ok(transcript.finish())
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let (root, layout) = resolve_session_root()?;
    let (_, source) = validate_source(&root, path, layout)?;
    let tree = read_tree(&source)?;
    read_active_messages(&source, &tree)
}

fn resume_command(source: &Path) -> Result<String, String> {
    use crate::session_manager::terminal::shell_escape;
    let agent = crate::ohmypi_config::get_ohmypi_agent_dir().map_err(|error| error.to_string())?;
    let native_agent =
        crate::ohmypi_config::get_ohmypi_native_agent_dir().map_err(|error| error.to_string())?;
    let profile = if agent == native_agent {
        crate::ohmypi_config::get_ohmypi_active_profile().map_err(|error| error.to_string())?
    } else {
        None
    };
    let profile = profile.as_deref().unwrap_or("default");
    let mut assignments = vec![
        format!("OMP_PROFILE={}", shell_escape(profile)),
        format!("PI_PROFILE={}", shell_escape(profile)),
        format!(
            "PI_CODING_AGENT_DIR={}",
            shell_escape(&agent.to_string_lossy())
        ),
    ];
    // Terminal launch is POSIX/macOS. Clear GUI-unset directory variables too:
    // inheriting the terminal's profile/XDG paths would select different native state.
    for variable in [
        "PI_CONFIG_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ] {
        let value = std::env::var(variable).unwrap_or_default();
        assignments.push(format!("{variable}={}", shell_escape(&value)));
    }
    Ok(format!(
        "{} omp --resume {}",
        assignments.join(" "),
        shell_escape(&source.to_string_lossy())
    ))
}

fn parse_session(path: &Path) -> Result<SessionMeta, String> {
    let tree = read_tree(path)?;
    let messages = read_active_messages(path, &tree)?;
    let cwd = tree
        .header
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !cwd.is_empty())
        .map(str::to_string);
    let title = tree
        .title
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.role == "user" && !message.content.trim().is_empty())
                .map(|message| truncate_summary(&message.content, TITLE_MAX_CHARS))
                .or_else(|| cwd.as_deref().and_then(path_basename))
        });
    let summary = messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .map(|message| truncate_summary(&message.content, 160));
    let created_at = tree.header.get("timestamp").and_then(parse_timestamp_to_ms);
    Ok(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: tree.header["id"]
            .as_str()
            .expect("validated session id")
            .to_string(),
        title,
        summary,
        project_dir: cwd,
        created_at,
        last_active_at: tree.last_active_at.or(created_at),
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(resume_command(path)?),
    })
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let (configured_root, layout) = resolve_session_root()?;
    let (validated_root, source) = validate_source(&configured_root, path, layout)?;
    if root.canonicalize().map_err(|error| error.to_string())? != validated_root {
        return Err("Oh My Pi session root changed before deletion".to_string());
    }
    let tree = read_tree(&source)?;
    if tree.header["id"].as_str() != Some(session_id) {
        return Err("Oh My Pi session ID mismatch".to_string());
    }
    let artifacts = source.with_extension("");
    let artifacts_type = match fs::symlink_metadata(&artifacts) {
        Ok(metadata) => Some(metadata.file_type()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Failed to inspect Oh My Pi artifacts: {error}")),
    };
    fs::remove_file(&source)
        .map_err(|error| format!("Failed to delete Oh My Pi session: {error}"))?;
    if let Some(kind) = artifacts_type {
        let result = if kind.is_dir() {
            fs::remove_dir_all(&artifacts)
        } else {
            fs::remove_file(&artifacts)
        };
        result.map_err(|error| {
            format!("Oh My Pi session deleted but artifact cleanup failed: {error}")
        })?;
    }
    let prefix = format!(
        "{}.",
        source
            .file_name()
            .expect("validated session file")
            .to_string_lossy()
    );
    if let Ok(entries) = fs::read_dir(source.parent().expect("validated session parent")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix)
                && name.ends_with(".bak")
                && entry.file_type().is_ok_and(|kind| !kind.is_dir())
            {
                if let Err(error) = fs::remove_file(entry.path()) {
                    log::warn!(
                        "Failed to remove Oh My Pi session backup {}: {error}",
                        entry.path().display()
                    );
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohmypi_config::test_support::TestAgentDir;
    use crate::session_manager::model::{ContentRef, SessionBlock};
    use serde_json::json;
    use serial_test::serial;

    struct SessionDirEnv(Option<std::ffi::OsString>);
    impl SessionDirEnv {
        fn set(path: Option<&Path>) -> Self {
            let previous = std::env::var_os("PI_CODING_AGENT_SESSION_DIR");
            if let Some(path) = path {
                std::env::set_var("PI_CODING_AGENT_SESSION_DIR", path);
            } else {
                std::env::remove_var("PI_CODING_AGENT_SESSION_DIR");
            }
            Self(previous)
        }
    }
    impl Drop for SessionDirEnv {
        fn drop(&mut self) {
            if let Some(previous) = &self.0 {
                std::env::set_var("PI_CODING_AGENT_SESSION_DIR", previous);
            } else {
                std::env::remove_var("PI_CODING_AGENT_SESSION_DIR");
            }
        }
    }
    fn write(path: &Path, records: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    fn header(id: &str) -> Value {
        json!({"type":"session","version":3,"id":id,"timestamp":"2026-10-10T18:00:00Z","cwd":"/repo","title":"Legacy title"})
    }
    fn message(id: &str, parent: Option<&str>, role: &str, text: &str) -> Value {
        json!({"type":"message","id":id,"parentId":parent,"timestamp":"2026-10-10T18:00:01Z","message":{"role":role,"content":[{"type":"text","text":text}]}})
    }

    #[test]
    #[serial]
    fn renders_active_branch_and_structured_tools_with_native_titles() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(None);
        let root = session_roots().remove(0);
        let file = root.join("project").join("2026-10-10_session-id.jsonl");
        write(
            &file,
            &[
                json!({"type":"title","v":1,"title":"Current renamed title","updatedAt":"2026-10-10T18:02:00Z","pad":""}),
                header("session-id"),
                message("u1", None, "user", "Root request"),
                message("old-answer", Some("u1"), "assistant", "Abandoned answer"),
                message("u2", Some("u1"), "user", "New alternative"),
                json!({"type":"model_change","id":"model","parentId":"u2","model":"openai/gpt-5"}),
                json!({"type":"message","id":"a2","parentId":"model","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Check the repository ".repeat(30)},{"type":"toolCall","id":"call1","name":"bash","arguments":{"command":"pwd"}}]}}),
                json!({"type":"message","id":"result","parentId":"a2","message":{"role":"toolResult","toolCallId":"call1","toolName":"bash","content":[{"type":"text","text":"/repo"}],"isError":false}}),
            ],
        );
        let messages = load_messages(&file).unwrap();
        assert!(!messages
            .iter()
            .any(|message| message.id.as_deref() == Some("old-answer")));
        assert_eq!(messages[0].content, "Root request");
        assert_eq!(messages[1].content, "New alternative");
        assert!(messages
            .iter()
            .any(|message| message.content == "openai/gpt-5"));
        assert!(messages
            .iter()
            .flat_map(|message| &message.blocks)
            .any(|block| matches!(block,SessionBlock::ToolCall { id,.. } if id == "call1")));
        assert!(messages.iter().flat_map(|message| &message.blocks).any(
            |block| matches!(block,SessionBlock::ToolResult { preview,.. } if preview == "/repo")
        ));
        assert!(messages.iter().flat_map(|message| &message.blocks).any(|block| matches!(block,SessionBlock::Thinking { full:Some(ContentRef::Jsonl {pointer,..}),.. } if pointer == "/message/content/0/thinking")));
        let sessions = scan_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Current renamed title"));
        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                header("session-id"),
                message("u1", None, "user", "Prompt title")
            ),
        )
        .unwrap();
        assert_eq!(scan_sessions()[0].title.as_deref(), Some("Legacy title"));
    }

    #[test]
    #[serial]
    fn excludes_child_artifacts_and_honors_flat_custom_session_roots() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(None);
        let root = session_roots().remove(0);
        let file = root.join("project/parent.jsonl");
        write(
            &file,
            &[
                header("parent"),
                message("u1", None, "user", "Interactive parent"),
            ],
        );
        let child = root.join("project/parent/worker.jsonl");
        write(
            &child,
            &[
                header("child-id"),
                message("u1", None, "user", "Delegated worker"),
            ],
        );
        assert_eq!(
            scan_sessions()
                .iter()
                .map(|session| &session.session_id)
                .collect::<Vec<_>>(),
            vec!["parent"]
        );
        assert!(load_messages(&child).is_err());
        let custom = tempfile::tempdir().unwrap();
        let _custom_env = SessionDirEnv::set(Some(custom.path()));
        write(&custom.path().join("custom.jsonl"), &[header("custom")]);
        write(
            &custom.path().join("nested/hidden.jsonl"),
            &[header("hidden")],
        );
        assert_eq!(session_roots(), vec![custom.path().to_path_buf()]);
        assert_eq!(scan_sessions()[0].session_id, "custom");
        assert_eq!(scan_sessions().len(), 1);
        assert!(load_messages(&file).is_err());
    }

    #[test]
    #[serial]
    fn relative_custom_root_never_falls_back_to_default_history() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(Some(Path::new(".omp/project-sessions")));
        assert!(resolve_session_root()
            .unwrap_err()
            .contains("requires a project cwd"));
        assert!(session_roots().is_empty());
        assert!(scan_sessions().is_empty());
    }

    #[test]
    #[serial]
    fn deletion_uses_header_identity_and_removes_artifacts_and_only_own_backups() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(None);
        let root = session_roots().remove(0);
        let file = root.join("project/custom-name.jsonl");
        write(&file, &[header("real-header-id")]);
        let artifacts = file.with_extension("");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(artifacts.join("draft.txt"), "Private draft").unwrap();
        write(&artifacts.join("worker.jsonl"), &[header("worker-id")]);
        let backup = file.with_file_name("custom-name.jsonl.123.bak");
        let unrelated = file.with_file_name("other.jsonl.123.bak");
        fs::write(&backup, "own backup").unwrap();
        fs::write(&unrelated, "other backup").unwrap();
        assert!(delete_session(&root, &file, "wrong-id").is_err());
        assert!(file.exists() && artifacts.exists());
        assert!(delete_session(&root, &file, "real-header-id").unwrap());
        assert!(!file.exists() && !artifacts.exists() && !backup.exists());
        assert!(unrelated.exists());
        assert!(scan_sessions().is_empty());
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn artifact_symlink_cleanup_never_removes_its_external_target() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(None);
        let root = session_roots().remove(0);
        let file = root.join("project/safe.jsonl");
        write(&file, &[header("safe")]);
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("keep.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(external.path(), file.with_extension("")).unwrap();
        assert!(delete_session(&root, &file, "safe").unwrap());
        assert!(external.path().join("keep.txt").exists());
        assert!(fs::symlink_metadata(file.with_extension("")).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn resume_preserves_custom_agent_directory_and_quotes_absolute_source() {
        let _agent = TestAgentDir::new();
        let _env = SessionDirEnv::set(None);
        let directory = crate::ohmypi_config::get_ohmypi_agent_dir().unwrap();
        let source = directory.join("sessions/project/quote'$(false) name.jsonl");
        let command = resume_command(&source).unwrap();
        let shell=format!("omp() {{ printf '%s\\n' \"$OMP_PROFILE\" \"$PI_CODING_AGENT_DIR\" \"$@\"; }}; {command}");
        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(shell)
            .env("OMP_PROFILE", "wrong-shell-profile")
            .env("PI_PROFILE", "wrong-shell-profile")
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8(result.stdout)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            vec![
                "default",
                directory.to_str().unwrap(),
                "--resume",
                source.to_str().unwrap()
            ]
        );
    }
}
