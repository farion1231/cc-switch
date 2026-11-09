//! Qoder's committed transcript records use the same user/assistant/tool block
//! schema as Claude. Reuse its reader while keeping discovery and roots separate.
use super::{
    claude,
    utils::{FileParseCache, ScanStatus},
};
use crate::{
    app_config::AppType,
    qoder_config,
    session_manager::{SessionMessage, SessionMeta},
};
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

static GLOBAL_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);
static CN_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);

pub fn session_roots(app: &AppType) -> Vec<PathBuf> {
    vec![qoder_config::data_dir(app).join("projects")]
}

pub fn scan_edition_sessions(app: &AppType) -> Vec<SessionMeta> {
    scan_edition_sessions_with_status(app, &mut ScanStatus::default())
}

/// Scans one edition so the index can track each provider id's completeness separately.
pub(crate) fn scan_edition_sessions_with_status(
    app: &AppType,
    status: &mut ScanStatus,
) -> Vec<SessionMeta> {
    let cache: &FileParseCache = if *app == AppType::QoderCn {
        &CN_CACHE
    } else {
        &GLOBAL_CACHE
    };
    scan_root(app, &session_roots(app)[0], cache, status)
}

fn scan_root(
    app: &AppType,
    root: &Path,
    cache: &FileParseCache,
    status: &mut ScanStatus,
) -> Vec<SessionMeta> {
    let mut files = Vec::new();
    for project in status.entries(root) {
        let project = project.path();
        if !status.is_dir(&project) {
            continue;
        }
        // Subagents and session sidecars live one level deeper. They are not
        // standalone conversations and must not be listed a second time.
        for entry in status.entries(&project) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "jsonl")
                && entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                files.push(path);
            }
        }
    }
    let mut sessions = cache.scan_with_status(files, claude::scan_session_file, status);
    for session in &mut sessions {
        session.provider_id = app.as_str().into();
        session.resume_command = Some(resume_command(app, &session.session_id));
    }
    sessions
}

fn resume_command(app: &AppType, id: &str) -> String {
    let (variable, default_directory) = if *app == AppType::QoderCn {
        ("QODERCN_CONFIG_DIR", ".qoder-cn")
    } else {
        ("QODER_CONFIG_DIR", ".qoder")
    };
    let directory = qoder_config::data_dir(app);
    let command = qoder_config::command(app);
    let quote = |text: &str| {
        if cfg!(windows) {
            format!("'{}'", text.replace('\'', "''"))
        } else {
            format!("'{}'", text.replace('\'', "'\"'\"'"))
        }
    };
    let command = format!("{command} --resume {}", quote(id));
    if directory == crate::config::get_home_dir().join(default_directory) {
        command
    } else if cfg!(windows) {
        format!(
            "$env:{variable}={}; {command}",
            quote(&directory.to_string_lossy())
        )
    } else {
        format!(
            "{variable}={} {command}",
            quote(&directory.to_string_lossy())
        )
    }
}

pub fn load_messages(app: &AppType, path: &Path) -> Result<Vec<SessionMessage>, String> {
    let (_, path) = crate::session_manager::content::resolve_under_roots(
        app.as_str(),
        path,
        &session_roots(app),
    )?;
    claude::load_messages(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn qoder_discovers_editions_independently_and_skips_subagents() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("session/subagents")).unwrap();
        let records = [
            json!({"type":"user","sessionId":"session","cwd":"/work/repo","uuid":"u1","timestamp":"2026-10-22T00:00:00Z","message":{"role":"user","content":"Fix Qoder integration"}}),
            json!({"type":"assistant","sessionId":"session","uuid":"a1","parentUuid":"u1","timestamp":"2026-10-22T00:01:00Z","message":{"role":"assistant","content":[{"type":"text","text":"Done"}]}}),
        ];
        let text = records
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(project.join("session.jsonl"), &text).unwrap();
        std::fs::write(project.join("session/subagents/agent-test.jsonl"), &text).unwrap();
        for app in [AppType::Qoder, AppType::QoderCn] {
            let sessions = scan_root(
                &app,
                dir.path(),
                &FileParseCache::new(),
                &mut ScanStatus::default(),
            );
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].provider_id, app.as_str());
            assert_eq!(sessions[0].project_dir.as_deref(), Some("/work/repo"));
            assert!(sessions[0]
                .resume_command
                .as_ref()
                .unwrap()
                .contains(qoder_config::command(&app)));
            let messages = claude::load_messages(&project.join("session.jsonl")).unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[1].content, "Done");
        }
    }
}
