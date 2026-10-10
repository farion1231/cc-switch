//! Import MCode's committed token-usage projection without modifying its database.
//!
//! Models prefer the usage row, then uniquely matched request telemetry, then the
//! session's effectiveModel. Reconcile the last two days and repair candidates
//! so late telemetry can fill provisional models without replaying all history.
//! MCode's thinking time is not TTFT; first_token_ms stays NULL.
use std::collections::{HashMap, HashSet};

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{
    calculator::{CostBreakdown, CostCalculator, ModelPricing},
    parser::TokenUsage,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::{session_usage::SessionSyncResult, usage_stats::find_model_pricing};
use crate::session_manager::providers::mcode;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::Value;

/// The source has no shared request id. Only a unique token-usage match within
/// the same session and turn is safe; repeated fingerprints remain unmatched.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RequestKey {
    session_id: String,
    turn_id: Option<String>,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
}

struct RequestMeta {
    model: Option<String>,
    latency_ms: Option<i64>,
}

struct SourceMeta {
    requests: HashMap<RequestKey, Option<RequestMeta>>,
    session_models: HashMap<String, String>,
}

const RECONCILE_WINDOW_SECONDS: i64 = 2 * 24 * 60 * 60;

struct ResolvedCost {
    input: String,
    output: String,
    cache_read: String,
    cache_creation: String,
    total: String,
}

impl From<CostBreakdown> for ResolvedCost {
    fn from(cost: CostBreakdown) -> Self {
        Self {
            input: cost.input_cost.to_string(),
            output: cost.output_cost.to_string(),
            cache_read: cost.cache_read_cost.to_string(),
            cache_creation: cost.cache_creation_cost.to_string(),
            total: cost.total_cost.to_string(),
        }
    }
}

struct RetainedLog {
    model: String,
    latency_ms: i64,
}

struct SourceUsage {
    id: i64,
    key: RequestKey,
    model: Option<String>,
    ts: i64,
    reasoning_tokens: u32,
    native_cost: Option<f64>,
}

/// Strip only the provider prefix, preserving model namespaces after it.
fn strip_model_namespace(native_model: &str) -> &str {
    native_model
        .split_once('/')
        .map_or(native_model, |(_, model)| model)
}

fn usable_model(model: &str) -> Option<&str> {
    let model = strip_model_namespace(model.trim()).trim();
    (!model.is_empty() && !model.eq_ignore_ascii_case("unknown")).then_some(model)
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn json_tokens(value: &Value, pointer: &str) -> Option<u32> {
    value
        .pointer(pointer)?
        .as_u64()
        .and_then(|tokens| u32::try_from(tokens).ok())
}

fn has_column(source: &rusqlite::Connection, table: &str, column: &str) -> bool {
    source
        .query_row(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, column],
            |_| Ok(()),
        )
        .is_ok()
}

fn load_source_meta(
    source: &rusqlite::Connection,
    usage_rows: &[&SourceUsage],
) -> Result<SourceMeta, AppError> {
    let mut requests = HashMap::new();
    let turns: HashSet<_> = usage_rows
        .iter()
        .map(|row| (&row.key.session_id, &row.key.turn_id))
        .collect();
    if table_exists(source, "local_runtime_message_rows") {
        let mut stmt = source.prepare(
            "SELECT data_json FROM local_runtime_message_rows
             WHERE role = 'assistant' AND session_id = ?1 AND turn_id IS ?2",
        )?;
        for (session_id, turn_id) in turns {
            let mut rows = stmt.query(params![session_id, turn_id])?;
            while let Some(row) = rows.next()? {
                let data: String = row.get(0)?;
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                let (Some(input_tokens), Some(output_tokens)) = (
                    json_tokens(&value, "/usage/input_tokens"),
                    json_tokens(&value, "/usage/output_tokens"),
                ) else {
                    continue;
                };
                let key = RequestKey {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens: json_tokens(&value, "/usage/cache_read").unwrap_or(0),
                    cache_write_tokens: json_tokens(&value, "/usage/cache_write").unwrap_or(0),
                };
                let meta = RequestMeta {
                    model: value
                        .pointer("/context_usage_telemetry/model")
                        .and_then(Value::as_str)
                        .and_then(usable_model)
                        .map(str::to_owned),
                    latency_ms: value
                        .pointer("/usage/request_duration_ms")
                        .and_then(Value::as_i64)
                        .filter(|&duration| duration > 0),
                };
                requests
                    .entry(key)
                    .and_modify(|existing| *existing = None)
                    .or_insert(Some(meta));
            }
        }
    }

    let mut session_models = HashMap::new();
    // Older MCode schemas have this table but no extra_data_json. Session
    // selection is only a fallback and must never prevent token import.
    if has_column(source, "local_runtime_sessions", "extra_data_json") {
        let sessions: HashSet<_> = usage_rows.iter().map(|row| &row.key.session_id).collect();
        let mut stmt = source
            .prepare("SELECT extra_data_json FROM local_runtime_sessions WHERE session_id = ?1")?;
        for session in sessions {
            let mut rows = stmt.query([session])?;
            if let Some(row) = rows.next()? {
                let data: String = row.get(0)?;
                if let Some(model) = serde_json::from_str::<Value>(&data).ok().and_then(|value| {
                    value
                        .get("effectiveModel")?
                        .as_str()
                        .and_then(usable_model)
                        .map(str::to_owned)
                }) {
                    session_models.insert(session.clone(), model);
                }
            }
        }
    }
    Ok(SourceMeta {
        requests,
        session_models,
    })
}

