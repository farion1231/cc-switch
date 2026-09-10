//! Cherry Studio usage import.
//!
//! Reads Cherry Studio's SQLite usage ledger without modifying it, then copies
//! language-model invocation records into CC Switch's unified usage table.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::find_model_pricing;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const APP_TYPE: &str = "cherry-studio";
const DATA_SOURCE: &str = "cherry_studio";

#[derive(Debug)]
struct CherryUsageRecord {
    request_id: String,
    provider_id: String,
    provider_name: String,
    model_id: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    first_token_ms: Option<i64>,
    duration_ms: i64,
    created_at: i64,
}

/// Returns Cherry Studio's usage database path.
///
/// `CHERRY_STUDIO_DB` supports portable and relocated installations. Otherwise
/// the platform-standard packaged Electron user-data location is used.
pub fn get_cherry_studio_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CHERRY_STUDIO_DB").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    #[cfg(target_os = "macos")]
    let user_data = crate::config::get_home_dir()
        .join("Library")
        .join("Application Support")
        .join("CherryStudio");

    #[cfg(target_os = "windows")]
    let user_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::config::get_home_dir()
                .join("AppData")
                .join("Roaming")
        })
        .join("CherryStudio");

    #[cfg(target_os = "linux")]
    let user_data = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::get_home_dir().join(".config"))
        .join("CherryStudio");

    user_data.join("Data").join("cherrystudio.sqlite")
}

/// Imports newly committed Cherry Studio usage records.
pub fn sync_cherry_studio_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_cherry_studio_db_path();
    sync_cherry_studio_usage_from_path(db, &db_path)
}

fn sync_cherry_studio_usage_from_path(
    db: &Database,
    db_path: &Path,
) -> Result<SessionSyncResult, AppError> {
    if !db_path.exists() {
        return Ok(SessionSyncResult::default());
    }

    let db_path_str = db_path.to_string_lossy().to_string();
    let metadata = fs::metadata(db_path).map_err(|error| AppError::io(db_path, error))?;
    let mut modified = metadata_modified_nanos(&metadata);
    let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
    if let Ok(wal_metadata) = fs::metadata(wal_path) {
        modified = modified.max(metadata_modified_nanos(&wal_metadata));
    }

    let cursors = load_sync_cursors(db)?;
    let cursor = cursors.get(&db_path_str);
    let last_modified = cursor.map_or(0, |c| c.last_modified);
    if modified <= last_modified {
        return Ok(SessionSyncResult {
            files_scanned: 1,
            ..SessionSyncResult::default()
        });
    }

    let cherry =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| {
                AppError::Database(format!("无法只读打开 Cherry Studio 数据库: {error}"))
            })?;

    if !has_usage_table(&cherry)? {
        return Ok(SessionSyncResult {
            files_scanned: 1,
            errors: vec!["Cherry Studio 数据库中没有 ai_usage_record 表".to_string()],
            ..SessionSyncResult::default()
        });
    }

    let source_max_rowid = max_usage_rowid(&cherry)?;
    // Cherry records live in a normal SQLite rowid table. Reuse the generic
    // line-offset column as the durable source cursor so steady-state polls
    // only read newly appended rows. A replaced/truncated database can have a
    // lower maximum rowid; restart from zero and let the durable ledger dedup
    // the one recovery scan.
    let saved_rowid = cursor.map_or(0, |c| c.last_line_offset.max(0));
    let after_rowid = if source_max_rowid < saved_rowid {
        0
    } else {
        saved_rowid
    };
    let records = query_usage_records(&cherry, after_rowid, source_max_rowid)?;
    import_usage_records(db, &db_path_str, modified, source_max_rowid, &records)
}

fn has_usage_table(conn: &rusqlite::Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ai_usage_record')",
        [],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("检查 Cherry Studio 用量表失败: {error}")))
}

fn nonnegative_u32(value: Option<i64>) -> u32 {
    value.unwrap_or(0).clamp(0, u32::MAX as i64) as u32
}

pub(crate) fn backfill_dedup_ledger_on_conn(conn: &rusqlite::Connection) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         SELECT ?1, request_id, request_id, 1
         FROM proxy_request_logs
         WHERE app_type = ?2 AND data_source = ?1",
        rusqlite::params![DATA_SOURCE, APP_TYPE],
    )?;
    Ok(())
}

fn max_usage_rowid(conn: &rusqlite::Connection) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COALESCE(MAX(rowid), 0) FROM ai_usage_record",
        [],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("读取 Cherry Studio 用量游标失败: {error}")))
}

