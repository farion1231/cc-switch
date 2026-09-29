//! Which `model_provider` ids do historical Codex sessions actually reference?
//!
//! Codex 0.149+ refuses to load a session whose `model_provider` has no matching
//! `[model_providers.<id>]` table, and session metadata is immutable
//! (`docs/rfcs/0002-codex-inert-provider-tables.md` §2.2). So anything writing
//! `config.toml` has to know the set of ids history depends on.
//!
//! **The set must be read from the history itself.** The previous proxy —
//! "every other codex provider row in the DB" — is not equivalent: ids minted by
//! the live projection (e.g. `cc-switch-official`, written by
//! `apply_codex_official_proxy_route` during official takeover) exist in no DB
//! row, so a DB-driven merge can never restore them. Every official ⇄ third-party
//! round trip then breaks every session created while the other route was live,
//! and the failure is one-directional: the `custom` alias rule (RFC §2.4b)
//! protects the third-party id, nothing protected the official one.
//!
//! Two sources, deliberately unioned because they disagree: rollout `.jsonl`
//! files and the `threads` table in Codex's state DB. Codex writes them
//! independently and either can be missing a thread the other knows about
//! (`codex doctor` reports exactly that), so a single source silently drops ids.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::codex_state_db::codex_state_db_paths;

/// How many leading lines of a rollout file to consider before giving up on
/// finding its `session_meta` record. Codex writes the record first, but a
/// truncated or externally-edited file must degrade to "not found" rather than
/// to a full-file scan — this runs on every provider switch and the history is
/// hundreds of megabytes.
const SESSION_META_PROBE_LINES: usize = 8;

/// Collect every `model_provider` id referenced by historical Codex sessions.
///
/// `codex_dir` is the Codex config dir (`~/.codex`); `config_text` is the live
/// `config.toml` contents, used to resolve a relocated state DB.
///
/// Never fails: a missing history directory, an unparseable line, or a locked
/// state DB each degrade to "that source contributed nothing". A guard that
/// breaks the switch when it cannot read history would be worse than the bug it
/// prevents.
pub(crate) fn collect_session_referenced_provider_ids(
    codex_dir: &Path,
    config_text: &str,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    collect_from_rollout_files(codex_dir, &mut ids);
    collect_from_state_dbs(codex_dir, config_text, &mut ids);
    ids
}

/// Provider ids referenced by rollout files, from the `session_meta` record's
/// `payload.model_provider`.
fn collect_from_rollout_files(codex_dir: &Path, ids: &mut BTreeSet<String>) {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4);
    for path in files {
        if let Some(id) = rollout_session_meta_provider_id(&path) {
            ids.insert(id);
        }
    }
}

/// Read the leading `session_meta` record of one rollout file.
fn rollout_session_meta_provider_id(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().take(SESSION_META_PROBE_LINES) {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
            continue;
        }
        if let Some(id) = value
            .get("payload")
            .and_then(|p| p.get("model_provider"))
            .and_then(|p| p.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            return Some(id.to_string());
        }
        // A `session_meta` without a provider id carries no routing truth; keep
        // probing rather than settling for a later record of another type.
    }
    None
}

/// Provider ids referenced by the `threads` table of Codex's state DB(s).
///
/// Opened read-only on purpose: the state DB belongs to the running Codex app,
/// and this function must not create a WAL or take a write lock on it.
fn collect_from_state_dbs(codex_dir: &Path, config_text: &str, ids: &mut BTreeSet<String>) {
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if !db_path.exists() {
            continue;
        }
        let Ok(conn) = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
            continue;
        };
        // Codex holds this DB while running; a short timeout plus a table-exists
        // guard keeps a busy app from stalling a provider switch.
        let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
        let Ok(mut stmt) = conn.prepare(
            "SELECT DISTINCT model_provider FROM threads \
             WHERE model_provider IS NOT NULL AND TRIM(model_provider) <> ''",
        ) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
            continue;
        };
        for row in rows.flatten() {
            let trimmed = row.trim();
            if !trimmed.is_empty() {
                ids.insert(trimmed.to_string());
            }
        }
    }
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u8, max_depth: u8) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn session_meta_line(provider: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-09-29T00:00:00Z","type":"session_meta","payload":{{"id":"x","model_provider":"{provider}","cwd":"/tmp"}}}}"#
        )
    }

    #[test]
    fn reads_provider_id_from_first_line_of_rollout() {
        let _guard = tempdir().unwrap();
        let dir = _guard.path();
        let path = dir.join("sessions/2026/09/29/rollout-a.jsonl");
        write_file(
            &path,
            &format!(
                "{}\n{}\n",
                session_meta_line("cc-switch-official"),
                r#"{"type":"event_msg","payload":{"note":"large tail is never parsed"}}"#
            ),
        );

        let ids = collect_session_referenced_provider_ids(dir, "");
        assert_eq!(
            ids,
            BTreeSet::from(["cc-switch-official".to_string()]),
            "session_meta on line 1 must yield its provider id"
        );
    }

    #[test]
    fn skips_truncated_file_without_a_session_meta_record() {
        let _guard = tempdir().unwrap();
        let dir = _guard.path();
        write_file(
            &dir.join("sessions/2026/09/29/rollout-broken.jsonl"),
            r#"{"type":"event_msg","payload":{"partial":"#,
        );

        let ids = collect_session_referenced_provider_ids(dir, "");
        assert!(
            ids.is_empty(),
            "a truncated rollout contributes nothing rather than panicking: {ids:?}"
        );
    }

    #[test]
    fn unions_rollout_and_state_db_sources() {
        let _guard = tempdir().unwrap();
        let dir = _guard.path();
        write_file(
            &dir.join("sessions/2026/09/29/rollout-a.jsonl"),
            &session_meta_line("from-rollout"),
        );
        let db_path = dir.join("state_5.sqlite");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO threads (id, model_provider) VALUES ('a', 'from-state-db'), ('b', 'from-rollout')",
                [],
            )
            .unwrap();
        }

        let ids = collect_session_referenced_provider_ids(dir, "");
        assert_eq!(
            ids,
            BTreeSet::from(["from-rollout".to_string(), "from-state-db".to_string()]),
            "either source alone would miss an id the other knows about"
        );
    }

    #[test]
    fn missing_history_yields_empty_set_instead_of_failing() {
        let _guard = tempdir().unwrap();
        let dir = _guard.path();
        assert!(
            collect_session_referenced_provider_ids(dir, "").is_empty(),
            "no history at all is a normal state, not an error"
        );
    }
}
