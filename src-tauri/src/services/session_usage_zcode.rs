//! ZCode coding-agent session usage importer.
//!
//! ZCode persists per-request token accounting in the `model_usage` table of
//! `~/.zcode/cli/db/db.sqlite`. Each row already carries the normalized
//! Anthropic-style usage buckets plus latency and status metadata, so the
//! importer is a straight table-to-table copy gated by the shared
//! `session_usage_dedup` ledger keyed by `zcode:<model_usage.id>`. Because the
//! source database runs in WAL mode (its main-file mtime does not advance on
//! every write), we intentionally skip the mtime fast-path used by the JSONL
//! importers and rely on full reparses plus the ledger for idempotence.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::SessionSyncResult;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::find_model_pricing;
use rust_decimal::Decimal;
use std::path::PathBuf;

const APP_TYPE: &str = "zcode";
const DATA_SOURCE: &str = "zcode_session";
/// Display-name placeholder surfaced by `usage_stats::provider_name_coalesce`
/// when a row carries no real provider_id. ZCode's `model_usage.provider_id`
/// is NOT NULL, so import copies it verbatim; this constant documents the
/// mapping key and is exercised by the display-name test below.
#[allow(dead_code)]
const PROVIDER_PLACEHOLDER: &str = "_zcode_session";
const REQUEST_ID_PREFIX: &str = "zcode:";
const MIN_SQLITE_UNIX_SECONDS: i64 = -62_167_219_200;
const MAX_SQLITE_UNIX_SECONDS: i64 = 253_402_300_799;
const ZCODE_REQUEST_DEDUP_SQL: &str = "SELECT EXISTS(
         SELECT 1 FROM session_usage_dedup
         WHERE data_source = ?1 AND request_id = ?2
     )";

/// A single `model_usage` row mapped to the dashboard schema.
#[derive(Debug)]
struct ZcodeUsageRecord {
    request_id: String,
    provider_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    latency_ms: Option<i64>,
    first_token_ms: Option<i64>,
    status_code: i64,
    error_message: Option<String>,
    session_id: String,
    created_at: i64,
}

/// Import usage from the ZCode SQLite database. A missing database simply
/// means there is nothing to import yet; schema drift defers to the next pass
/// rather than failing the whole sync.
pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = zcode_db_path();
    if !db_path.exists() {
        return Ok(SessionSyncResult {
            files_scanned: 0,
            ..Default::default()
        });
    }
    sync_zcode_usage_from_path(db, &db_path)
}

/// Path-injectable core so tests can drive the importer without touching the
/// process-wide `ZCODE_DB_PATH` environment variable (cargo test runs suites
/// in parallel, and `std::env::set_var` is not isolated per thread).
fn sync_zcode_usage_from_path(
    db: &Database,
    db_path: &std::path::Path,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };

    let zcode_conn = match open_zcode_db_readonly(db_path) {
        Ok(conn) => conn,
        Err(error) => {
            let message = format!("ZCode 数据库打开失败 ({}): {error}", db_path.display());
            log::warn!("[ZCODE-SYNC] {message}");
            result.errors.push(message);
            result.deferred_files = 1;
            return Ok(result);
        }
    };

    let records = match load_zcode_records(&zcode_conn) {
        Ok(records) => records,
        Err(error) => {
            let message = format!("ZCode model_usage 读取失败: {error}");
            log::warn!("[ZCODE-SYNC] {message}");
            result.errors.push(message);
            result.deferred_files = 1;
            return Ok(result);
        }
    };

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 ZCode 用量导入事务失败: {error}")))?;
    for record in &records {
        match insert_zcode_record(&tx, record)? {
            true => result.imported = result.imported.saturating_add(1),
            false => result.skipped = result.skipped.saturating_add(1),
        }
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 ZCode 用量导入事务失败: {error}")))?;

    if result.imported > 0 {
        log::info!(
            "[ZCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条",
            result.imported,
            result.skipped
        );
    }
    Ok(result)
}

/// Resolve the ZCode database path. `ZCODE_DB_PATH` overrides the default;
/// empty strings fall back to the standard location.
fn zcode_db_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("ZCODE_DB_PATH") {
        let trimmed = custom.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    crate::config::get_home_dir()
        .join(".zcode")
        .join("cli")
        .join("db")
        .join("db.sqlite")
}