fn has_usage_column(conn: &rusqlite::Connection, column: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('ai_usage_record') WHERE name = ?1
         )",
        [column],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("检查 Cherry Studio 用量字段失败: {error}")))
}

fn query_usage_records(
    conn: &rusqlite::Connection,
    after_rowid: i64,
    through_rowid: i64,
) -> Result<Vec<CherryUsageRecord>, AppError> {
    // Migrated aggregate rows represent multiple requests and cannot be
    // losslessly expressed in the detail table, whose request count is
    // COUNT(*). Older Cherry schemas do not have record_kind, so only add the
    // predicate when the column exists.
    let record_kind_filter = if has_usage_column(conn, "record_kind")? {
        "AND COALESCE(record_kind, '') <> 'legacy-aggregate'"
    } else {
        ""
    };
    let query = format!(
        "SELECT request_id, provider_id, provider_name, model_id, model_name,
                input_tokens, output_tokens, no_cache_tokens,
                cache_read_tokens, cache_write_tokens,
                time_first_token_ms, time_completion_ms, created_at
         FROM ai_usage_record
         WHERE rowid > ?1 AND rowid <= ?2 AND modality = 'language'
         {record_kind_filter}
         ORDER BY rowid"
    );
    let mut stmt = conn
        .prepare(&query)
        .map_err(|error| AppError::Database(format!("准备 Cherry Studio 用量查询失败: {error}")))?;

    let rows = stmt
        .query_map(rusqlite::params![after_rowid, through_rowid], |row| {
            let all_input = nonnegative_u32(row.get(5)?);
            let cache_read = nonnegative_u32(row.get(8)?);
            let cache_write = nonnegative_u32(row.get(9)?);
            let no_cache: Option<i64> = row.get(7)?;
            let input_tokens = no_cache
                .map(|value| nonnegative_u32(Some(value)))
                .unwrap_or_else(|| {
                    all_input.saturating_sub(cache_read.saturating_add(cache_write))
                });
            let first_token_ms: Option<i64> = row.get(10)?;
            let completion_ms: Option<i64> = row.get(11)?;
            let provider_id: Option<String> = row.get(1)?;
            let provider_name: Option<String> = row.get(2)?;
            let model_id: Option<String> = row.get(3)?;
            let model_name: Option<String> = row.get(4)?;

            Ok(CherryUsageRecord {
                request_id: row.get(0)?,
                provider_id: provider_id.unwrap_or_else(|| "unknown".to_string()),
                provider_name: provider_name.unwrap_or_else(|| "Cherry Studio".to_string()),
                model_id: model_id
                    .or(model_name)
                    .unwrap_or_else(|| "unknown".to_string()),
                input_tokens,
                output_tokens: nonnegative_u32(row.get(6)?),
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                first_token_ms: first_token_ms.filter(|value| *value >= 0),
                duration_ms: completion_ms.unwrap_or(0).max(0),
                created_at: row.get::<_, i64>(12)? / 1000,
            })
        })
        .map_err(|error| AppError::Database(format!("查询 Cherry Studio 用量失败: {error}")))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(format!("读取 Cherry Studio 用量行失败: {error}")))
}

fn import_usage_records(
    db: &Database,
    file_path: &str,
    modified: i64,
    source_rowid: i64,
    records: &[CherryUsageRecord],
) -> Result<SessionSyncResult, AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..SessionSyncResult::default()
    };
    let mut pricing_cache: HashMap<String, Option<ModelPricing>> = HashMap::new();
    for record in records {
        if insert_usage_record_on_conn(&tx, record, &mut pricing_cache)? {
            result.imported = result.imported.saturating_add(1);
        } else {
            result.skipped = result.skipped.saturating_add(1);
        }
    }
    update_sync_state_on_conn(&tx, file_path, modified, source_rowid)?;
    tx.commit()?;
    Ok(result)
}

