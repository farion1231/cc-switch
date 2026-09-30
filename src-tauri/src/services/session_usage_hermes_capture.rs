//! Read-only import of the optional Hermes request-metadata plugin ledger.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::hermes_config::get_hermes_dir;
use crate::services::session_usage::SessionSyncResult;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const LEDGER: &str = "ccswitch-usage.sqlite";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesRequestEvent {
    pub event_id: String,
    pub profile_name: String,
    pub kind: String,
    pub session_id: String,
    pub task_id: String,
    pub aux_task: String,
    pub model: String,
    pub provider: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub status: String,
    pub status_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub usage_available: bool,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesHistoryEstimate {
    pub profile_name: String,
    pub captured_at_ms: i64,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost_usd: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesReplayResult {
    pub imported: u32,
    pub skipped: u32,
    pub unavailable: u32,
    pub errors: Vec<String>,
}

fn sources(root: &Path) -> Result<Vec<(String, PathBuf)>, AppError> {
    let mut result = Vec::new();
    if root.join(LEDGER).is_file() {
        result.push(("default".to_string(), root.join(LEDGER)));
    }
    let profiles = root.join("profiles");
    if profiles.is_dir() {
        for entry in fs::read_dir(&profiles).map_err(|e| AppError::io(&profiles, e))? {
            let entry = entry.map_err(|e| AppError::io(&profiles, e))?;
            let directory = entry.path();
            if directory.is_dir() && directory.join(LEDGER).is_file() {
                result.push((
                    entry.file_name().to_string_lossy().to_string(),
                    directory.join(LEDGER),
                ));
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

fn source_key(path: &Path, ledger_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(path.to_string_lossy().as_bytes());
    hash.update([0]);
    hash.update(ledger_id.as_bytes());
    format!("{:x}", hash.finalize())
}

fn ledger_key(source: &Connection, path: &Path) -> Result<String, AppError> {
    let ledger_id: String = source.query_row(
        "SELECT ledger_id FROM historical_baseline WHERE id=1",
        [],
        |row| row.get(0),
    )?;
    if ledger_id.is_empty() {
        return Err(AppError::Database(
            "Hermes capture ledger has no identity".to_string(),
        ));
    }
    Ok(source_key(path, &ledger_id))
}

fn open_source(path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA query_only=ON")?;
    Ok(conn)
}

pub fn sync_hermes_capture(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_hermes_capture_at_root(db, &get_hermes_dir())
}

pub(crate) fn sync_hermes_capture_at_root(
    db: &Database,
    root: &Path,
) -> Result<SessionSyncResult, AppError> {
    let sources = sources(root)?;
    let mut result = SessionSyncResult {
        files_scanned: sources.len() as u32,
        ..SessionSyncResult::default()
    };
    for (profile, path) in sources {
        match import_source(db, &profile, &path) {
            Ok((imported, skipped)) => {
                result.imported = result.imported.saturating_add(imported);
                result.skipped = result.skipped.saturating_add(skipped);
            }
            Err(error) => result
                .errors
                .push(format!("Hermes capture profile '{profile}': {error}")),
        }
    }
    Ok(result)
}

fn import_source(db: &Database, profile: &str, path: &Path) -> Result<(u32, u32), AppError> {
    let source = open_source(path)?;
    let key = ledger_key(&source, path)?;
    let mut imported = 0u32;
    let mut skipped = 0u32;
    loop {
        let cursor: i64 = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT last_rowid FROM hermes_capture_cursors WHERE source_key=?1",
                [&key],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0)
        };
        let mut stmt = source.prepare(
            "SELECT rowid, event_id, kind, session_id, task_id, aux_task,
        model, provider, started_at_ms, ended_at_ms, status, status_code,
        duration_ms, usage_available, input_tokens, output_tokens, cache_read_tokens,
        cache_write_tokens, reasoning_tokens FROM request_events
        WHERE rowid > ?1 ORDER BY rowid LIMIT 500",
        )?;
        let rows = stmt.query_map([cursor], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                HermesRequestEvent {
                    event_id: row.get(1)?,
                    profile_name: profile.to_string(),
                    kind: row.get(2)?,
                    session_id: row.get(3)?,
                    task_id: row.get(4)?,
                    aux_task: row.get(5)?,
                    model: row.get(6)?,
                    provider: row.get(7)?,
                    started_at_ms: row.get(8)?,
                    ended_at_ms: row.get(9)?,
                    status: row.get(10)?,
                    status_code: row.get(11)?,
                    duration_ms: row.get(12)?,
                    usage_available: row.get::<_, i64>(13)? != 0,
                    input_tokens: row.get(14)?,
                    output_tokens: row.get(15)?,
                    cache_read_tokens: row.get(16)?,
                    cache_write_tokens: row.get(17)?,
                    reasoning_tokens: row.get(18)?,
                },
            ))
        })?;
        let events = rows.collect::<Result<Vec<_>, _>>()?;
        if events.is_empty() {
            break;
        }
        let last_rowid = events.last().map(|(rowid, _)| *rowid).unwrap_or(cursor);
        let conn = lock_conn!(db.conn);
        let tx = conn.unchecked_transaction()?;
        for (_, event) in events {
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO hermes_request_events (
            source_key, event_id, profile_name, kind, session_id, task_id, aux_task,
            model, provider, started_at_ms, ended_at_ms, status, status_code, duration_ms,
            usage_available, input_tokens, output_tokens, cache_read_tokens,
            cache_write_tokens, reasoning_tokens
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18, ?19, ?20)",
                params![
                    key,
                    event.event_id,
                    event.profile_name,
                    event.kind,
                    event.session_id,
                    event.task_id,
                    event.aux_task,
                    event.model,
                    event.provider,
                    event.started_at_ms,
                    event.ended_at_ms,
                    event.status,
                    event.status_code,
                    event.duration_ms,
                    event.usage_available,
                    event.input_tokens,
                    event.output_tokens,
                    event.cache_read_tokens,
                    event.cache_write_tokens,
                    event.reasoning_tokens,
                ],
            )?;
            if inserted == 1 {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        tx.execute(
            "INSERT INTO hermes_capture_cursors (source_key, last_rowid) VALUES (?1, ?2)
             ON CONFLICT(source_key) DO UPDATE SET last_rowid=excluded.last_rowid",
            params![key, last_rowid],
        )?;
        tx.commit()?;
    }
    Ok((imported, skipped))
}

pub fn replay_hermes_history(db: &Database) -> Result<HermesReplayResult, AppError> {
    replay_hermes_history_at_root(db, &get_hermes_dir())
}

pub(crate) fn replay_hermes_history_at_root(
    db: &Database,
    root: &Path,
) -> Result<HermesReplayResult, AppError> {
    let mut result = HermesReplayResult::default();
    for (profile, path) in sources(root)? {
        match replay_source(db, &profile, &path) {
            Ok(Some(true)) => result.imported += 1,
            Ok(Some(false)) => result.skipped += 1,
            Ok(None) => result.unavailable += 1,
            Err(error) => result
                .errors
                .push(format!("Hermes profile '{profile}': {error}")),
        }
    }
    Ok(result)
}

fn replay_source(db: &Database, profile: &str, path: &Path) -> Result<Option<bool>, AppError> {
    let source = open_source(path)?;
    let baseline = source.query_row(
        "SELECT captured_at_ms, available, request_count,
        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
        reasoning_tokens, cost_usd FROM historical_baseline WHERE id=1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        },
    )?;
    if baseline.1 == 0 {
        return Ok(None);
    }
    let key = ledger_key(&source, path)?;
    let conn = lock_conn!(db.conn);
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO hermes_history_estimates (
        source_key, profile_name, captured_at_ms, request_count, input_tokens,
        output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_usd
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            key,
            profile,
            baseline.0,
            baseline.2.unwrap_or(0),
            baseline.3.unwrap_or(0),
            baseline.4.unwrap_or(0),
            baseline.5.unwrap_or(0),
            baseline.6.unwrap_or(0),
            baseline.7.unwrap_or(0),
            baseline.8.unwrap_or_else(|| "0".to_string()),
        ],
    )?;
    Ok(Some(inserted == 1))
}