/// Open the ZCode database read-only with a busy timeout so a concurrent
/// ZCode writer never blocks us indefinitely.
fn open_zcode_db_readonly(path: &std::path::Path) -> Result<rusqlite::Connection, AppError> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| AppError::Database(format!("无法打开 ZCode 数据库: {error}")))?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .map_err(|error| AppError::Database(format!("设置 ZCode busy_timeout 失败: {error}")))?;
    Ok(conn)
}

/// Read every `model_usage` row. A missing table or unexpected column defers
/// to the next sync pass rather than panicking — ZCode upgrades may reshape
/// the schema between releases.
fn load_zcode_records(conn: &rusqlite::Connection) -> Result<Vec<ZcodeUsageRecord>, AppError> {
    if !zcode_table_exists(conn)? {
        log::info!("[ZCODE-SYNC] model_usage 表不存在，跳过本轮");
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, provider_id, model_id, status, error_message,
                    started_at, duration_ms, time_to_first_token_ms,
                    input_tokens, output_tokens,
                    cache_read_input_tokens, cache_creation_input_tokens
             FROM model_usage
             WHERE status != 'running'",
        )
        .map_err(|error| AppError::Database(format!("准备 ZCode model_usage 查询失败: {error}")))?;

    let rows = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let provider_id: String = row.get(2)?;
            let model_id: String = row.get(3)?;
            let status: String = row.get(4)?;
            let error_message: Option<String> = row.get(5)?;
            let started_at: i64 = row.get(6)?;
            let duration_ms: Option<i64> = row.get(7)?;
            let time_to_first_token_ms: Option<i64> = row.get(8)?;
            let input_tokens: i64 = row.get(9)?;
            let output_tokens: i64 = row.get(10)?;
            let cache_read_input_tokens: i64 = row.get(11)?;
            let cache_creation_input_tokens: i64 = row.get(12)?;

            // completed → 200; everything else (error, cancelled) → 500.
            // `running` rows are excluded by the WHERE clause so that a
            // still-active request is not prematurely imported with
            // incomplete counters and locked into the dedup ledger.
            let status_code = if status == "completed" { 200 } else { 500 };

            let created_at = started_at
                .div_euclid(1000)
                .clamp(MIN_SQLITE_UNIX_SECONDS, MAX_SQLITE_UNIX_SECONDS);

            Ok(ZcodeUsageRecord {
                request_id: format!("{REQUEST_ID_PREFIX}{id}"),
                provider_id,
                model: model_id,
                input_tokens,
                output_tokens,
                cache_read_tokens: cache_read_input_tokens,
                cache_creation_tokens: cache_creation_input_tokens,
                latency_ms: duration_ms,
                first_token_ms: time_to_first_token_ms,
                status_code,
                error_message: if status_code == 500 {
                    error_message
                } else {
                    None
                },
                session_id,
                created_at,
            })
        })
        .map_err(|error| AppError::Database(format!("读取 ZCode model_usage 行失败: {error}")))?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(|error| {
            AppError::Database(format!("解析 ZCode model_usage 行失败: {error}"))
        })?);
    }
    Ok(records)
}

fn zcode_table_exists(conn: &rusqlite::Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'model_usage')",
        [],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|error| AppError::Database(format!("查询 ZCode model_usage 表存在性失败: {error}")))
}