fn insert_usage_record_on_conn(
    conn: &rusqlite::Connection,
    record: &CherryUsageRecord,
    pricing_cache: &mut HashMap<String, Option<ModelPricing>>,
) -> Result<bool, AppError> {
    let request_id = format!("cherry_studio:{}", record.request_id);
    let ledger_inserted = conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?2, 1)",
        rusqlite::params![DATA_SOURCE, request_id],
    )?;
    if ledger_inserted == 0 {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_write_tokens,
        model: Some(record.model_id.clone()),
        message_id: None,
    };
    let pricing = pricing_cache
        .entry(record.model_id.clone())
        .or_insert_with(|| find_model_pricing(conn, &record.model_id));
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, pricing, Decimal::ONE);
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
    };

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd,
            cache_creation_cost_usd, total_cost_usd, latency_ms, first_token_ms,
            duration_ms, status_code, error_message, session_id, provider_type,
            is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                  ?14, ?15, ?16, ?17, ?18, ?19, 200, NULL, NULL, ?20, 1, '1.0',
                  ?21, ?22)",
        rusqlite::params![
            request_id,
            record.provider_name,
            APP_TYPE,
            record.model_id,
            record.model_id,
            record.model_id,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_write_tokens,
            INPUT_TOKEN_SEMANTICS_FRESH,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            record.duration_ms,
            record.first_token_ms,
            record.duration_ms,
            record.provider_id,
            record.created_at,
            DATA_SOURCE,
        ],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cherry_database_is_a_noop() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().unwrap();

        let result = sync_cherry_studio_usage_from_path(&db, &temp.path().join("missing.sqlite"))?;

        assert_eq!(result.imported, 0);
        assert_eq!(result.files_scanned, 0);
        assert!(result.errors.is_empty());
        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    fn backfill_only_captures_legacy_cherry_details() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch(
                "INSERT INTO proxy_request_logs
                    (request_id, provider_id, app_type, model, input_tokens, output_tokens,
                     latency_ms, status_code, created_at, data_source)
                 VALUES
                    ('cherry_studio:old', 'OpenAI', 'cherry-studio', 'gpt-test',
                     10, 2, 5, 200, 1, 'cherry_studio'),
                    ('existing-claude', 'anthropic', 'claude', 'claude-test',
                     7, 3, 5, 200, 1, 'proxy');",
            )?;
        }

        let conn = lock_conn!(db.conn);
        backfill_dedup_ledger_on_conn(&conn)?;
        backfill_dedup_ledger_on_conn(&conn)?;
        let rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_usage_dedup
             WHERE data_source = 'cherry_studio'
               AND request_id = 'cherry_studio:old'
               AND semantic_id = 'cherry_studio:old'",
            [],
            |row| row.get(0),
        )?;
        let unrelated_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_usage_dedup WHERE request_id = 'existing-claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rows, 1);
        assert_eq!(unrelated_rows, 0);
        Ok(())
    }

    #[test]
    fn regular_sync_does_not_repeat_legacy_backfill() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs
                    (request_id, provider_id, app_type, model, input_tokens, output_tokens,
                     latency_ms, status_code, created_at, data_source)
                 VALUES ('cherry_studio:legacy', 'OpenAI', 'cherry-studio', 'gpt-test',
                         10, 2, 5, 200, 1, 'cherry_studio')",
                [],
            )?;
        }

        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );",
        )?;
        drop(cherry);

        let result = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(result.imported, 0);

        let conn = lock_conn!(db.conn);
        let dedup_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_usage_dedup
             WHERE data_source = 'cherry_studio'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(dedup_count, 0);
        Ok(())
    }

    #[test]
    fn import_is_scoped_to_cherry_usage_and_preserves_other_apps() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES ('existing-claude', 'anthropic', 'claude', 'claude-test',
                          'claude-test', 7, 3, 0, 0, '0.01', 10, 200, 1, 'proxy')",
                [],
            )?;
        }

        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'language', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
                 'language', 100, 20, 60, 30, 10, 0.5, 'USD', 12, 34, 2000),
                ('2', 'embedding', 'openai', 'OpenAI', 'embed-test', NULL,
                 'embedding', 50, 0, 50, 0, 0, 0.1, 'USD', NULL, 10, 3000);",
        )?;
        drop(cherry);

        let first = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(first.imported, 1);
        assert!(first.errors.is_empty());

        let second = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(second.imported, 0);
        assert!(second.errors.is_empty());

        let conn = lock_conn!(db.conn);
        let claude_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id = 'existing-claude' AND app_type = 'claude'
               AND input_tokens = 7 AND output_tokens = 3",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(claude_count, 1);

        let cherry_row: (i64, String, String, String, i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), data_source, provider_id, provider_type, input_tokens,
                    output_tokens, cache_read_tokens, cache_creation_tokens
             FROM proxy_request_logs WHERE app_type = 'cherry-studio'",
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
                ))
            },
        )?;
        assert_eq!(
            cherry_row,
            (
                1,
                "cherry_studio".into(),
                "OpenAI".into(),
                "openai".into(),
                60,
                20,
                30,
                10
            )
        );
        let pricing: (String, String, String, String, i64) = conn.query_row(
            "SELECT model, request_model, pricing_model, total_cost_usd,
                    input_token_semantics
             FROM proxy_request_logs WHERE app_type = 'cherry-studio'",
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
        )?;
        assert_eq!(
            pricing,
            (
                "gpt-test".into(),
                "gpt-test".into(),
                "gpt-test".into(),
                "0".into(),
                INPUT_TOKEN_SEMANTICS_FRESH
            )
        );
        let cherry_dedup_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'cherry_studio'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cherry_dedup_count, 1);
        Ok(())
    }

    #[test]
    fn durable_dedup_prevents_reimport_after_detail_rollup() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'old', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
                 'language', 100, 20, 60, 30, 10, 0.5, 'USD', 12, 34, 2000);",
        )?;
        drop(cherry);

        let first = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(first.imported, 1);
        assert_eq!(db.rollup_and_prune(30)?, 1);

        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute(
            "INSERT INTO ai_usage_record VALUES
             ('2', 'new', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
              'language', 80, 10, 50, 20, 10, 0.25, 'USD', 8, 21, 3000)",
            [],
        )?;
        drop(cherry);
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE session_log_sync SET last_modified = 0 WHERE file_path = ?1",
                [cherry_path.to_string_lossy().as_ref()],
            )?;
        }

        let second = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(second.imported, 1);
        assert_eq!(second.skipped, 0);

        let conn = lock_conn!(db.conn);
        let detail_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE app_type = 'cherry-studio' AND data_source = 'cherry_studio'",
            [],
            |row| row.get(0),
        )?;
        let rollup_count: i64 = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0) FROM usage_daily_rollups
             WHERE app_type = 'cherry-studio'",
            [],
            |row| row.get(0),
        )?;
        let dedup_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'cherry_studio'",
            [],
            |row| row.get(0),
        )?;
        let source_cursor: i64 = conn.query_row(
            "SELECT last_line_offset FROM session_log_sync WHERE file_path = ?1",
            [cherry_path.to_string_lossy().as_ref()],
            |row| row.get(0),
        )?;
        assert_eq!(detail_count, 1);
        assert_eq!(rollup_count, 1);
        assert_eq!(dedup_count, 2);
        assert_eq!(source_cursor, 2);
        Ok(())
    }

    #[test]
    fn batch_import_rolls_back_rows_and_cursor_together() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER reject_bad_cherry_record
                 BEFORE INSERT ON proxy_request_logs
                 WHEN NEW.request_id = 'cherry_studio:bad'
                 BEGIN
                     SELECT RAISE(FAIL, 'rejected test row');
                 END;",
            )?;
        }

        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'good', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
                 'language', 10, 2, 8, 0, 0, 0.1, 'USD', 1, 2, 2000),
                ('2', 'bad', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
                 'language', 10, 2, 8, 0, 0, 0.1, 'USD', 1, 2, 3000);",
        )?;
        drop(cherry);

        assert!(sync_cherry_studio_usage_from_path(&db, &cherry_path).is_err());

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs
                 WHERE data_source = 'cherry_studio'),
                (SELECT COUNT(*) FROM session_usage_dedup
                 WHERE data_source = 'cherry_studio'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1)",
            [cherry_path.to_string_lossy().as_ref()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (0, 0, 0));
        Ok(())
    }

    #[test]
    fn startup_rollup_backfills_recent_legacy_dedup_without_pruning() -> Result<(), AppError> {
        let db = Database::memory()?;
        let recent_ts = chrono::Utc::now().timestamp();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                 ) VALUES ('cherry_studio:recent-legacy', 'OpenAI', 'cherry-studio',
                           'gpt-test', 'gpt-test', 60, 20, 30, 10, '0.5', 34, 200,
                           ?1, 'cherry_studio')",
                [recent_ts],
            )?;
        }

        // A source rowid reset must be able to deduplicate this retained row even
        // though no details are old enough for the rollup pass to prune.
        assert_eq!(db.rollup_and_prune(30)?, 0);

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs
                 WHERE request_id = 'cherry_studio:recent-legacy'),
                (SELECT COUNT(*) FROM session_usage_dedup
                 WHERE data_source = 'cherry_studio'
                   AND request_id = 'cherry_studio:recent-legacy')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(counts, (1, 1));
        Ok(())
    }

    #[test]
    fn startup_rollup_backfills_legacy_dedup_before_cherry_sync() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                 ) VALUES ('cherry_studio:legacy', 'OpenAI', 'cherry-studio', 'gpt-test',
                           'gpt-test', 60, 20, 30, 10, '0.5', 34, 200, 1,
                           'cherry_studio')",
                [],
            )?;
        }

        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'legacy', 'openai', 'OpenAI', 'gpt-test', 'GPT Test',
                 'language', 100, 20, 60, 30, 10, 0.5, 'USD', 12, 34, 2000);",
        )?;
        drop(cherry);

        assert_eq!(db.rollup_and_prune(30)?, 1);
        let sync = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(sync.imported, 0);
        assert_eq!(sync.skipped, 1);

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs
                 WHERE request_id = 'cherry_studio:legacy'),
                (SELECT COALESCE(SUM(request_count), 0) FROM usage_daily_rollups
                 WHERE app_type = 'cherry-studio'),
                (SELECT COUNT(*) FROM session_usage_dedup
                 WHERE data_source = 'cherry_studio'
                   AND request_id = 'cherry_studio:legacy')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (0, 1, 1));
        Ok(())
    }

    #[test]
    fn sync_prices_all_cherry_currencies_from_cc_switch_tokens() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (
                    model_id, display_name, input_cost_per_million,
                    output_cost_per_million, cache_read_cost_per_million,
                    cache_creation_cost_per_million
                 ) VALUES ('m1', 'Model One', '1', '2', '0.5', '3')",
                [],
            )?;
        }

        let temp = tempfile::tempdir().unwrap();
        let cherry_path = temp.path().join("cherrystudio.sqlite");
        let cherry = rusqlite::Connection::open(&cherry_path)?;
        cherry.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'usd', 'p1', 'Provider', 'm1', 'Display Name', 'language',
                 100, 20, 60, 30, 10, 999.0, 'USD', 12, 34, 2000),
                ('2', 'cny', 'p1', 'Provider', 'm1', 'Display Name', 'language',
                 100, 20, 60, 30, 10, 888.0, 'CNY', 12, 34, 3000);",
        )?;
        drop(cherry);

        let result = sync_cherry_studio_usage_from_path(&db, &cherry_path)?;
        assert_eq!(result.imported, 2);

        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT request_id, CAST(total_cost_usd AS REAL), pricing_model
             FROM proxy_request_logs
             WHERE data_source = 'cherry_studio'
             ORDER BY request_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(rows.len(), 2);
        for (_, total_cost, pricing_model) in rows {
            assert!((total_cost - 0.000145).abs() < 1e-12);
            assert_eq!(pricing_model, "m1");
        }
        Ok(())
    }

    #[test]
    fn query_normalizes_cherry_input_and_preserves_raw_model_id() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'usd', 'p1', 'Provider', 'm1', 'Model', 'language', 100, 20, NULL, 30, 10, 0.5, 'USD', 12, 34, 2000),
                ('2', 'cny', 'p1', 'Provider', 'm1', NULL, 'language', 100, 20, 55, 30, 10, 3.0, 'CNY', NULL, NULL, 3000);",
        )
        .unwrap();

        let max_rowid = max_usage_rowid(&conn).unwrap();
        let records = query_usage_records(&conn, 0, max_rowid).unwrap();
        assert_eq!(max_rowid, 2);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].input_tokens, 60);
        assert_eq!(records[0].model_id, "m1");
        assert_eq!(records[1].input_tokens, 55);
        assert_eq!(records[1].model_id, "m1");
    }

    #[test]
    fn query_excludes_migrated_legacy_aggregates() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE ai_usage_record (
                id TEXT, request_id TEXT, provider_id TEXT, provider_name TEXT,
                model_id TEXT, model_name TEXT, modality TEXT, input_tokens INTEGER,
                output_tokens INTEGER, no_cache_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, cost REAL, cost_currency TEXT,
                time_first_token_ms INTEGER, time_completion_ms INTEGER, created_at INTEGER,
                record_kind TEXT, request_count INTEGER
             );
             INSERT INTO ai_usage_record VALUES
                ('1', 'aggregate', 'p1', 'Provider', 'm1', 'Model', 'language',
                 1000, 200, 800, 100, 100, 5.0, 'USD', NULL, 300, 2000,
                 'legacy-aggregate', 10),
                ('2', 'detail', 'p1', 'Provider', 'm1', 'Model', 'language',
                 100, 20, 80, 10, 10, 0.5, 'USD', 12, 34, 3000,
                 'detail', 1);",
        )
        .unwrap();

        let max_rowid = max_usage_rowid(&conn).unwrap();
        let records = query_usage_records(&conn, 0, max_rowid).unwrap();

        assert_eq!(max_rowid, 2);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].request_id, "detail");
    }
}
