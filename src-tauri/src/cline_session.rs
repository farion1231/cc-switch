//! Cline CLI session index shared by the session reader and the usage importer.
//!
//! Cline keeps its index in `sessions.db` (WAL) and each conversation in
//! `<sessions dir>/<id>/<id>.messages.json`. The database is only ever opened
//! read-only; when it is missing the per-session manifests are used instead.
use rusqlite::{Connection, OpenFlags, Row};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ClineSession {
    pub id: String,
    pub is_subagent: bool,
    pub cwd: Option<String>,
    pub started_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub messages_path: PathBuf,
}

/// Session ids become path segments and shell arguments.
pub(crate) fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub(crate) fn timestamp_ms(value: &Value) -> Option<i64> {
    match value {
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|time| time.timestamp_millis()),
        Value::Number(number) => {
            let time = number.as_f64().filter(|t| t.is_finite() && *t > 0.0)?;
            let millis = if time < 1e12 { time * 1000.0 } else { time };
            (millis < i64::MAX as f64).then_some(millis as i64)
        }
        _ => None,
    }
}

/// The CLI wraps the first prompt as `<user_input mode="act">…</user_input>`.
pub(crate) fn strip_user_input(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("<user_input") else {
        return trimmed;
    };
    if !rest.starts_with(['>', ' ']) {
        return trimmed;
    }
    let Some(end) = rest.find('>') else {
        return trimmed;
    };
    let inner = &rest[end + 1..];
    inner.strip_suffix("</user_input>").unwrap_or(inner).trim()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

fn messages_path(sessions_dir: &Path, id: &str, recorded: Option<String>) -> PathBuf {
    let fallback = sessions_dir.join(id).join(format!("{id}.messages.json"));
    match non_empty(recorded).map(PathBuf::from) {
        Some(path) if path.is_absolute() && path.is_file() => path,
        _ => fallback,
    }
}

fn title_from_metadata(metadata: &Value) -> Option<String> {
    metadata
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

fn column<T: rusqlite::types::FromSql>(row: &Row<'_>, name: &str) -> Option<T> {
    row.get::<_, Option<T>>(name).ok().flatten()
}

fn from_database(db_path: &Path, sessions_dir: &Path) -> Option<Vec<ClineSession>> {
    if !db_path.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut stmt = conn
        .prepare("SELECT * FROM sessions WHERE source = 'cli'")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get("session_id")?;
            let metadata = column::<String>(row, "metadata_json")
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .unwrap_or(Value::Null);
            let is_subagent = column::<i64>(row, "is_subagent").unwrap_or(0) != 0
                || non_empty(column(row, "parent_session_id")).is_some();
            let recorded = column::<String>(row, "messages_path");
            Ok(ClineSession {
                is_subagent,
                cwd: non_empty(column(row, "cwd"))
                    .or_else(|| non_empty(column(row, "workspace_root"))),
                started_at: column::<String>(row, "started_at")
                    .and_then(|text| timestamp_ms(&Value::String(text))),
                updated_at: column::<String>(row, "updated_at")
                    .or_else(|| column(row, "ended_at"))
                    .and_then(|text| timestamp_ms(&Value::String(text))),
                title: title_from_metadata(&metadata),
                prompt: non_empty(column(row, "prompt")),
                messages_path: messages_path(sessions_dir, &id, recorded),
                id,
            })
        })
        .ok()?;
    Some(
        rows.flatten()
            .filter(|session| valid_session_id(&session.id))
            .collect(),
    )
}

fn from_manifests(sessions_dir: &Path) -> Vec<ClineSession> {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if !valid_session_id(&id) {
            continue;
        }
        let Some(manifest) = fs::read(entry.path().join(format!("{id}.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            continue;
        };
        if manifest.get("source").and_then(Value::as_str) != Some("cli") {
            continue;
        }
        let text = |key: &str| {
            manifest
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|text| !text.trim().is_empty())
        };
        let is_subagent = manifest
            .get("is_subagent")
            .is_some_and(|value| value.as_bool() == Some(true) || value.as_i64() == Some(1))
            || text("parent_session_id").is_some();
        sessions.push(ClineSession {
            is_subagent,
            cwd: text("cwd").or_else(|| text("workspace_root")),
            started_at: manifest.get("started_at").and_then(timestamp_ms),
            updated_at: manifest
                .get("updated_at")
                .or_else(|| manifest.get("ended_at"))
                .and_then(timestamp_ms),
            title: manifest.get("metadata").and_then(title_from_metadata),
            prompt: text("prompt"),
            messages_path: messages_path(sessions_dir, &id, text("messages_path")),
            id,
        });
    }
    sessions
}

