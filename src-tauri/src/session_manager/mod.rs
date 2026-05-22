pub mod providers;
pub mod terminal;

use crate::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use providers::{
    claude, codex, gemini, hermes, openclaw, opencode, truncate_summary, TITLE_MAX_CHARS,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SessionTitleOverride {
    provider_id: String,
    session_id: String,
    source_path: String,
    title: String,
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

pub fn scan_sessions_with_title_overrides(db: &Database) -> Result<Vec<SessionMeta>, String> {
    apply_title_overrides(db, scan_sessions())
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

pub fn rename_session(
    db: &Database,
    provider_id: &str,
    session_id: &str,
    source_path: &str,
    title: &str,
) -> Result<SessionMeta, String> {
    if source_path.trim().is_empty() {
        return Err("Session source path is required".to_string());
    }

    let title = truncate_summary(title, TITLE_MAX_CHARS);
    if title.is_empty() {
        return Err("Session title cannot be empty".to_string());
    }
    let target = scan_sessions()
        .into_iter()
        .find(|session| {
            session.provider_id == provider_id
                && session.session_id == session_id
                && session.source_path.as_deref() == Some(source_path)
        })
        .ok_or_else(|| "Session not found".to_string())?;

    upsert_title_override(db, provider_id, session_id, source_path, &title)?;

    Ok(SessionMeta {
        title: Some(title),
        ..target
    })
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

    let root = provider_root(provider_id)?;
    delete_session_with_root(provider_id, session_id, Path::new(source_path), &root)
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

fn apply_title_overrides(
    db: &Database,
    mut sessions: Vec<SessionMeta>,
) -> Result<Vec<SessionMeta>, String> {
    let overrides = load_title_overrides(db)?;
    if overrides.is_empty() {
        return Ok(sessions);
    }

    let session_keys: HashSet<(String, String, String)> = sessions
        .iter()
        .filter_map(|session| {
            session.source_path.as_ref().map(|source_path| {
                (
                    session.provider_id.clone(),
                    session.session_id.clone(),
                    source_path.clone(),
                )
            })
        })
        .collect();
    let mut by_key: HashMap<(String, String, String), String> = HashMap::new();
    let mut active_overrides = Vec::new();
    let mut pruned_stale = false;

    for entry in overrides {
        let key = (
            entry.provider_id.clone(),
            entry.session_id.clone(),
            entry.source_path.clone(),
        );
        if session_keys.contains(&key) {
            by_key.insert(key, entry.title.clone());
            active_overrides.push(entry);
        } else {
            pruned_stale = true;
        }
    }

    for session in &mut sessions {
        if let Some(source_path) = session.source_path.as_ref() {
            let key = (
                session.provider_id.clone(),
                session.session_id.clone(),
                source_path.clone(),
            );
            if let Some(title) = by_key.get(&key) {
                session.title = Some(title.clone());
            }
        }
    }

    if pruned_stale {
        save_title_overrides(db, &active_overrides)?;
    }

    Ok(sessions)
}

fn load_title_overrides(db: &Database) -> Result<Vec<SessionTitleOverride>, String> {
    let raw = db
        .get_setting("session_title_overrides")
        .map_err(|e| e.to_string())?;

    match raw {
        Some(value) if !value.trim().is_empty() => serde_json::from_str(&value)
            .map_err(|e| format!("Failed to parse session title overrides from settings: {e}")),
        _ => Ok(Vec::new()),
    }
}

fn save_title_overrides(db: &Database, overrides: &[SessionTitleOverride]) -> Result<(), String> {
    let json = serde_json::to_string(overrides)
        .map_err(|e| format!("Failed to serialize session title overrides: {e}"))?;
    db.set_setting("session_title_overrides", &json)
        .map_err(|e| e.to_string())
}

fn upsert_title_override(
    db: &Database,
    provider_id: &str,
    session_id: &str,
    source_path: &str,
    title: &str,
) -> Result<(), String> {
    let mut overrides = load_title_overrides(db)?;

    if let Some(existing) = overrides.iter_mut().find(|entry| {
        entry.provider_id == provider_id
            && entry.session_id == session_id
            && entry.source_path == source_path
    }) {
        existing.title = title.to_string();
    } else {
        overrides.push(SessionTitleOverride {
            provider_id: provider_id.to_string(),
            session_id: session_id.to_string(),
            source_path: source_path.to_string(),
            title: title.to_string(),
        });
    }

    save_title_overrides(db, &overrides)
}

fn delete_session_with_root(
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

    match provider_id {
        "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
        "claude" => claude::delete_session(&validated_root, &validated_source, session_id),
        "opencode" => opencode::delete_session(&validated_root, &validated_source, session_id),
        "openclaw" => openclaw::delete_session(&validated_root, &validated_source, session_id),
        "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
        "hermes" => hermes::delete_session(&validated_root, &validated_source, session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

fn provider_root(provider_id: &str) -> Result<PathBuf, String> {
    let root = match provider_id {
        "codex" => crate::codex_config::get_codex_config_dir().join("sessions"),
        "claude" => crate::config::get_claude_config_dir().join("projects"),
        "opencode" => opencode::get_opencode_data_dir(),
        "openclaw" => crate::openclaw_config::get_openclaw_dir().join("agents"),
        "gemini" => crate::gemini_config::get_gemini_dir().join("tmp"),
        "hermes" => crate::hermes_config::get_hermes_dir().join("sessions"),
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
    use crate::database::Database;
    use tempfile::tempdir;

    #[test]
    fn rejects_source_path_outside_provider_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let source = outside.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err = delete_session_with_root("codex", "session-1", &source, root.path())
            .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider root"));
    }

    #[test]
    fn rejects_missing_source_path() {
        let root = tempdir().expect("tempdir");
        let missing = root.path().join("missing.jsonl");

        let err = delete_session_with_root("codex", "session-1", &missing, root.path())
            .expect_err("expected missing source path to fail");

        assert!(err.contains("session source not found"));
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

    #[test]
    fn applies_title_overrides_and_prunes_stale_entries() {
        let db = Database::memory().expect("memory db");
        save_title_overrides(
            &db,
            &[
                SessionTitleOverride {
                    provider_id: "codex".to_string(),
                    session_id: "s1".to_string(),
                    source_path: "/tmp/s1.jsonl".to_string(),
                    title: "Renamed session".to_string(),
                },
                SessionTitleOverride {
                    provider_id: "codex".to_string(),
                    session_id: "deleted".to_string(),
                    source_path: "/tmp/deleted.jsonl".to_string(),
                    title: "Deleted session".to_string(),
                },
            ],
        )
        .expect("save overrides");

        let sessions = vec![SessionMeta {
            provider_id: "codex".to_string(),
            session_id: "s1".to_string(),
            title: Some("Original title".to_string()),
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: Some("/tmp/s1.jsonl".to_string()),
            resume_command: None,
        }];

        let applied = apply_title_overrides(&db, sessions).expect("apply overrides");

        assert_eq!(applied[0].title.as_deref(), Some("Renamed session"));
        let saved = load_title_overrides(&db).expect("load overrides");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].session_id, "s1");
    }

    #[test]
    fn upsert_title_override_updates_existing_entry() {
        let db = Database::memory().expect("memory db");

        upsert_title_override(&db, "codex", "s1", "/tmp/s1.jsonl", "First title")
            .expect("insert override");
        upsert_title_override(&db, "codex", "s1", "/tmp/s1.jsonl", "Second title")
            .expect("update override");

        let saved = load_title_overrides(&db).expect("load overrides");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].title, "Second title");
    }
}
