//! Hermes Agent session usage tracking.
//!
//! Reads token and cost totals from Hermes' local SQLite state database and
//! imports one usage row per session into `proxy_request_logs`.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::hermes_config::get_hermes_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{get_sync_state, update_sync_state, SessionSyncResult};
use crate::services::usage_stats::{has_matching_proxy_usage_log, DedupKey};
use rusqlite::{Connection, OpenFlags};
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const HERMES_APP_TYPE: &str = "hermes";
const HERMES_PROVIDER_ID: &str = "_hermes_session";
const HERMES_DATA_SOURCE: &str = "hermes_session";
const HERMES_SYNC_PREFIX: &str = "sqlite:";
const WATERMARK_OVERLAP_MS: i64 = 300_000;
const ACTIVE_SESSION_LOOKBACK_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug)]
struct HermesSessionUsage {
    session_id: String,
    model: String,
    timestamp_ms: i64,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    total_cost_usd: Option<String>,
}

/// Sync Hermes usage data from state.db and profile state databases.
pub fn sync_hermes_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let hermes_dir = get_hermes_dir();
    let db_paths = collect_hermes_db_paths(&hermes_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: db_paths.len() as u32,
        errors: vec![],
    };

    for db_path in &db_paths {
        match sync_single_hermes_db(db, db_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Hermes 数据库同步失败 {}: {e}", db_path.display());
                log::warn!("[HERMES-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[HERMES-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个数据库",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

fn collect_hermes_db_paths(hermes_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let root_db = hermes_dir.join("state.db");
    if root_db.is_file() {
        paths.push(root_db);
    }

    let profiles_dir = hermes_dir.join("profiles");
    if let Ok(entries) = fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let profile_db = entry.path().join("state.db");
            if profile_db.is_file() {
                paths.push(profile_db);
            }
        }
    }

    paths
}

fn sync_single_hermes_db(db: &Database, db_path: &Path) -> Result<(u32, u32), AppError> {
    let sync_key = hermes_sync_key(db_path);
    let file_modified = sqlite_db_modified_ms(db_path);
    let (last_modified, last_watermark) = get_sync_state(db, &sync_key)?;

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Database(format!("打开 Hermes 数据库失败: {e}")))?;

    ensure_sessions_table(&conn)?;
    let columns = get_table_columns(&conn, "sessions")?;
    validate_hermes_columns(&columns)?;

    let threshold = last_watermark.saturating_sub(WATERMARK_OVERLAP_MS);
    let sessions = load_hermes_sessions(&conn, &columns, threshold)?;

    let mut imported = 0;
    let mut skipped = 0;
    let mut max_watermark = last_watermark;

    for session in &sessions {
        max_watermark = max_watermark.max(session.timestamp_ms);
        match insert_hermes_session_entry(db, session) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[HERMES-SYNC] 插入失败 ({}): {e}", session.session_id);
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &sync_key, file_modified, max_watermark)?;

    Ok((imported, skipped))
}

fn hermes_sync_key(db_path: &Path) -> String {
    let absolute = db_path
        .canonicalize()
        .unwrap_or_else(|_| db_path.to_path_buf());
    format!("{HERMES_SYNC_PREFIX}{}", absolute.display())
}

fn sqlite_db_modified_ms(db_path: &Path) -> i64 {
    [
        db_path.to_path_buf(),
        sqlite_sidecar_path(db_path, "-wal"),
        sqlite_sidecar_path(db_path, "-shm"),
    ]
    .iter()
    .filter_map(|path| fs::metadata(path).ok())
    .filter_map(|metadata| metadata.modified().ok())
    .filter_map(system_time_to_ms)
    .max()
    .unwrap_or(0)
}

fn sqlite_sidecar_path(db_path: &Path, suffix: &str) -> PathBuf {
    let mut raw = db_path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

fn system_time_to_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

fn ensure_sessions_table(conn: &Connection) -> Result<(), AppError> {
    let has_sessions = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='sessions'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);

    if has_sessions {
        Ok(())
    } else {
        Err(AppError::Database(
            "Hermes 数据库缺少 sessions 表".to_string(),
        ))
    }
}