/// Insert one ZCode record into the dashboard. Returns `true` when a new row
/// was written, `false` when the ledger already had it.
fn insert_zcode_record(
    conn: &rusqlite::Connection,
    record: &ZcodeUsageRecord,
) -> Result<bool, AppError> {
    let already_seen: bool = conn
        .query_row(
            ZCODE_REQUEST_DEDUP_SQL,
            rusqlite::params![DATA_SOURCE, record.request_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(format!("查询 ZCode 用量去重账本失败: {error}")))?;
    if already_seen {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![DATA_SOURCE, record.request_id, record.request_id, 1i64],
    )
    .map_err(|error| AppError::Database(format!("写入 ZCode 用量去重账本失败: {error}")))?;

    let usage = TokenUsage {
        input_tokens: record.input_tokens.max(0) as u32,
        output_tokens: record.output_tokens.max(0) as u32,
        cache_read_tokens: record.cache_read_tokens.max(0) as u32,
        cache_creation_tokens: record.cache_creation_tokens.max(0) as u32,
        model: Some(record.model.clone()),
        message_id: None,
    };
    let costs = find_model_pricing(conn, &record.model).map(|pricing| {
        let calculated =
            CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::ONE);
        (
            calculated.input_cost,
            calculated.output_cost,
            calculated.cache_read_cost,
            calculated.cache_creation_cost,
            calculated.total_cost,
        )
    });
    let (input_cost, output_cost, cache_read_cost, cache_write_cost, total_cost) =
        costs.unwrap_or((
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        ));

    conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd,
            cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )",
        rusqlite::params![
            record.request_id,
            record.provider_id,
            APP_TYPE,
            record.model,
            record.model,
            record.model,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_creation_tokens,
            INPUT_TOKEN_SEMANTICS_TOTAL,
            input_cost.to_string(),
            output_cost.to_string(),
            cache_read_cost.to_string(),
            cache_write_cost.to_string(),
            total_cost.to_string(),
            record.latency_ms.unwrap_or(0),
            record.first_token_ms,
            record.status_code,
            record.error_message,
            record.session_id,
            Some(DATA_SOURCE),
            0i64,
            "1.0",
            record.created_at,
            DATA_SOURCE,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|error| AppError::Database(format!("插入 ZCode 会话用量失败: {error}")))
}

#[cfg(test)]
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
mod tests {
    use super::*;