fn load_source_usage(source: &rusqlite::Connection) -> Result<Vec<SourceUsage>, AppError> {
    let mut stmt = source.prepare(
        "SELECT id, session_id, model, ts, input_tokens, output_tokens,
                reasoning_tokens, cache_read_tokens, cache_write_tokens, cost_usd, turn_id
         FROM local_runtime_token_usage ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SourceUsage {
            id: row.get(0)?,
            key: RequestKey {
                session_id: row.get(1)?,
                turn_id: row.get(10)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                cache_read_tokens: row.get(7)?,
                cache_write_tokens: row.get(8)?,
            },
            model: row.get(2)?,
            ts: row.get(3)?,
            reasoning_tokens: row.get(6)?,
            native_cost: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// One full reconciliation upgrades records from earlier importer versions.
/// Subsequent passes only revisit recent requests or unfinished model/TTFT repairs.
fn retained_logs(
    conn: &rusqlite::Connection,
    repair_all: bool,
    cutoff: i64,
) -> Result<HashMap<String, RetainedLog>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT request_id, model, latency_ms FROM proxy_request_logs
         WHERE data_source = 'mcode_session'
           AND (?1 OR created_at >= ?2 OR model = 'unknown' OR first_token_ms IS NOT NULL)",
    )?;
    let rows = stmt.query_map(params![repair_all, cutoff], |row| {
        Ok((
            row.get(0)?,
            RetainedLog {
                model: row.get(1)?,
                latency_ms: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

fn resolve_cost(
    tx: &rusqlite::Transaction<'_>,
    native_cost: Option<f64>,
    usage: &TokenUsage,
    model: &str,
    pricing_cache: &mut HashMap<String, Option<ModelPricing>>,
) -> ResolvedCost {
    if let Some(cost) = native_cost.filter(|cost| cost.is_finite() && *cost > 0.0) {
        // The native total does not include a reliable component breakdown.
        return ResolvedCost {
            input: "0".into(),
            output: "0".into(),
            cache_read: "0".into(),
            cache_creation: "0".into(),
            total: cost.to_string(),
        };
    }

    let pricing = pricing_cache
        .entry(model.to_owned())
        .or_insert_with(|| find_model_pricing(tx, model));
    pricing
        .as_ref()
        .map(|pricing| CostCalculator::calculate_for_app("mcode", usage, pricing, Decimal::ONE))
        .unwrap_or(CostBreakdown {
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            cache_read_cost: Decimal::ZERO,
            cache_creation_cost: Decimal::ZERO,
            total_cost: Decimal::ZERO,
        })
        .into()
}

pub fn sync_mcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    if !mcode::database_path().exists() {
        return Ok(SessionSyncResult::default());
    }
    let source = mcode::open_database()?;
    let key = format!("mcode:{}", mcode::database_path().display());
    sync_from_database(db, &source, &key)
}

fn sync_from_database(
    db: &Database,
    source: &rusqlite::Connection,
    key: &str,
) -> Result<SessionSyncResult, AppError> {
    let repair_key = format!("{key}:request-cost-repair-v1");
    let cutoff = chrono::Utc::now().timestamp() - RECONCILE_WINDOW_SECONDS;
    // Release the main database lock before reading or parsing source metadata.
    let (mut cursor, retained, repair_all) = {
        let conn = lock_conn!(db.conn);
        let cursor = conn.query_row(
            "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
            [key],
            |row| row.get::<_, i64>(0),
        )?;
        let repaired: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_log_sync WHERE file_path = ?1)",
            [&repair_key],
            |row| row.get(0),
        )?;
        (cursor, retained_logs(&conn, !repaired, cutoff)?, !repaired)
    };
    let snapshot = source.unchecked_transaction()?;
    let max_id: i64 = snapshot.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM local_runtime_token_usage",
        [],
        |row| row.get(0),
    )?;
    // No source history or JSON is loaded on an idle pass with no repair candidates.
    let source_rows = if retained.is_empty() && max_id <= cursor {
        Vec::new()
    } else {
        load_source_usage(&snapshot)?
    };
    let mut occurrences = HashMap::<&RequestKey, usize>::new();
    for row in &source_rows {
        *occurrences.entry(&row.key).or_default() += 1;
    }
    let candidates: Vec<_> = source_rows
        .iter()
        .filter(|row| {
            row.id > cursor
                || retained.contains_key(&format!("mcode:{}:{}", row.key.session_id, row.id))
        })
        .collect();
    let source_meta = if candidates.is_empty() {
        SourceMeta {
            requests: HashMap::new(),
            session_models: HashMap::new(),
        }
    } else {
        load_source_meta(&snapshot, &candidates)?
    };
    snapshot.commit()?;

    let mut result = SessionSyncResult::default();
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut pricing_cache = HashMap::new();
    result.files_scanned = 1;
    for row in candidates {
        let request_id = format!("mcode:{}:{}", row.key.session_id, row.id);
        let existing = retained.get(&request_id);
        let existing_model = existing.map(|log| log.model.as_str());
        // Replay retained rows, but never resurrect logs deleted below the watermark.
        if row.id <= cursor && existing_model.is_none() {
            continue;
        }
        let meta = source_meta
            .requests
            .get(&row.key)
            .and_then(Option::as_ref)
            .filter(|_| occurrences.get(&row.key) == Some(&1));
        let model = row
            .model
            .as_deref()
            .and_then(usable_model)
            .or_else(|| meta.and_then(|meta| meta.model.as_deref()))
            // Do not replace a previously resolved model with today's session fallback
            // if the corresponding message was removed from the source database.
            .or_else(|| existing_model.filter(|model| usable_model(model).is_some()))
            .or_else(|| {
                source_meta
                    .session_models
                    .get(&row.key.session_id)
                    .map(String::as_str)
            })
            .unwrap_or("unknown");
        let usage = TokenUsage {
            input_tokens: row.key.input_tokens,
            output_tokens: row.key.output_tokens.saturating_add(row.reasoning_tokens),
            cache_read_tokens: row.key.cache_read_tokens,
            cache_creation_tokens: row.key.cache_write_tokens,
            model: Some(model.into()),
            message_id: None,
        };
        let cost = resolve_cost(&tx, row.native_cost, &usage, model, &mut pricing_cache);
        let ResolvedCost {
            input: input_cost,
            output: output_cost,
            cache_read: cache_read_cost,
            cache_creation: cache_creation_cost,
            total: total_cost,
        } = cost;
        // Missing/ambiguous messages cannot invalidate a previously known duration.
        let latency_ms = meta
            .and_then(|meta| meta.latency_ms)
            .or_else(|| existing.map(|log| log.latency_ms))
            .unwrap_or(0);
        let changed = if existing_model.is_some() {
            // Null-safe comparisons make reconciliation idempotent, including repairs
            // to old turn-level durations and fabricated first-token measurements.
            tx.execute(
                "UPDATE proxy_request_logs
                 SET model = ?1, request_model = ?1, total_cost_usd = ?2,
                     latency_ms = ?3, first_token_ms = NULL,
                     input_cost_usd = ?5, output_cost_usd = ?6,
                     cache_read_cost_usd = ?7, cache_creation_cost_usd = ?8
                 WHERE request_id = ?4 AND data_source = 'mcode_session'
                   AND (model IS NOT ?1 OR request_model IS NOT ?1
                        OR total_cost_usd IS NOT ?2 OR latency_ms IS NOT ?3
                        OR first_token_ms IS NOT NULL
                        OR input_cost_usd IS NOT ?5 OR output_cost_usd IS NOT ?6
                        OR cache_read_cost_usd IS NOT ?7 OR cache_creation_cost_usd IS NOT ?8)",
                params![
                    model,
                    total_cost,
                    latency_ms,
                    request_id,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost
                ],
            )?
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, first_token_ms, status_code, session_id,
                    provider_type, is_streaming, cost_multiplier, created_at, data_source,
                    input_token_semantics
                 ) VALUES (?1, '_mcode_session', 'mcode', ?2, ?2, ?3, ?4, ?5, ?6,
                           ?12, ?13, ?14, ?15, ?7, ?8, NULL, 200, ?9, 'mcode_session', 1, '1',
                           ?10, 'mcode_session', ?11)",
                params![
                    request_id,
                    model,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens,
                    total_cost,
                    latency_ms,
                    row.key.session_id,
                    row.ts / 1000,
                    INPUT_TOKEN_SEMANTICS_FRESH,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost
                ],
            )?
        };
        result.imported += changed as u32;
        result.skipped += u32::from(changed == 0);
        cursor = cursor.max(row.id);
    }
    tx.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, ?2, unixepoch()) ON CONFLICT(file_path) DO UPDATE SET
         last_line_offset = excluded.last_line_offset, last_synced_at = excluded.last_synced_at",
        params![key, cursor],
    )?;
    if repair_all {
        tx.execute(
            "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
             VALUES (?1, 0, 1, unixepoch()) ON CONFLICT(file_path) DO NOTHING",
            [&repair_key],
        )?;
    }
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source() -> rusqlite::Connection {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source.execute_batch(
            "CREATE TABLE local_runtime_token_usage (
                id INTEGER PRIMARY KEY, session_id TEXT, model TEXT, ts INTEGER,
                input_tokens INTEGER, output_tokens INTEGER, reasoning_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER, cost_usd REAL, turn_id TEXT);
             CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, turn_id TEXT, data_json TEXT);
             CREATE TABLE local_runtime_sessions (session_id TEXT PRIMARY KEY, extra_data_json TEXT);"
        ).unwrap();
        source
    }

    fn add_usage(
        source: &rusqlite::Connection,
        id: i64,
        session: &str,
        turn: &str,
        model: Option<&str>,
        tokens: (u32, u32),
    ) {
        source
            .execute(
                "INSERT INTO local_runtime_token_usage VALUES (?1,?2,?3,unixepoch()*1000,?4,?5,0,0,0,0,?6)",
                params![id, session, model, tokens.0, tokens.1, turn],
            )
            .unwrap();
    }

    fn add_message(
        source: &rusqlite::Connection,
        id: i64,
        session: &str,
        turn: &str,
        model: Option<&str>,
        tokens: (u32, u32),
        duration: Option<i64>,
    ) {
        let data = json!({
            "context_usage_telemetry": {"model": model},
            "usage": {
                "input_tokens": tokens.0, "output_tokens": tokens.1,
                "request_duration_ms": duration,
            },
            "thinking_duration_ms": 3200,
        });
        source
            .execute(
                "INSERT INTO local_runtime_message_rows VALUES (?1,?2,'assistant',?3,?4)",
                params![id, session, turn, data.to_string()],
            )
            .unwrap();
    }

    fn add_session(source: &rusqlite::Connection, session: &str, model: &str) {
        source
            .execute(
                "INSERT INTO local_runtime_sessions VALUES (?1,?2)",
                params![session, json!({"effectiveModel": model}).to_string()],
            )
            .unwrap();
    }

    fn log(db: &Database, request: &str) -> (String, String, i64, Option<i64>) {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT model,total_cost_usd,latency_ms,first_token_ms FROM proxy_request_logs
             WHERE request_id=?1",
                [request],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn mcode_usage_preserves_model_namespaces() {
        let source = source();
        add_usage(
            &source,
            1,
            "s1",
            "t1",
            Some("custom_provider:router/vendor/model"),
            (1, 2),
        );
        add_usage(
            &source,
            2,
            "s1",
            "t2",
            Some("custom_provider:router/another/model"),
            (1, 2),
        );
        source
            .execute_batch("UPDATE local_runtime_token_usage SET cost_usd=id*0.25")
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "namespaces")
                .unwrap()
                .imported,
            2
        );
        assert_eq!(
            log(&db, "mcode:s1:1"),
            ("vendor/model".into(), "0.25".into(), 0, None)
        );
        assert_eq!(
            log(&db, "mcode:s1:2"),
            ("another/model".into(), "0.5".into(), 0, None)
        );
        assert_eq!(
            sync_from_database(&db, &source, "namespaces")
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn preserves_tokens_and_does_not_reimport_pruned_rows() {
        let source = source();
        add_usage(
            &source,
            1,
            "s1",
            "t1",
            Some("custom_provider:test/model"),
            (10, 20),
        );
        source
            .execute_batch(
                "UPDATE local_runtime_token_usage SET reasoning_tokens=3,
            cache_read_tokens=40,cache_write_tokens=5",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "pruned").unwrap().imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let values: (i64, i64, i64, i64, i64) = conn
                .query_row(
                    "SELECT input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,
                    input_token_semantics FROM proxy_request_logs",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(values, (10, 23, 40, 5, INPUT_TOKEN_SEMANTICS_FRESH));
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        // Even newly available telemetry must not resurrect a pruned request.
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (10, 20),
            Some(8500),
        );
        assert_eq!(
            sync_from_database(&db, &source, "pruned").unwrap().imported,
            0
        );
        add_usage(&source, 2, "s1", "t1", Some("minimax/MiniMax-M3"), (1, 2));
        assert_eq!(
            sync_from_database(&db, &source, "pruned").unwrap().imported,
            1
        );
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn model_fallback_and_individual_request_durations() {
        let source = source();
        add_session(&source, "s1", "minimax/MiniMax-M2.7-highspeed");
        add_usage(&source, 1, "s1", "t1", None, (100, 200));
        add_usage(&source, 2, "s1", "t1", None, (10, 20));
        add_usage(&source, 3, "s1", "t2", None, (1, 2));
        add_usage(&source, 4, "s2", "t1", None, (5, 6));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("minimax/MiniMax-M3"),
            (100, 200),
            Some(8500),
        );
        add_message(
            &source,
            2,
            "s1",
            "t1",
            Some("MiniMax-M2.7-highspeed"),
            (10, 20),
            Some(4200),
        );
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "fallback")
                .unwrap()
                .imported,
            4
        );
        assert_eq!(log(&db, "mcode:s1:1").0, "MiniMax-M3");
        assert_eq!(log(&db, "mcode:s1:1").2, 8500);
        assert_eq!(log(&db, "mcode:s1:2").0, "MiniMax-M2.7-highspeed");
        assert_eq!(log(&db, "mcode:s1:2").2, 4200);
        assert_eq!(log(&db, "mcode:s1:3").0, "MiniMax-M2.7-highspeed");
        assert_eq!(log(&db, "mcode:s2:4").0, "unknown");
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT SUM(latency_ms) FROM proxy_request_logs",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            12700
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(first_token_ms) FROM proxy_request_logs",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn repairs_old_unknown_model_cost_and_fabricated_first_token() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", None, (1000, 200));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (1000, 200),
            Some(4200),
        );
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "backfill").unwrap();
        let expected = log(&db, "mcode:s1:1");
        assert!(expected.1.parse::<f64>().unwrap() > 0.0);
        db.conn.lock().unwrap().execute_batch("UPDATE proxy_request_logs SET
            model='unknown',request_model='unknown',total_cost_usd='0',latency_ms=0,first_token_ms=3200").unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "backfill")
                .unwrap()
                .imported,
            1
        );
        assert_eq!(log(&db, "mcode:s1:1"), expected);
        assert_eq!(
            sync_from_database(&db, &source, "backfill")
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn late_telemetry_replaces_session_model_and_reprices_cost() {
        let source = source();
        add_session(&source, "s1", "minimax/MiniMax-M2.7-highspeed");
        add_usage(&source, 1, "s1", "t1", None, (1000, 200));
        let db = Database::memory().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "UPDATE model_pricing SET
            input_cost_per_million='1',output_cost_per_million='2'
            WHERE model_id='minimax-m2.7-highspeed';
            UPDATE model_pricing SET input_cost_per_million='3',output_cost_per_million='4'
            WHERE model_id='minimax-m3'",
            )
            .unwrap();
        sync_from_database(&db, &source, "late").unwrap();
        assert_eq!(log(&db, "mcode:s1:1").0, "MiniMax-M2.7-highspeed");
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (1000, 200),
            Some(4200),
        );
        assert_eq!(
            sync_from_database(&db, &source, "late").unwrap().imported,
            1
        );
        let actual = log(&db, "mcode:s1:1");
        assert_eq!(actual.0, "MiniMax-M3");
        assert_eq!(actual.1.parse::<f64>().unwrap(), 0.0038);
        assert_eq!((actual.2, actual.3), (4200, None));
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT request_model FROM proxy_request_logs", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "MiniMax-M3"
        );
        assert_eq!(
            sync_from_database(&db, &source, "late").unwrap().imported,
            0
        );
        // Losing old messages must preserve both confirmed model and duration.
        let confirmed = log(&db, "mcode:s1:1");
        source
            .execute("DELETE FROM local_runtime_message_rows", [])
            .unwrap();
        sync_from_database(&db, &source, "late").unwrap();
        assert_eq!(log(&db, "mcode:s1:1"), confirmed);
    }

    #[test]
    fn late_duration_updates_an_already_known_model() {
        let source = source();
        add_usage(
            &source,
            1,
            "s1",
            "t1",
            Some("minimax/MiniMax-M3"),
            (100, 200),
        );
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "late-duration").unwrap();
        add_message(&source, 1, "s1", "t1", None, (100, 200), Some(4200));
        assert_eq!(
            sync_from_database(&db, &source, "late-duration")
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            (log(&db, "mcode:s1:1").2, log(&db, "mcode:s1:1").3),
            (4200, None)
        );
        assert_eq!(
            sync_from_database(&db, &source, "late-duration")
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn request_durations_are_stable_across_sync_rounds() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", None, (100, 200));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (100, 200),
            Some(8500),
        );
        let incremental = Database::memory().unwrap();
        sync_from_database(&incremental, &source, "rounds").unwrap();
        add_usage(&source, 2, "s1", "t1", None, (10, 20));
        add_message(
            &source,
            2,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (10, 20),
            Some(4200),
        );
        assert_eq!(
            sync_from_database(&incremental, &source, "rounds")
                .unwrap()
                .imported,
            1
        );
        let batch = Database::memory().unwrap();
        sync_from_database(&batch, &source, "batch").unwrap();
        for id in [1, 2] {
            let request = format!("mcode:s1:{id}");
            assert_eq!(log(&incremental, &request), log(&batch, &request));
        }
        // Repair records written by the previous turn-level MAX implementation.
        incremental.conn.lock().unwrap().execute_batch(
            "UPDATE proxy_request_logs SET latency_ms=0,first_token_ms=0 WHERE request_id='mcode:s1:2'"
        ).unwrap();
        assert_eq!(
            sync_from_database(&incremental, &source, "rounds")
                .unwrap()
                .imported,
            1
        );
        assert_eq!(log(&incremental, "mcode:s1:2").2, 4200);
    }

    #[test]
    fn request_matching_is_scoped_to_session_and_turn() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", None, (10, 20));
        add_usage(&source, 2, "s2", "t1", None, (10, 20));
        add_usage(&source, 3, "s1", "t2", None, (10, 20));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (10, 20),
            Some(1000),
        );
        add_message(
            &source,
            2,
            "s2",
            "t1",
            Some("glm-5.3"),
            (10, 20),
            Some(2000),
        );
        add_message(
            &source,
            3,
            "s1",
            "t2",
            Some("MiniMax-M2.7-highspeed"),
            (10, 20),
            Some(3000),
        );
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "identity").unwrap();
        assert_eq!(
            (log(&db, "mcode:s1:1").0, log(&db, "mcode:s1:1").2),
            ("MiniMax-M3".into(), 1000)
        );
        assert_eq!(
            (log(&db, "mcode:s2:2").0, log(&db, "mcode:s2:2").2),
            ("glm-5.3".into(), 2000)
        );
        assert_eq!(
            (log(&db, "mcode:s1:3").0, log(&db, "mcode:s1:3").2),
            ("MiniMax-M2.7-highspeed".into(), 3000)
        );
    }

    #[test]
    fn ambiguous_message_or_usage_fingerprints_do_not_fabricate_metadata() {
        for duplicate_usage in [false, true] {
            let source = source();
            add_usage(&source, 1, "s1", "t1", None, (10, 20));
            add_message(
                &source,
                1,
                "s1",
                "t1",
                Some("MiniMax-M3"),
                (10, 20),
                Some(1000),
            );
            if duplicate_usage {
                add_usage(&source, 2, "s1", "t1", None, (10, 20));
            } else {
                add_message(
                    &source,
                    2,
                    "s1",
                    "t1",
                    Some("glm-5.3"),
                    (10, 20),
                    Some(2000),
                );
            }
            let db = Database::memory().unwrap();
            sync_from_database(&db, &source, "ambiguous").unwrap();
            assert_eq!(
                log(&db, "mcode:s1:1"),
                ("unknown".into(), "0".into(), 0, None)
            );
        }
    }

    #[test]
    fn matching_includes_cache_usage() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", None, (10, 20));
        add_usage(&source, 2, "s1", "t1", None, (10, 20));
        source.execute("UPDATE local_runtime_token_usage SET cache_read_tokens=40,cache_write_tokens=5 WHERE id=2",[]).unwrap();
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (10, 20),
            Some(1000),
        );
        add_message(
            &source,
            2,
            "s1",
            "t1",
            Some("glm-5.3"),
            (10, 20),
            Some(2000),
        );
        source
            .execute(
                "UPDATE local_runtime_message_rows SET data_json=json_set(data_json,
            '$.usage.cache_read',40,'$.usage.cache_write',5) WHERE id=2",
                [],
            )
            .unwrap();
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "cache-match").unwrap();
        assert_eq!(log(&db, "mcode:s1:1").2, 1000);
        assert_eq!(log(&db, "mcode:s1:2").2, 2000);
        assert_eq!(log(&db, "mcode:s1:2").0, "glm-5.3");
    }

    #[test]
    fn backfill_matches_full_request_identity_across_sources() {
        let source_a = source();
        add_usage(&source_a, 1, "session-A", "t1", None, (1, 2));
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source_a, "mcode:path-a").unwrap();
        let source_b = source();
        add_usage(
            &source_b,
            1,
            "session-B",
            "t1",
            Some("minimax/MiniMax-M3"),
            (5, 6),
        );
        assert_eq!(
            sync_from_database(&db, &source_b, "mcode:path-b")
                .unwrap()
                .imported,
            1
        );
        assert_eq!(log(&db, "mcode:session-A:1").0, "unknown");
        assert_eq!(log(&db, "mcode:session-B:1").0, "MiniMax-M3");
    }

    #[test]
    fn missing_metadata_tables_and_invalid_models_fall_back_safely() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", Some("unknown"), (10, 20));
        add_session(&source, "s1", "minimax/MiniMax-M3");
        source
            .execute_batch("DROP TABLE local_runtime_message_rows")
            .unwrap();
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "old-source").unwrap();
        assert_eq!(log(&db, "mcode:s1:1").0, "MiniMax-M3");
        source
            .execute_batch("DROP TABLE local_runtime_sessions")
            .unwrap();
        add_usage(&source, 2, "s2", "t2", Some("  "), (1, 2));
        sync_from_database(&db, &source, "old-source").unwrap();
        assert_eq!(
            log(&db, "mcode:s2:2"),
            ("unknown".into(), "0".into(), 0, None)
        );
    }

    #[test]
    fn old_session_schema_without_extra_data_still_imports_tokens() {
        let source = source();
        source
            .execute_batch(
                "DROP TABLE local_runtime_sessions;
             CREATE TABLE local_runtime_sessions (
                session_id TEXT PRIMARY KEY,record_json TEXT,updated_at_ms INTEGER);
             INSERT INTO local_runtime_sessions VALUES ('s1','{}',0);",
            )
            .unwrap();
        add_usage(&source, 1, "s1", "t1", None, (100, 200));
        add_usage(&source, 2, "s1", "t2", None, (10, 20));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (100, 200),
            Some(8500),
        );
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "legacy-schema")
                .unwrap()
                .imported,
            2
        );
        assert_eq!(log(&db, "mcode:s1:1").0, "MiniMax-M3");
        assert_eq!(log(&db, "mcode:s1:2").0, "unknown");
        let totals: (i64, i64) = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT SUM(input_tokens),SUM(output_tokens) FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(totals, (110, 220));
        assert_eq!(
            sync_from_database(&db, &source, "legacy-schema")
                .unwrap()
                .imported,
            0
        );
    }

    fn costs(db: &Database, request: &str) -> [Decimal; 5] {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT input_cost_usd,output_cost_usd,cache_read_cost_usd,
                    cache_creation_cost_usd,total_cost_usd
             FROM proxy_request_logs WHERE request_id=?1",
                [request],
                |row| {
                    Ok(std::array::from_fn(|i| {
                        row.get::<_, String>(i).unwrap().parse().unwrap()
                    }))
                },
            )
            .unwrap()
    }

    #[test]
    fn calculated_cost_components_are_written_on_insert_and_historical_repair() {
        let source = source();
        add_usage(
            &source,
            1,
            "s1",
            "t1",
            Some("minimax/MiniMax-M3"),
            (1000, 200),
        );
        source
            .execute_batch(
                "UPDATE local_runtime_token_usage
            SET cache_read_tokens=400,cache_write_tokens=50,ts=100000",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "UPDATE model_pricing SET input_cost_per_million='1',output_cost_per_million='2',
                cache_read_cost_per_million='3',cache_creation_cost_per_million='4'
             WHERE model_id='minimax-m3'",
            )
            .unwrap();
        sync_from_database(&db, &source, "components").unwrap();
        let expected = ["0.001", "0.0004", "0.0012", "0.0002", "0.0028"]
            .map(|v| v.parse::<Decimal>().unwrap());
        assert_eq!(costs(&db, "mcode:s1:1"), expected);
        // Simulate a prior importer: old, known-model, positive-total row whose
        // components were zero. The one-time repair must include it despite its age.
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "UPDATE proxy_request_logs SET input_cost_usd='0',output_cost_usd='0',
                cache_read_cost_usd='0',cache_creation_cost_usd='0';
             DELETE FROM session_log_sync WHERE file_path='components:request-cost-repair-v1'",
            )
            .unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "components")
                .unwrap()
                .imported,
            1
        );
        assert_eq!(costs(&db, "mcode:s1:1"), expected);
        assert_eq!(
            sync_from_database(&db, &source, "components")
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn native_cost_keeps_its_total_and_no_invented_components() {
        let source = source();
        add_usage(
            &source,
            1,
            "s1",
            "t1",
            Some("minimax/MiniMax-M3"),
            (1000, 200),
        );
        source
            .execute_batch("UPDATE local_runtime_token_usage SET cost_usd=0.25")
            .unwrap();
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "native-cost").unwrap();
        assert_eq!(
            costs(&db, "mcode:s1:1"),
            [
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::new(25, 2)
            ]
        );
        assert_eq!(
            sync_from_database(&db, &source, "native-cost")
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn ambiguous_or_incomplete_telemetry_keeps_confirmed_duration() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", None, (10, 20));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("MiniMax-M3"),
            (10, 20),
            Some(8500),
        );
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "preserve-duration").unwrap();
        let confirmed = log(&db, "mcode:s1:1");
        source
            .execute_batch(
                "UPDATE local_runtime_message_rows SET
            data_json=json_remove(data_json,'$.usage.request_duration_ms')",
            )
            .unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "preserve-duration")
                .unwrap()
                .imported,
            0
        );
        assert_eq!(log(&db, "mcode:s1:1"), confirmed);
        add_message(
            &source,
            2,
            "s1",
            "t1",
            Some("glm-5.3"),
            (10, 20),
            Some(2000),
        );
        assert_eq!(
            sync_from_database(&db, &source, "preserve-duration")
                .unwrap()
                .imported,
            0
        );
        assert_eq!(log(&db, "mcode:s1:1"), confirmed);
    }

    #[test]
    fn steady_reconciliation_is_bounded_but_keeps_old_repair_candidates() {
        let source = source();
        for id in 1..=4 {
            add_usage(
                &source,
                id,
                "s1",
                &format!("t{id}"),
                Some("minimax/MiniMax-M3"),
                (10, 20),
            );
            add_message(
                &source,
                id,
                "s1",
                &format!("t{id}"),
                Some("MiniMax-M3"),
                (10, 20),
                Some(8500),
            );
        }
        source
            .execute_batch("UPDATE local_runtime_token_usage SET ts=100000 WHERE id<4")
            .unwrap();
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "bounded").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "UPDATE proxy_request_logs SET model='unknown',request_model='unknown'
                WHERE request_id='mcode:s1:2';
                UPDATE proxy_request_logs SET first_token_ms=3200 WHERE request_id='mcode:s1:3'",
            )
            .unwrap();
            let cutoff = chrono::Utc::now().timestamp() - RECONCILE_WINDOW_SECONDS;
            let selected = retained_logs(&conn, false, cutoff).unwrap();
            assert_eq!(selected.len(), 3);
            assert!(!selected.contains_key("mcode:s1:1"));
            for id in [2, 3, 4] {
                assert!(selected.contains_key(&format!("mcode:s1:{id}")));
            }
        }
        let result = sync_from_database(&db, &source, "bounded").unwrap();
        assert_eq!((result.imported, result.skipped), (2, 1));
        assert_eq!(log(&db, "mcode:s1:2").0, "MiniMax-M3");
        assert_eq!(log(&db, "mcode:s1:3").3, None);
        let result = sync_from_database(&db, &source, "bounded").unwrap();
        assert_eq!((result.imported, result.skipped), (0, 1));
    }

    #[test]
    fn global_duplicate_detection_includes_rows_outside_reconciliation_window() {
        let source = source();
        add_usage(&source, 1, "s1", "t1", Some("minimax/MiniMax-M3"), (10, 20));
        source
            .execute_batch("UPDATE local_runtime_token_usage SET ts=100000")
            .unwrap();
        let db = Database::memory().unwrap();
        sync_from_database(&db, &source, "global-duplicates").unwrap();
        add_usage(&source, 2, "s1", "t1", None, (10, 20));
        add_message(
            &source,
            1,
            "s1",
            "t1",
            Some("glm-5.3"),
            (10, 20),
            Some(8500),
        );
        sync_from_database(&db, &source, "global-duplicates").unwrap();
        assert_eq!(log(&db, "mcode:s1:1").0, "MiniMax-M3");
        assert_eq!(
            log(&db, "mcode:s1:2"),
            ("unknown".into(), "0".into(), 0, None)
        );
    }

    #[test]
    #[ignore = "manual performance measurement; run with --ignored --nocapture"]
    fn reconciliation_benchmark() {
        for count in [5000, 20000] {
            for recent in [false, true] {
                let mut source = source();
                let tx = source.transaction().unwrap();
                for id in 1..=count {
                    add_usage(
                        &tx,
                        id,
                        "s1",
                        &format!("t{id}"),
                        Some("minimax/MiniMax-M3"),
                        (10, 20),
                    );
                }
                tx.commit().unwrap();
                if !recent {
                    source
                        .execute_batch("UPDATE local_runtime_token_usage SET ts=100000")
                        .unwrap();
                }
                let db = Database::memory().unwrap();
                let start = std::time::Instant::now();
                sync_from_database(&db, &source, "benchmark").unwrap();
                let first = start.elapsed();
                let start = std::time::Instant::now();
                let result = sync_from_database(&db, &source, "benchmark").unwrap();
                eprintln!(
                    "rows={count} recent={recent} first={first:?} idle={:?} reconciled={}",
                    start.elapsed(),
                    result.skipped
                );
                assert_eq!(result.imported, 0);
                assert_eq!(result.skipped, if recent { count as u32 } else { 0 });
            }
        }
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;
    #[test]
    #[ignore = "requires explicit read-only CCS and MCode snapshots"]
    fn repairs_existing_usage_snapshot_without_changing_tokens() -> Result<(), AppError> {
        let path = std::env::var("CC_SWITCH_VALIDATION_DB").expect("set snapshot database path");
        assert!(std::env::var("CC_SWITCH_TEST_HOME").is_ok());
        assert!(std::env::var("MINIMAX_DATA_DIR").is_ok());
        let input = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let db = Database::memory()?;
        {
            // Reconcile an in-memory backup, never the supplied CCS database file.
            let mut conn = lock_conn!(db.conn);
            let backup = rusqlite::backup::Backup::new(&input, &mut conn)?;
            backup.run_to_completion(100, std::time::Duration::from_millis(1), None)?;
        }
        fn totals(db: &Database) -> (i64, i64, i64, i64, i64) {
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*),COALESCE(SUM(input_tokens),0),COALESCE(SUM(output_tokens),0),
                 COALESCE(SUM(cache_read_tokens),0),COALESCE(SUM(cache_creation_tokens),0)
                 FROM proxy_request_logs WHERE data_source='mcode_session'",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .unwrap()
        }
        fn missing_components(db: &Database) -> i64 {
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source='mcode_session'
                 AND CAST(total_cost_usd AS REAL)>0 AND CAST(input_cost_usd AS REAL)=0
                 AND CAST(output_cost_usd AS REAL)=0 AND CAST(cache_read_cost_usd AS REAL)=0
                 AND CAST(cache_creation_cost_usd AS REAL)=0",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        }
        let source = mcode::open_database()?;
        let key = format!("mcode:{}", mcode::database_path().display());
        let max_id: i64 = source.query_row(
            "SELECT COALESCE(MAX(id),0) FROM local_runtime_token_usage",
            [],
            |row| row.get(0),
        )?;
        // This check focuses on reconciliation of existing records, preserving pruning.
        db.conn.lock().unwrap().execute(
            "INSERT INTO session_log_sync(file_path,last_modified,last_line_offset,last_synced_at)
             VALUES (?1,0,?2,unixepoch()) ON CONFLICT(file_path) DO UPDATE SET last_line_offset=?2",
            params![key, max_id],
        )?;
        let before = totals(&db);
        let missing_before = missing_components(&db);
        let updated = sync_mcode_usage(&db)?;
        assert_eq!(totals(&db), before);
        let missing_after = missing_components(&db);
        let mut native_totals = HashSet::new();
        {
            let mut stmt = source.prepare(
                "SELECT session_id,id FROM local_runtime_token_usage WHERE cost_usd > 0",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                native_totals.insert(format!(
                    "mcode:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?
                ));
            }
        }
        // Native positive totals legitimately have no component breakdown.
        let mut native_without_components = 0;
        {
            let conn = lock_conn!(db.conn);
            let mut stmt = conn.prepare(
                "SELECT input_cost_usd,output_cost_usd,cache_read_cost_usd,
                    cache_creation_cost_usd,total_cost_usd,request_id FROM proxy_request_logs
                 WHERE data_source='mcode_session'",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let values: Vec<Decimal> = (0..5)
                    .map(|i| row.get::<_, String>(i).unwrap().parse().unwrap())
                    .collect();
                if native_totals.contains(&row.get::<_, String>(5)?) && values[4] > Decimal::ZERO {
                    native_without_components += 1;
                } else {
                    assert_eq!(values[..4].iter().copied().sum::<Decimal>(), values[4]);
                }
            }
        }
        assert_eq!(missing_after, native_without_components);
        assert_eq!(sync_mcode_usage(&db)?.imported, 0);
        eprintln!("copied CCS rows={} repaired={} missing_components_before={} after={} tokens_unchanged=true idempotent=true",
            before.0,updated.imported,missing_before,missing_after);
        Ok(())
    }

    #[test]
    #[ignore = "requires the real MCode workflow in an isolated CC_SWITCH_TEST_HOME"]
    fn mcode_reads_real_sessions_and_usage() -> Result<(), AppError> {
        assert!(std::env::var("CC_SWITCH_TEST_HOME").is_ok());
        let sessions = mcode::scan_sessions();
        assert!(!sessions.is_empty());
        let mut messages = 0;
        for session in &sessions {
            messages += mcode::load_messages(session.source_path.as_deref().unwrap())
                .unwrap()
                .len();
        }
        assert!(messages >= 2);
        let db = Database::memory()?;
        let imported = sync_mcode_usage(&db)?;
        assert!(imported.imported > 0);
        assert_eq!(sync_mcode_usage(&db)?.imported, 0);
        let native = mcode::open_database()?;
        let expected=native.query_row("SELECT SUM(input_tokens), SUM(output_tokens + reasoning_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens) FROM local_runtime_token_usage",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?;
        let conn = lock_conn!(db.conn);
        let actual=conn.query_row("SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_creation_tokens) FROM proxy_request_logs WHERE app_type='mcode'",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