fn get_table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, AppError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| AppError::Database(format!("读取 Hermes 表结构失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| AppError::Database(format!("读取 Hermes 字段失败: {e}")))?;

    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(columns)
}

fn validate_hermes_columns(columns: &HashSet<String>) -> Result<(), AppError> {
    if !columns.contains("id") {
        return Err(AppError::Database(
            "Hermes sessions 表缺少 id 字段".to_string(),
        ));
    }

    if !columns.contains("started_at") && !columns.contains("ended_at") {
        return Err(AppError::Database(
            "Hermes sessions 表缺少 started_at/ended_at 字段".to_string(),
        ));
    }

    Ok(())
}

fn load_hermes_sessions(
    conn: &Connection,
    columns: &HashSet<String>,
    threshold_ms: i64,
) -> Result<Vec<HermesSessionUsage>, AppError> {
    let timestamp_expr = timestamp_expr(columns);
    let timestamp_ms_expr =
        format!("CAST(ROUND(CAST(({timestamp_expr}) AS REAL) * 1000.0) AS INTEGER)");
    let active_session_clause = if columns.contains("ended_at") && columns.contains("started_at") {
        format!(
            " OR (ended_at IS NULL AND {timestamp_ms_expr} > ?1 - {ACTIVE_SESSION_LOOKBACK_MS})"
        )
    } else {
        String::new()
    };

    let sql = format!(
        "SELECT
            CAST(id AS TEXT) AS id,
            {model_expr} AS model,
            {timestamp_ms_expr} AS timestamp_ms,
            {input_expr} AS input_tokens,
            {output_expr} AS output_tokens,
            {reasoning_expr} AS reasoning_tokens,
            {cache_read_expr} AS cache_read_tokens,
            {cache_write_expr} AS cache_write_tokens,
            {actual_cost_expr} AS actual_cost_usd,
            {estimated_cost_expr} AS estimated_cost_usd
         FROM sessions
         WHERE ({timestamp_ms_expr} > ?1{active_session_clause})
         ORDER BY {timestamp_ms_expr} ASC",
        model_expr = text_expr(columns, "model", "unknown"),
        input_expr = int_expr(columns, "input_tokens"),
        output_expr = int_expr(columns, "output_tokens"),
        reasoning_expr = int_expr(columns, "reasoning_tokens"),
        cache_read_expr = int_expr(columns, "cache_read_tokens"),
        cache_write_expr = int_expr(columns, "cache_write_tokens"),
        actual_cost_expr = cost_expr(columns, "actual_cost_usd"),
        estimated_cost_expr = cost_expr(columns, "estimated_cost_usd"),
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AppError::Database(format!("准备 Hermes 查询失败: {e}")))?;

    let rows = stmt
        .query_map([threshold_ms], |row| {
            let output_tokens = get_i64(row, 4).saturating_add(get_i64(row, 5));
            let actual_cost: Option<String> = row.get(8)?;
            let estimated_cost: Option<String> = row.get(9)?;

            Ok(HermesSessionUsage {
                session_id: row.get(0)?,
                model: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "unknown".to_string()),
                timestamp_ms: get_i64(row, 2),
                input_tokens: to_u32(get_i64(row, 3)),
                output_tokens: to_u32(output_tokens),
                cache_read_tokens: to_u32(get_i64(row, 6)),
                cache_creation_tokens: to_u32(get_i64(row, 7)),
                total_cost_usd: actual_cost
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| estimated_cost.filter(|s| !s.trim().is_empty())),
            })
        })
        .map_err(|e| AppError::Database(format!("查询 Hermes sessions 失败: {e}")))?;

    let mut sessions = Vec::new();
    for row in rows {
        let session = row.map_err(|e| AppError::Database(e.to_string()))?;
        if session.session_id.trim().is_empty() || session.timestamp_ms <= 0 {
            continue;
        }
        sessions.push(session);
    }

    Ok(sessions)
}

fn timestamp_expr(columns: &HashSet<String>) -> String {
    match (columns.contains("ended_at"), columns.contains("started_at")) {
        (true, true) => "COALESCE(ended_at, started_at)".to_string(),
        (true, false) => "ended_at".to_string(),
        (false, true) => "started_at".to_string(),
        (false, false) => "NULL".to_string(),
    }
}

fn text_expr(columns: &HashSet<String>, column: &str, default: &str) -> String {
    if columns.contains(column) {
        format!("COALESCE(NULLIF(TRIM(CAST({column} AS TEXT)), ''), '{default}')")
    } else {
        format!("'{default}'")
    }
}

fn int_expr(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        format!("CAST(COALESCE({column}, 0) AS INTEGER)")
    } else {
        "0".to_string()
    }
}

