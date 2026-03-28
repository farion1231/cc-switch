pub mod providers;
pub mod terminal;

use crate::database::{Database, SessionOverrideKey};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use providers::{claude, codex, gemini, openclaw, opencode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_custom_title: Option<bool>,
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

pub fn scan_sessions_with_overrides(db: &Database) -> Result<Vec<SessionMeta>, String> {
    let mut sessions = scan_sessions();
    apply_title_overrides(&mut sessions, load_session_title_overrides(db));
    Ok(sessions)
}

fn load_session_title_overrides(db: &Database) -> Vec<crate::database::SessionTitleOverride> {
    match db.list_session_title_overrides() {
        Ok(overrides) => overrides,
        Err(err) => {
            log::warn!("Failed to load session title overrides; falling back to none: {err}");
            Vec::new()
        }
    }
}

pub fn rename_session(
    db: &Database,
    provider_id: &str,
    session_id: &str,
    source_path: &str,
    custom_title: Option<&str>,
) -> Result<(), String> {
    db.set_session_custom_title(provider_id, session_id, source_path, custom_title)
        .map_err(|e| format!("Failed to save session title override: {e}"))
}

fn apply_title_overrides(
    sessions: &mut [SessionMeta],
    overrides: Vec<crate::database::SessionTitleOverride>,
) {
    let overrides = overrides
        .into_iter()
        .map(|item| (item.key, item.custom_title))
        .collect::<HashMap<_, _>>();

    for session in sessions.iter_mut() {
        let Some(source_path) = session.source_path.clone() else {
            continue;
        };

        let key = SessionOverrideKey {
            provider_id: session.provider_id.clone(),
            session_id: session.session_id.clone(),
            source_path,
        };

        let Some(custom_title) = overrides.get(&key) else {
            session.has_custom_title = None;
            session.original_title = None;
            continue;
        };

        let original_title = session.title.clone();
        session.title = Some(custom_title.clone());
        session.original_title = original_title;
        session.has_custom_title = Some(true);
    }
}

pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    // OpenCode SQLite sessions use a "sqlite:" prefixed source_path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::load_messages_sqlite(source_path);
    }

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

pub fn delete_session(
    db: Option<&Database>,
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<bool, String> {
    // OpenCode SQLite sessions bypass the file-based deletion path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        let deleted = opencode::delete_session_sqlite(session_id, source_path)?;
        if deleted {
            clear_session_title_override(db, provider_id, session_id, source_path)?;
        }
        return Ok(deleted);
    }

    let root = provider_root(provider_id)?;
    delete_session_with_root(db, provider_id, session_id, Path::new(source_path), &root)
}

fn delete_session_with_root(
    db: Option<&Database>,
    provider_id: &str,
    session_id: &str,
    source_path: &Path,
    root: &Path,
) -> Result<bool, String> {
    let validated_root = canonicalize_existing_path(root, "session root")?;
    let validated_source = canonicalize_existing_path(source_path, "session source")?;

    if !validated_source.starts_with(&validated_root) {
        return Err(format!(
            "Session source path is outside provider root: {}",
            source_path.display()
        ));
    }

    let deleted = match provider_id {
        "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
        "claude" => claude::delete_session(&validated_root, &validated_source, session_id),
        "opencode" => opencode::delete_session(&validated_root, &validated_source, session_id),
        "openclaw" => openclaw::delete_session(&validated_root, &validated_source, session_id),
        "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }?;

    if deleted {
        clear_session_title_override(db, provider_id, session_id, &source_path.to_string_lossy())?;
    }

    Ok(deleted)
}

fn clear_session_title_override(
    db: Option<&Database>,
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<(), String> {
    let Some(db) = db else {
        return Ok(());
    };

    db.set_session_custom_title(provider_id, session_id, source_path, None)
        .map_err(|e| format!("Failed to clear session title override: {e}"))
}

fn provider_root(provider_id: &str) -> Result<PathBuf, String> {
    let root = match provider_id {
        "codex" => crate::codex_config::get_codex_config_dir().join("sessions"),
        "claude" => crate::config::get_claude_config_dir().join("projects"),
        "opencode" => opencode::get_opencode_data_dir(),
        "openclaw" => crate::openclaw_config::get_openclaw_dir().join("agents"),
        "gemini" => crate::gemini_config::get_gemini_dir().join("tmp"),
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };

    Ok(root)
}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("{label} not found: {}", path.display()));
    }

    path.canonicalize()
        .map_err(|e| format!("Failed to resolve {label} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{SessionOverrideKey, SessionTitleOverride};
    use tempfile::tempdir;

    #[test]
    fn rejects_source_path_outside_provider_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let source = outside.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err = delete_session_with_root(None, "codex", "session-1", &source, root.path())
            .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider root"));
    }

    #[test]
    fn rejects_missing_source_path() {
        let root = tempdir().expect("tempdir");
        let missing = root.path().join("missing.jsonl");

        let err = delete_session_with_root(None, "codex", "session-1", &missing, root.path())
            .expect_err("expected missing source path to fail");

        assert!(err.contains("session source not found"));
    }

    #[test]
    fn applies_custom_title_and_preserves_original_title() {
        let mut sessions = vec![SessionMeta {
            provider_id: "codex".to_string(),
            session_id: "session-1".to_string(),
            title: Some("Original title".to_string()),
            original_title: None,
            has_custom_title: None,
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: Some("C:\\sessions\\session-1.jsonl".to_string()),
            resume_command: None,
        }];

        apply_title_overrides(
            &mut sessions,
            vec![SessionTitleOverride {
                key: SessionOverrideKey {
                    provider_id: "codex".to_string(),
                    session_id: "session-1".to_string(),
                    source_path: "C:\\sessions\\session-1.jsonl".to_string(),
                },
                custom_title: "Pinned session".to_string(),
            }],
        );

        assert_eq!(sessions[0].title.as_deref(), Some("Pinned session"));
        assert_eq!(sessions[0].original_title.as_deref(), Some("Original title"));
        assert_eq!(sessions[0].has_custom_title, Some(true));
    }

    #[test]
    fn load_session_title_overrides_falls_back_on_query_error() {
        let db = Database::memory().expect("create memory db");
        {
            let conn = db.conn.lock().expect("lock conn");
            conn.execute("DROP TABLE session_overrides", [])
                .expect("drop helper table");
        }

        let overrides = load_session_title_overrides(&db);
        assert!(overrides.is_empty(), "expected empty fallback overrides");
    }

    #[test]
    fn delete_session_clears_title_override_metadata() {
        let db = Database::memory().expect("create memory db");
        let root = tempdir().expect("tempdir");
        let source = root.path().join("session.jsonl");
        std::fs::write(
            &source,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n"
            ),
        )
        .expect("write source");

        db.set_session_custom_title("codex", "session-1", &source.to_string_lossy(), Some("Pinned"))
            .expect("save override");

        let deleted = delete_session_with_root(
            Some(&db),
            "codex",
            "session-1",
            &source,
            root.path(),
        )
        .expect("delete session");

        assert!(deleted);
        assert!(
            db.get_session_custom_title("codex", "session-1", &source.to_string_lossy())
                .expect("read override")
                .is_none(),
            "override should be removed after session deletion"
        );
    }
}