#[allow(clippy::too_many_arguments)]
pub fn get_hermes_request_events(
    db: &Database,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    profile: Option<&str>,
    task: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    offset: Option<i64>,
) -> Result<Vec<HermesRequestEvent>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT event_id, profile_name, kind, session_id, task_id,
        aux_task, model, provider, started_at_ms, ended_at_ms, status, status_code,
        duration_ms, usage_available, input_tokens, output_tokens, cache_read_tokens,
        cache_write_tokens, reasoning_tokens FROM hermes_request_events
        WHERE (?1 IS NULL OR started_at_ms >= ?1)
          AND (?2 IS NULL OR started_at_ms <= ?2)
          AND (?3 IS NULL OR profile_name = ?3)
          AND (?4 IS NULL OR aux_task = ?4
               OR (?4 = 'unattributed_main' AND kind = 'main'))
          AND (?5 IS NULL OR LOWER(provider) = LOWER(?5))
          AND (?6 IS NULL OR model = ?6)
        ORDER BY started_at_ms DESC, source_key, event_id LIMIT 100 OFFSET ?7",
    )?;
    let rows = stmt.query_map(
        params![
            start_ms,
            end_ms,
            profile,
            task,
            provider,
            model,
            offset.unwrap_or(0).max(0)
        ],
        |row| {
            Ok(HermesRequestEvent {
                event_id: row.get(0)?,
                profile_name: row.get(1)?,
                kind: row.get(2)?,
                session_id: row.get(3)?,
                task_id: row.get(4)?,
                aux_task: row.get(5)?,
                model: row.get(6)?,
                provider: row.get(7)?,
                started_at_ms: row.get(8)?,
                ended_at_ms: row.get(9)?,
                status: row.get(10)?,
                status_code: row.get(11)?,
                duration_ms: row.get(12)?,
                usage_available: row.get::<_, i64>(13)? != 0,
                input_tokens: row.get(14)?,
                output_tokens: row.get(15)?,
                cache_read_tokens: row.get(16)?,
                cache_write_tokens: row.get(17)?,
                reasoning_tokens: row.get(18)?,
            })
        },
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_hermes_history_estimates(db: &Database) -> Result<Vec<HermesHistoryEstimate>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT profile_name, captured_at_ms, request_count,
        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
        reasoning_tokens, cost_usd FROM hermes_history_estimates ORDER BY profile_name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(HermesHistoryEstimate {
            profile_name: row.get(0)?,
            captured_at_ms: row.get(1)?,
            request_count: row.get(2)?,
            input_tokens: row.get(3)?,
            output_tokens: row.get(4)?,
            cache_read_tokens: row.get(5)?,
            cache_write_tokens: row.get(6)?,
            reasoning_tokens: row.get(7)?,
            cost_usd: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use rusqlite::Connection;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn destination() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        Database::create_tables_on_conn(&conn).unwrap();
        Database {
            conn: Mutex::new(conn),
        }
    }

    fn ledger(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE request_events (
            event_id TEXT PRIMARY KEY, kind TEXT, session_id TEXT, task_id TEXT,
            aux_task TEXT, model TEXT, provider TEXT, started_at_ms INTEGER,
            ended_at_ms INTEGER, status TEXT, status_code INTEGER, duration_ms INTEGER,
            usage_available INTEGER, input_tokens INTEGER, output_tokens INTEGER,
            cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER);
            CREATE TABLE historical_baseline (id INTEGER PRIMARY KEY, ledger_id TEXT, captured_at_ms INTEGER,
            available INTEGER, request_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
            cache_read_tokens INTEGER, cache_write_tokens INTEGER, reasoning_tokens INTEGER,
            cost_usd TEXT);
            INSERT INTO request_events VALUES ('main:a', 'main', 's', 't', '', 'm', 'p',
            100000, 101000, 'success', NULL, 1000, 1, 4, 5, 0, 0, 0);
            INSERT INTO historical_baseline VALUES (1, 'ledger-one', 90000, 1, 3, 10, 20, 0, 0, 0, '0.25');",
        )
        .unwrap();
    }

    #[test]
    fn exact_events_are_idempotent_and_history_is_manual_only() {
        let root = tempdir().unwrap();
        ledger(&root.path().join("ccswitch-usage.sqlite"));
        let db = destination();
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            0
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, None, None, None, None, None)
                .unwrap()
                .len(),
            1
        );
        assert!(get_hermes_history_estimates(&db).unwrap().is_empty());
        assert_eq!(
            replay_hermes_history_at_root(&db, root.path())
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            replay_hermes_history_at_root(&db, root.path())
                .unwrap()
                .imported,
            0
        );
        let totals = get_hermes_history_estimates(&db).unwrap();
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].request_count, 3);
        assert_eq!(totals[0].cost_usd, "0.25");
        let summary = db
            .get_usage_summary_with_hermes_filters(
                None,
                None,
                Some("hermes"),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(summary.total_requests, 0);
        let conn = db.conn.lock().unwrap();
        let daily_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_daily_rollups WHERE app_type='hermes'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(daily_rows, 0);
    }

    #[test]
    fn same_request_id_in_distinct_profiles_remains_distinct() {
        let root = tempdir().unwrap();
        let profile_dir = root.path().join("profiles/other");
        fs::create_dir_all(&profile_dir).unwrap();
        ledger(&root.path().join("ccswitch-usage.sqlite"));
        ledger(&profile_dir.join("ccswitch-usage.sqlite"));
        let db = destination();
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            2
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, None, None, None, None, None)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, Some("other"), None, None, None, None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            get_hermes_request_events(
                &db,
                None,
                None,
                Some("other"),
                Some("unattributed_main"),
                None,
                None,
                None
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            replay_hermes_history_at_root(&db, root.path())
                .unwrap()
                .imported,
            2
        );
        assert_eq!(get_hermes_history_estimates(&db).unwrap().len(), 2);
    }

    #[test]
    fn replaced_ledger_at_same_path_imports_new_events_but_not_overlapping_history() {
        let root = tempdir().unwrap();
        let path = root.path().join(LEDGER);
        ledger(&path);
        let db = destination();
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            replay_hermes_history_at_root(&db, root.path())
                .unwrap()
                .imported,
            1
        );
        fs::remove_file(&path).unwrap();
        ledger(&path);
        let source = Connection::open(&path).unwrap();
        source
            .execute("UPDATE historical_baseline SET ledger_id='ledger-two'", [])
            .unwrap();
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            replay_hermes_history_at_root(&db, root.path())
                .unwrap()
                .imported,
            0
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, None, None, None, None, None)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(get_hermes_history_estimates(&db).unwrap().len(), 1);
    }

    #[test]
    fn importer_batches_and_request_pages_reach_older_rows() {
        let root = tempdir().unwrap();
        let path = root.path().join(LEDGER);
        ledger(&path);
        let source = Connection::open(&path).unwrap();
        for n in 0..1001 {
            source
                .execute(
                    "INSERT INTO request_events SELECT ?1, kind, session_id, task_id, aux_task,
                 model, provider, started_at_ms + ?2, ended_at_ms + ?2, status,
                 status_code, duration_ms, usage_available, input_tokens, output_tokens,
                 cache_read_tokens, cache_write_tokens, reasoning_tokens
                 FROM request_events WHERE event_id='main:a'",
                    params![format!("extra:{n}"), n + 1],
                )
                .unwrap();
        }
        let db = destination();
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            1002
        );
        assert_eq!(
            sync_hermes_capture_at_root(&db, root.path())
                .unwrap()
                .imported,
            0
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, None, None, None, None, Some(0))
                .unwrap()
                .len(),
            100
        );
        assert_eq!(
            get_hermes_request_events(&db, None, None, None, None, None, None, Some(1000))
                .unwrap()
                .len(),
            2
        );
    }
}