fn cost_expr(columns: &HashSet<String>, column: &str) -> String {
    if columns.contains(column) {
        format!("NULLIF(TRIM(CAST({column} AS TEXT)), '')")
    } else {
        "NULL".to_string()
    }
}

fn get_i64(row: &rusqlite::Row<'_>, index: usize) -> i64 {
    row.get::<_, Option<i64>>(index).ok().flatten().unwrap_or(0)
}

fn to_u32(value: i64) -> u32 {
    value.max(0).min(u32::MAX as i64) as u32
}

fn insert_hermes_session_entry(
    db: &Database,
    session: &HermesSessionUsage,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    let created_at = session.timestamp_ms / 1000;
    let request_id = format!("hermes-session-{}", session.session_id);

    let dedup_key = DedupKey {
        app_type: HERMES_APP_TYPE,
        model: &session.model,
        input_tokens: session.input_tokens,
        output_tokens: session.output_tokens,
        cache_read_tokens: session.cache_read_tokens,
        cache_creation_tokens: session.cache_creation_tokens,
        created_at,
    };
    // Hermes rows use UPSERT so repeated request_ids can refresh estimated cost
    // into actual cost. This check only prevents double-counting when a matching
    // proxy log already covers the same usage window.
    if has_matching_proxy_usage_log(&conn, &dedup_key)? {
        return Ok(false);
    }

    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match &session.total_cost_usd {
            Some(total) => (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                total.clone(),
            ),
            None => calculate_fallback_cost(&conn, session),
        };

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            request_model = excluded.request_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            created_at = excluded.created_at
        WHERE model != excluded.model
           OR request_model != excluded.request_model
           OR input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR cache_creation_tokens != excluded.cache_creation_tokens
           OR input_cost_usd != excluded.input_cost_usd
           OR output_cost_usd != excluded.output_cost_usd
           OR cache_read_cost_usd != excluded.cache_read_cost_usd
           OR cache_creation_cost_usd != excluded.cache_creation_cost_usd
           OR total_cost_usd != excluded.total_cost_usd
           OR created_at != excluded.created_at",
        rusqlite::params![
            request_id,
            HERMES_PROVIDER_ID,
            HERMES_APP_TYPE,
            session.model,
            session.model,
            session.input_tokens,
            session.output_tokens,
            session.cache_read_tokens,
            session.cache_creation_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            session.session_id,
            Some(HERMES_DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            HERMES_DATA_SOURCE,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Hermes 会话用量失败: {e}")))?;

    Ok(conn.changes() > 0)
}

fn calculate_fallback_cost(
    conn: &Connection,
    session: &HermesSessionUsage,
) -> (String, String, String, String, String) {
    let usage = TokenUsage {
        input_tokens: session.input_tokens,
        output_tokens: session.output_tokens,
        cache_read_tokens: session.cache_read_tokens,
        cache_creation_tokens: session.cache_creation_tokens,
        model: Some(session.model.clone()),
        message_id: None,
    };

    match find_hermes_pricing(conn, &session.model) {
        Some(pricing) => {
            let cost = CostCalculator::calculate(&usage, &pricing, Decimal::from(1));
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            )
        }
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
        ),
    }
}

fn find_hermes_pricing(conn: &Connection, model_id: &str) -> Option<ModelPricing> {
    let candidates = pricing_candidates(model_id);
    for candidate in &candidates {
        if let Some(pricing) = try_find_pricing(conn, candidate) {
            return Some(pricing);
        }
    }

    for candidate in &candidates {
        let pattern = format!("{candidate}%");
        if let Ok((input, output, cache_read, cache_creation)) = conn.query_row(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing WHERE model_id LIKE ?1 LIMIT 1",
            rusqlite::params![pattern],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        ) {
            if let Ok(pricing) =
                ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
            {
                return Some(pricing);
            }
        }
    }

    None
}

fn pricing_candidates(model_id: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let raw = model_id.trim();
    if !raw.is_empty() {
        candidates.push(raw.to_string());
        if let Some((_, tail)) = raw.rsplit_once('/') {
            if !tail.is_empty() && tail != raw {
                candidates.push(tail.to_string());
            }
        }
    }
    candidates
}