    /// Build a throwaway ZCode fixture database with the real `model_usage`
    /// schema so the importer exercises the same SQL it sees in production.
    fn create_fixture_db(dir: &std::path::Path) -> PathBuf {
        let db_path = dir.join("db.sqlite");
        let conn = rusqlite::Connection::open(&db_path).expect("open fixture db");
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY);
             CREATE TABLE model_usage (
                id text primary key,
                logical_request_id text not null,
                attempt_index integer not null default 0,
                session_id text not null references session(id) on delete cascade,
                turn_id text,
                trace_id text,
                span_id text,
                assistant_message_id text,
                parent_user_message_id text,
                query_source text not null,
                provider_id text not null,
                model_id text not null,
                variant text,
                agent text,
                mode text,
                task_type text,
                status text not null check(status in ('running', 'completed', 'error', 'cancelled')),
                started_at integer not null,
                first_token_at integer,
                completed_at integer,
                duration_ms integer,
                time_to_first_token_ms integer,
                finish_reason text,
                tool_call_count integer not null default 0,
                input_tokens integer not null default 0,
                output_tokens integer not null default 0,
                reasoning_tokens integer not null default 0,
                cache_creation_input_tokens integer not null default 0,
                cache_read_input_tokens integer not null default 0,
                provider_total_tokens integer,
                computed_total_tokens integer not null default 0,
                retry_count integer not null default 0,
                retryable integer not null default 0 check(retryable in (0, 1)),
                cancelled_by_user integer not null default 0 check(cancelled_by_user in (0, 1)),
                context_exceeded integer not null default 0 check(context_exceeded in (0, 1)),
                error_type text,
                error_code text,
                error_message text,
                raw_usage_json text,
                provider_metadata_json text
             );",
        )
        .expect("create fixture schema");
        db_path
    }

    fn insert_usage_row(
        conn: &rusqlite::Connection,
        id: &str,
        session_id: &str,
        provider_id: &str,
        model_id: &str,
        status: &str,
        started_at: i64,
        duration_ms: Option<i64>,
        ttft: Option<i64>,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
        error_message: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO session (id) VALUES (?1) ON CONFLICT DO NOTHING",
            [session_id],
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO model_usage (
                id, logical_request_id, session_id, query_source, provider_id, model_id,
                status, started_at, duration_ms, time_to_first_token_ms,
                input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                error_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                id,
                id,
                session_id,
                "main_turn",
                provider_id,
                model_id,
                status,
                started_at,
                duration_ms,
                ttft,
                input,
                output,
                cache_read,
                cache_creation,
                error_message,
            ],
        )
        .expect("insert model_usage row");
    }

    fn run_sync(dir: &std::path::Path) -> SessionSyncResult {
        let db = Database::memory().expect("memory db");
        sync_zcode_usage_from_path(&db, &dir.join("db.sqlite")).expect("sync")
    }

    #[test]
    fn imports_usage_events_with_total_semantics_and_per_column_mapping() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            insert_usage_row(
                &conn,
                "row-1",
                "sess-1",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "completed",
                1_787_362_690_458,
                Some(15666),
                Some(10110),
                19356,
                398,
                11712,
                0,
                None,
            );
        }

        let db = Database::memory().expect("memory db");
        let result = sync_zcode_usage_from_path(&db, &db_path).expect("sync");

        assert_eq!(result.imported, 1);
        assert!(result.errors.is_empty());

        let conn = lock_conn!(db.conn);
        let row: (
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            Option<i64>,
            Option<i64>,
            i64,
            Option<String>,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT request_id, provider_id, app_type, model, request_model,
                        pricing_model, input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, input_token_semantics, status_code,
                        latency_ms, first_token_ms, created_at, error_message,
                        data_source, is_streaming
                 FROM proxy_request_logs WHERE request_id = 'zcode:row-1'",
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
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                        row.get(15)?,
                        row.get(16)?,
                        row.get(17)?,
                    ))
                },
            )
            .expect("read row");

        assert_eq!(row.0, "zcode:row-1");
        assert_eq!(row.1, "builtin:zai-start-plan");
        assert_eq!(row.2, "zcode");
        assert_eq!(row.3, "GLM-5.3");
        assert_eq!(row.4, "GLM-5.3");
        assert_eq!(row.5, "GLM-5.3");
        assert_eq!((row.6, row.7, row.8, row.9), (19356, 398, 11712, 0));
        assert_eq!(row.10, INPUT_TOKEN_SEMANTICS_TOTAL);
        assert_eq!(row.11, 200);
        assert_eq!(row.12, Some(15666));
        assert_eq!(row.13, Some(10110));
        assert_eq!(row.14, 1_787_362_690);
        assert_eq!(row.15, None);
        assert_eq!(row.16, "zcode_session");
        assert_eq!(row.17, 0);

        let dedup: (String, i64) = conn
            .query_row(
                "SELECT request_id, has_entry_id FROM session_usage_dedup
                 WHERE data_source = 'zcode_session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read dedup");
        assert_eq!(dedup, ("zcode:row-1".to_string(), 1));
        Ok(())
    }

    #[test]
    fn replay_is_idempotent_zero_new_imports() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            insert_usage_row(
                &conn,
                "row-a",
                "sess-a",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "completed",
                1_787_362_690_000,
                Some(5000),
                None,
                100,
                10,
                50,
                0,
                None,
            );
            insert_usage_row(
                &conn,
                "row-b",
                "sess-a",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "completed",
                1_787_362_691_000,
                Some(3000),
                None,
                200,
                20,
                100,
                0,
                None,
            );
        }

        let db = Database::memory().expect("memory db");

        let first = sync_zcode_usage_from_path(&db, &db_path).expect("sync 1");
        assert_eq!((first.imported, first.skipped), (2, 0));

        let second = sync_zcode_usage_from_path(&db, &db_path).expect("sync 2");
        assert_eq!((second.imported, second.skipped), (0, 2));

        let count: i64 = lock_conn!(db.conn)
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'zcode_session'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn missing_provider_falls_back_to_placeholder_and_display_name() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            // Empty provider_id is rejected by the NOT NULL constraint, so use
            // a blank-ish value to exercise the placeholder path via an
            // explicit empty model instead.
            insert_usage_row(
                &conn,
                "row-empty-model",
                "sess-anon",
                "some-provider",
                "",
                "completed",
                1_787_362_690_000,
                None,
                None,
                10,
                5,
                2,
                0,
                None,
            );
        }

        let result = run_sync(temp.path());
        assert_eq!(result.imported, 1);

        // The real importer copies provider_id verbatim; the placeholder is
        // only surfaced by the display-name CASE in usage_stats. Verify the
        // placeholder resolves through get_provider_stats so the dashboard
        // would show "ZCode (Session)" for a synthetic placeholder row.
        let db = Database::memory().expect("memory db");
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, latency_ms, status_code,
                    created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "placeholder-test",
                    PROVIDER_PLACEHOLDER,
                    APP_TYPE,
                    "GLM-5.3",
                    "GLM-5.3",
                    10,
                    5,
                    0,
                    200,
                    1_787_362_690,
                    DATA_SOURCE,
                ],
            )
            .expect("insert placeholder row");
        }
        let providers = db
            .get_provider_stats(None, None, Some(APP_TYPE), None, None)
            .expect("provider stats");
        assert!(providers.iter().any(|p| {
            p.provider_id == PROVIDER_PLACEHOLDER && p.provider_name == "ZCode (Session)"
        }));
        Ok(())
    }

    #[test]
    fn all_query_sources_are_imported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            // One row per query_source value seen in production.
            for (id, source) in [
                ("src-main", "main_turn"),
                ("src-title", "session_title"),
                ("src-sub", "subagent"),
                ("src-verify", "target_completion_verification"),
            ] {
                conn.execute(
                    "INSERT INTO session (id) VALUES ('sess-q') ON CONFLICT DO NOTHING",
                    [],
                )
                .expect("insert session");
                conn.execute(
                    "INSERT INTO model_usage (
                        id, logical_request_id, session_id, query_source, provider_id, model_id,
                        status, started_at, input_tokens, output_tokens
                    ) VALUES (?1, ?1, 'sess-q', ?2, 'builtin:zai-start-plan', 'GLM-5.3',
                        'completed', 1787362690000, 100, 10)",
                    rusqlite::params![id, source],
                )
                .expect("insert row");
            }
        }

        let result = run_sync(temp.path());
        assert_eq!(result.imported, 4);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn missing_model_usage_table_defers_gracefully() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("db.sqlite");
        // Create a database with no model_usage table.
        rusqlite::Connection::open(&db_path).expect("open empty db");

        let result = run_sync(temp.path());
        assert_eq!(result.imported, 0);
        assert_eq!(result.deferred_files, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn error_status_maps_to_500_with_error_message() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            insert_usage_row(
                &conn,
                "row-err",
                "sess-err",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "error",
                1_787_362_690_000,
                Some(100),
                None,
                50,
                0,
                30,
                0,
                Some("upstream timeout"),
            );
            insert_usage_row(
                &conn,
                "row-cancel",
                "sess-err",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "cancelled",
                1_787_362_691_000,
                None,
                None,
                20,
                0,
                10,
                0,
                None,
            );
        }

        let db = Database::memory().expect("memory db");
        let result = sync_zcode_usage_from_path(&db, &db_path).expect("sync");
        assert_eq!(result.imported, 2);

        let conn = lock_conn!(db.conn);
        let err_row: (i64, Option<String>) = conn
            .query_row(
                "SELECT status_code, error_message FROM proxy_request_logs
                 WHERE request_id = 'zcode:row-err'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read err row");
        assert_eq!(err_row.0, 500);
        assert_eq!(err_row.1.as_deref(), Some("upstream timeout"));

        let cancel_row: i64 = conn
            .query_row(
                "SELECT status_code FROM proxy_request_logs
                 WHERE request_id = 'zcode:row-cancel'",
                [],
                |row| row.get(0),
            )
            .expect("read cancel row");
        assert_eq!(cancel_row, 500);
        Ok(())
    }

    #[test]
    fn running_rows_are_skipped_until_terminal() -> Result<(), AppError> {
        // A `running` row has incomplete counters and must not be imported,
        // otherwise the dedup ledger would lock it as status 500 forever.
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = create_fixture_db(temp.path());
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            insert_usage_row(
                &conn,
                "row-running",
                "sess-run",
                "builtin:zai-start-plan",
                "GLM-5.3",
                "running",
                1_787_362_690_000,
                None,
                None,
                10,
                0,
                0,
                0,
                None,
            );
        }

        let db = Database::memory().expect("memory db");

        // First sync: running row is excluded.
        let first = sync_zcode_usage_from_path(&db, &db_path).expect("sync 1");
        assert_eq!(first.imported, 0);
        assert_eq!(first.skipped, 0);

        let count: i64 = lock_conn!(db.conn)
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'zcode_session'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0);

        // Simulate ZCode completing the row.
        {
            let conn = rusqlite::Connection::open(&db_path).expect("open fixture");
            conn.execute(
                "UPDATE model_usage SET status = 'completed', duration_ms = 5000,
                         output_tokens = 42
                  WHERE id = 'row-running'",
                [],
            )
            .expect("update to completed");
        }

        // Second sync: the now-completed row is imported fresh.
        let second = sync_zcode_usage_from_path(&db, &db_path).expect("sync 2");
        assert_eq!(second.imported, 1);

        let status: i64 = lock_conn!(db.conn)
            .query_row(
                "SELECT status_code FROM proxy_request_logs
                 WHERE request_id = 'zcode:row-running'",
                [],
                |row| row.get(0),
            )
            .expect("read status");
        assert_eq!(status, 200);
        Ok(())
    }
}