/// All CLI sessions (subagents included), from the index or the manifests.
pub(crate) fn read_index(db_path: &Path, sessions_dir: &Path) -> Vec<ClineSession> {
    from_database(db_path, sessions_dir).unwrap_or_else(|| from_manifests(sessions_dir))
}

pub(crate) fn sessions() -> Vec<ClineSession> {
    read_index(
        &crate::cline_config::sessions_db_path(),
        &crate::cline_config::sessions_dir(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// Write a minimal Cline 3.x data layout: `sessions.db` plus per-session files.
    pub(crate) fn write_fixture(
        db_path: &Path,
        sessions_dir: &Path,
        rows: &[(&str, &str, bool, Value)],
    ) {
        fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE sessions (session_id TEXT PRIMARY KEY, source TEXT NOT NULL, pid INTEGER NOT NULL,
               started_at TEXT NOT NULL, ended_at TEXT, exit_code INTEGER, status TEXT NOT NULL,
               interactive INTEGER NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL, cwd TEXT NOT NULL,
               workspace_root TEXT NOT NULL, parent_session_id TEXT, is_subagent INTEGER NOT NULL DEFAULT 0,
               prompt TEXT, metadata_json TEXT, transcript_path TEXT NOT NULL DEFAULT '',
               hook_path TEXT NOT NULL, messages_path TEXT, updated_at TEXT NOT NULL);",
        )
        .unwrap();
        for (id, source, subagent, messages) in rows {
            let dir = sessions_dir.join(id);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{id}.messages.json"));
            fs::write(
                &path,
                json!({"version":1,"sessionId":id,"messages":messages}).to_string(),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sessions VALUES (?1, ?2, 1, '2026-10-22T16:43:37.848Z', '2026-10-22T16:43:37.985Z', 0,
                  'completed', 0, 'anthropic', 'claude-sonnet-4-5', '/work/project', '/work/project',
                  CASE WHEN ?3 THEN 'parent' END, ?3, 'Reply with one word please.',
                  '{\"title\":\"Fixture title\",\"usage\":{\"inputTokens\":999}}', '', '', ?4,
                  '2026-10-22T16:44:00.000Z')",
                rusqlite::params![id, source, subagent, path.to_string_lossy()],
            )
            .unwrap();
        }
    }

    #[test]
    fn cline_index_reads_cli_rows_read_only_and_falls_back_to_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db/sessions.db");
        let sessions_dir = dir.path().join("sessions");
        write_fixture(
            &db,
            &sessions_dir,
            &[
                ("s_cli", "cli", false, json!([])),
                ("s_vscode", "vscode", false, json!([])),
                ("s_sub", "cli", true, json!([])),
            ],
        );
        let mut sessions = read_index(&db, &sessions_dir);
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            sessions
                .iter()
                .map(|s| (s.id.as_str(), s.is_subagent))
                .collect::<Vec<_>>(),
            vec![("s_cli", false), ("s_sub", true)]
        );
        assert_eq!(sessions[0].title.as_deref(), Some("Fixture title"));
        assert_eq!(sessions[0].cwd.as_deref(), Some("/work/project"));
        assert_eq!(sessions[0].started_at, Some(1_792_687_417_848));
        assert!(sessions[0].messages_path.is_file());

        let manifest_dir = dir.path().join("only-manifests");
        fs::create_dir_all(manifest_dir.join("m1")).unwrap();
        fs::write(
            manifest_dir.join("m1/m1.json"),
            json!({"session_id":"m1","source":"cli","cwd":"/p","started_at":"2026-10-22T16:43:37.848Z","metadata":{"title":"T"}}).to_string(),
        )
        .unwrap();
        let fallback = read_index(&dir.path().join("missing.db"), &manifest_dir);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].title.as_deref(), Some("T"));
        assert_eq!(
            fallback[0].messages_path,
            manifest_dir.join("m1/m1.messages.json")
        );
    }

    #[test]
    fn cline_user_input_wrapper_is_stripped() {
        assert_eq!(
            strip_user_input("<user_input mode=\"act\">Hello\nworld</user_input>"),
            "Hello\nworld"
        );
        assert_eq!(strip_user_input("<user_input>x</user_input>"), "x");
        assert_eq!(strip_user_input("<user_inputs>x"), "<user_inputs>x");
        assert_eq!(strip_user_input("  plain "), "plain");
        assert!(!valid_session_id("../x"));
        assert!(valid_session_id("1792687417846_66iec"));
    }
}