fn try_find_pricing(conn: &Connection, model_id: &str) -> Option<ModelPricing> {
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing WHERE model_id = ?1",
        rusqlite::params![model_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .ok()
    .and_then(|(input, output, cache_read, cache_creation)| {
        ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::session_usage::get_data_source_breakdown;
    use crate::services::usage_stats::LogFilters;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn create_hermes_db(path: &Path) -> Connection {
        let conn = Connection::open(path).expect("open hermes db");
        conn.execute(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                model TEXT,
                started_at REAL,
                ended_at REAL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                reasoning_tokens INTEGER,
                actual_cost_usd REAL,
                estimated_cost_usd REAL
            )",
            [],
        )
        .expect("create sessions");
        conn
    }

    fn insert_session(
        conn: &Connection,
        id: &str,
        ended_at: f64,
        actual_cost: Option<f64>,
        estimated_cost: Option<f64>,
    ) {
        conn.execute(
            "INSERT INTO sessions (
                id, model, started_at, ended_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens,
                actual_cost_usd, estimated_cost_usd
            ) VALUES (?1, 'openrouter/anthropic/claude-opus-4-7', ?2, ?3, 10, 20, 3, 4, 7, ?4, ?5)",
            rusqlite::params![id, ended_at - 10.0, ended_at, actual_cost, estimated_cost],
        )
        .expect("insert session");
    }

    fn insert_active_session(conn: &Connection, id: &str, started_at: f64) {
        conn.execute(
            "INSERT INTO sessions (
                id, model, started_at, ended_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens,
                actual_cost_usd, estimated_cost_usd
            ) VALUES (?1, 'openrouter/anthropic/claude-opus-4-7', ?2, NULL, 10, 20, 3, 4, 7, 0.1, NULL)",
            rusqlite::params![id, started_at],
        )
        .expect("insert active session");
    }

    #[test]
    fn test_ensure_sessions_table_errors_when_missing() {
        let conn = Connection::open_in_memory().expect("open memory db");
        let err = ensure_sessions_table(&conn).expect_err("missing sessions should error");
        assert!(err.to_string().contains("缺少 sessions 表"));
    }

    #[test]
    fn test_validate_hermes_columns_rejects_missing_id() {
        let columns = HashSet::from(["started_at".to_string()]);
        let err = validate_hermes_columns(&columns).expect_err("missing id should error");
        assert!(err.to_string().contains("缺少 id 字段"));
    }

    #[test]
    fn test_validate_hermes_columns_rejects_missing_timestamp() {
        let columns = HashSet::from(["id".to_string()]);
        let err = validate_hermes_columns(&columns).expect_err("missing timestamp should error");
        assert!(err.to_string().contains("缺少 started_at/ended_at 字段"));
    }

    #[test]
    fn test_sync_hermes_session_imports_tokens_and_actual_cost() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, Some(0.123456), Some(0.5));
        drop(hermes_conn);

        let db = Database::memory()?;
        let (imported, skipped) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!((imported, skipped), (1, 0));

        let conn = lock_conn!(db.conn);
        let row: (String, String, i64, i64, i64, String, String) = conn.query_row(
            "SELECT app_type, data_source, input_tokens, output_tokens, cache_creation_tokens, total_cost_usd, provider_id
             FROM proxy_request_logs WHERE request_id = 'hermes-session-s1'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )?;
        assert_eq!(row.0, "hermes");
        assert_eq!(row.1, HERMES_DATA_SOURCE);
        assert_eq!(row.2, 10);
        assert_eq!(row.3, 27);
        assert_eq!(row.4, 4);
        assert_eq!(row.5, "0.123456");
        assert_eq!(row.6, HERMES_PROVIDER_ID);
        Ok(())
    }

    #[test]
    fn test_load_hermes_sessions_limits_active_zombie_sessions() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_active_session(&hermes_conn, "old-active", 1_000.0);
        insert_active_session(&hermes_conn, "recent-active", 150_000.0);

        let columns = get_table_columns(&hermes_conn, "sessions")?;
        let sessions = load_hermes_sessions(&hermes_conn, &columns, 200_000_000)?;
        let ids: Vec<&str> = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();

        assert!(!ids.contains(&"old-active"));
        assert!(ids.contains(&"recent-active"));
        Ok(())
    }

    #[test]
    fn test_sync_state_is_per_database_and_watermark_incremental() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, Some(0.1), None);
        drop(hermes_conn);

        let db = Database::memory()?;
        let (imported, _) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!(imported, 1);

        let sync_key = hermes_sync_key(&db_path);
        {
            let conn = lock_conn!(db.conn);
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1",
                [sync_key.as_str()],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
        }

        thread::sleep(Duration::from_millis(5));
        let hermes_conn = Connection::open(&db_path).expect("reopen hermes db");
        insert_session(&hermes_conn, "s2", 1100.0, Some(0.2), None);
        drop(hermes_conn);

        let (imported, skipped) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!(imported, 1);
        assert!(skipped >= 1);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);
        let max_offset: i64 = conn.query_row(
            "SELECT last_line_offset FROM session_log_sync WHERE file_path = ?1",
            [sync_key.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(max_offset, 1_100_000);
        Ok(())
    }

    #[test]
    fn test_sqlite_mtime_includes_wal_and_shm_sidecars() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        fs::write(&db_path, b"db").expect("write db");
        let db_mtime = sqlite_db_modified_ms(&db_path);
        thread::sleep(Duration::from_millis(20));
        let wal_path = sqlite_sidecar_path(&db_path, "-wal");
        fs::write(&wal_path, b"wal").expect("write wal");
        let with_wal = sqlite_db_modified_ms(&db_path);
        assert!(with_wal >= db_mtime);
        assert!(with_wal > 0);
    }

    #[test]
    fn test_fallback_cost_uses_model_pricing_when_hermes_cost_missing() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, None, None);
        drop(hermes_conn);

        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES ('claude-opus-4-7', 'Claude Opus 4.7', '10', '20', '1', '5')",
                [],
            )?;
        }

        let (imported, _) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!(imported, 1);

        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'hermes-session-s1'",
            [],
            |row| row.get(0),
        )?;
        assert_ne!(total, "0");
        Ok(())
    }

    #[test]
    fn test_estimated_cost_used_when_actual_cost_missing() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, None, Some(0.456789));
        drop(hermes_conn);

        let db = Database::memory()?;
        let (imported, _) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!(imported, 1);

        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'hermes-session-s1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total, "0.456789");
        Ok(())
    }

    #[test]
    fn test_matching_proxy_log_skips_hermes_session_insert() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, Some(0.1), None);
        drop(hermes_conn);

        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES ('proxy-1', 'p1', 'hermes', 'openrouter/anthropic/claude-opus-4-7',
                    'openrouter/anthropic/claude-opus-4-7', 10, 27, 3, 4, '0.1', 100, 200, 1000, 'proxy')",
                [],
            )?;
        }

        let (imported, skipped) = sync_single_hermes_db(&db, &db_path)?;
        assert_eq!(imported, 0);
        assert_eq!(skipped, 1);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_hermes_provider_name_data_source_and_app_filter() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("state.db");
        let hermes_conn = create_hermes_db(&db_path);
        insert_session(&hermes_conn, "s1", 1000.0, Some(0.1), None);
        drop(hermes_conn);

        let db = Database::memory()?;
        sync_single_hermes_db(&db, &db_path)?;

        let provider_stats = db.get_provider_stats(None, None, Some("hermes"))?;
        assert_eq!(provider_stats.len(), 1);
        assert_eq!(provider_stats[0].provider_id, HERMES_PROVIDER_ID);
        assert_eq!(provider_stats[0].provider_name, "Hermes (Session)");

        let sources = get_data_source_breakdown(&db)?;
        assert!(sources
            .iter()
            .any(|source| source.data_source == HERMES_DATA_SOURCE && source.request_count == 1));

        let logs = db.get_request_logs(
            &LogFilters {
                app_type: Some("hermes".to_string()),
                ..Default::default()
            },
            0,
            20,
        )?;
        assert_eq!(logs.total, 1);
        assert_eq!(
            logs.data[0].provider_name.as_deref(),
            Some("Hermes (Session)")
        );
        Ok(())
    }

    #[test]
    fn test_collect_hermes_profile_databases() {
        let temp = tempdir().expect("tempdir");
        fs::write(temp.path().join("state.db"), b"root").expect("write root");
        let profile_dir = temp.path().join("profiles").join("work");
        fs::create_dir_all(&profile_dir).expect("profile dir");
        fs::write(profile_dir.join("state.db"), b"profile").expect("write profile");

        let paths = collect_hermes_db_paths(temp.path());
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|path| path.ends_with("state.db")));
        assert!(paths
            .iter()
            .any(|path| path.to_string_lossy().contains("profiles/work/state.db")));
    }
}
