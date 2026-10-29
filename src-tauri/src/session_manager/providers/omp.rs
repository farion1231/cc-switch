//! Oh My Pi 会话：格式沿用 Pi（会话树、活动分支、转录块），差异见 `pi::Dialect`。

use super::pi::{self, Dialect, SessionLayout, SessionRootResolution};
use crate::session_manager::{SessionMessage, SessionMeta};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const OMP: Dialect = Dialect {
    provider_id: "omp",
    label: "Oh My Pi",
    resolve_root: resolve_session_root,
    resume_command,
    title_slot: true,
    model_selector: true,
};

/// omp 与 Pi 共用 `PI_CODING_AGENT_SESSION_DIR`（上游行为）；未设置时按 profile 放在数据目录下。
fn resolve_session_root() -> SessionRootResolution {
    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|raw| !raw.is_empty())
    {
        let root = PathBuf::from(&raw);
        return if root.is_absolute() {
            SessionRootResolution::Available {
                root,
                layout: SessionLayout::Flat,
            }
        } else {
            SessionRootResolution::RequiresProjectContext {
                configured_path: raw.to_string_lossy().into_owned(),
            }
        };
    }
    match crate::omp_config::get_omp_data_dir() {
        Ok(dir) => SessionRootResolution::Available {
            root: dir.join("sessions"),
            layout: SessionLayout::ProjectDirectories,
        },
        Err(error) => SessionRootResolution::Unavailable {
            reason: error.to_string(),
        },
    }
}

pub fn session_roots() -> Vec<PathBuf> {
    pi::session_roots_for(&OMP)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    pi::scan_sessions_for(&OMP)
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    pi::load_messages_for(&OMP, path)
}

fn resume_command(source: &str) -> Result<String, String> {
    use crate::session_manager::terminal::shell_escape;
    let agent = crate::omp_config::get_omp_agent_dir().map_err(|error| error.to_string())?;
    let native_agent =
        crate::omp_config::get_omp_native_agent_dir().map_err(|error| error.to_string())?;
    let profile = if agent == native_agent {
        crate::omp_config::get_omp_active_profile().map_err(|error| error.to_string())?
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
        shell_escape(source)
    ))
}

/// 删会话文件后，再清掉同名的产物目录（子代理会话、草稿）和它自己的 `.bak` 备份。
/// 产物是符号链接时只删链接本身，不碰它指向的目录。
pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let source = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Oh My Pi session: {error}"))?;
    pi::delete_session_for(&OMP, root, &source, session_id)?;
    let artifacts = source.with_extension("");
    match fs::symlink_metadata(&artifacts) {
        Ok(metadata) => {
            let result = if metadata.is_dir() {
                fs::remove_dir_all(&artifacts)
            } else {
                fs::remove_file(&artifacts)
            };
            result.map_err(|error| {
                format!("Oh My Pi session deleted but artifact cleanup failed: {error}")
            })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect Oh My Pi artifacts: {error}")),
    }
    let prefix = format!(
        "{}.",
        source
            .file_name()
            .expect("validated session file")
            .to_string_lossy()
    );
    let parent = source.parent().expect("validated session parent");
    for entry in fs::read_dir(parent).into_iter().flatten().flatten() {
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
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omp_config::test_support::TestAgentDir;
    use crate::session_manager::model::{ContentRef, SessionBlock};
    use serde_json::json;
    use serde_json::Value;
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
        assert!(matches!(
            resolve_session_root(),
            SessionRootResolution::RequiresProjectContext { .. }
        ));
        assert!(load_messages(Path::new("/tmp/any.jsonl"))
            .unwrap_err()
            .contains("cannot be globally resolved"));
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
        let directory = crate::omp_config::get_omp_agent_dir().unwrap();
        let source = directory.join("sessions/project/quote'$(false) name.jsonl");
        let command = resume_command(source.to_str().unwrap()).unwrap();
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
