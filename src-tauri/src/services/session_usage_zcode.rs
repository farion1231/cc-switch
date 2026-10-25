//! Import ZCode's committed token-usage records from its local SQLite database
//! without modifying it.
//!
//! ZCode (0.15+) writes one `model_usage` row per model request into
//! `~/.zcode/cli/db/db.sqlite`. Its `input_tokens` column is total-inclusive
//! (cache reads counted inside, same as Codex/Gemini), so rows are stored with
//! `INPUT_TOKEN_SEMANTICS_TOTAL` and `zcode` belongs in
//! `CACHE_INCLUSIVE_APP_TYPES`.
use crate::config::get_home_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::session_usage::SessionSyncResult;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::params;
use rust_decimal::Decimal;
use std::path::PathBuf;

/// 回看窗口：晚于水位线开始、晚于上次同步才完成的请求（长回合可运行数十分钟），
/// 其 `started_at` 会落在水位线之前，靠窗口捞回；`request_id` 主键保证不重复导入。
const LOOKBACK_MS: i64 = 24 * 60 * 60 * 1000;

/// ZCode CLI 的本地用量数据库路径。
/// 优先级: `ZCODE_DB` 环境变量 > `~/.zcode/cli/db/db.sqlite`
fn zcode_db_path() -> PathBuf {
    // 支持环境变量覆盖（忽略空字符串）
    if let Ok(custom_path) = std::env::var("ZCODE_DB") {
        if !custom_path.is_empty() {
            return PathBuf::from(custom_path);
        }
    }
    get_home_dir()
        .join(".zcode")
        .join("cli")
        .join("db")
        .join("db.sqlite")
}

pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let source_path = zcode_db_path();
    if !source_path.exists() {
        return Ok(SessionSyncResult::default());
    }
    // 只读打开：这是正在运行的 ZCode 的主数据库，绝不写入
    let source = rusqlite::Connection::open_with_flags(
        &source_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| AppError::Database(format!("无法打开 ZCode 数据库: {e}")))?;
    let key = format!("zcode:{}", source_path.display());
    sync_from_database(db, &source, &key)
}

fn sync_from_database(
    db: &Database,
    source: &rusqlite::Connection,
    key: &str,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let cursor = tx.query_row(
        "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
        [&key],
        |row| row.get::<_, i64>(0),
    )?;
    let mut query = source.prepare(
        "SELECT id, session_id, model_id, started_at, COALESCE(duration_ms, 0),
                input_tokens, output_tokens, reasoning_tokens,
                cache_read_input_tokens, cache_creation_input_tokens
         FROM model_usage
         WHERE status = 'completed' AND started_at >= ?1
         ORDER BY started_at",
    )?;
    let mut rows = query.query([cursor.saturating_sub(LOOKBACK_MS)])?;
    let mut watermark = cursor;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let session_id: String = row.get(1)?;
        let model: String = row.get(2)?;
        let started_at: i64 = row.get(3)?;
        let duration_ms: i64 = row.get(4)?;
        let input_tokens: u32 = row.get(5)?;
        let reasoning_tokens: u32 = row.get(7)?;
        let usage = TokenUsage {
            // ZCode 的 input_tokens 含缓存读取（computed_total = input + output）
            input_tokens,
            // 与 MCode 同口径：reasoning 并入 output
            output_tokens: row.get::<_, u32>(6)?.saturating_add(reasoning_tokens),
            cache_read_tokens: row.get(8)?,
            cache_creation_tokens: row.get(9)?,
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        };
        let cost = find_model_pricing(&tx, &model)
            .map(|pricing| {
                CostCalculator::calculate_for_app("zcode", &usage, &pricing, Decimal::ONE)
                    .total_cost
                    .to_string()
            })
            .unwrap_or_else(|| "0".into());
        let request_id = format!("zcode:{id}");
        let changed = tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, session_id, provider_type,
                is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
             ) VALUES (?1, '_zcode_session', 'zcode', ?2, ?2, ?3, ?4, ?5, ?6,
                       '0', '0', '0', '0', ?7, ?8, 200, ?9, 'zcode_session', 1, '1', ?10, 'zcode_session', ?11)",
            params![
                request_id,
                model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                cost,
                duration_ms,
                session_id,
                started_at / 1000,
                INPUT_TOKEN_SEMANTICS_TOTAL
            ],
        )?;
        result.imported += changed as u32;
        result.skipped += u32::from(changed == 0);
        watermark = watermark.max(started_at);
    }
    // 水位线与导入行同事务提交；已被归档清理的明细行绝不重复导入
    tx.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, ?2, unixepoch()) ON CONFLICT(file_path) DO UPDATE SET
         last_line_offset = excluded.last_line_offset, last_synced_at = excluded.last_synced_at",
        params![key, watermark],
    )?;
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_source() -> rusqlite::Connection {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE model_usage (
                    id text primary key,
                    session_id text not null,
                    model_id text not null,
                    status text not null,
                    started_at integer not null,
                    duration_ms integer,
                    input_tokens integer not null default 0,
                    output_tokens integer not null default 0,
                    reasoning_tokens integer not null default 0,
                    cache_read_input_tokens integer not null default 0,
                    cache_creation_input_tokens integer not null default 0);
                INSERT INTO model_usage VALUES
                    ('u1','s1','glm-5','completed',1000000,500, 26486,303,0,24896,0),
                    ('u2','s1','glm-5','completed',1001000,300, 1000,50,20,0,0),
                    ('u3','s1','glm-5','error',1002000,10,900,9,0,0,0);",
            )
            .unwrap();
        source
    }

    #[test]
    fn zcode_usage_imports_completed_rows_with_total_semantics() {
        let source = setup_source();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            2
        );
        let conn = db.conn.lock().unwrap();
        let (input, output, cache_read, semantics, latency, created_at): (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens,
                        input_token_semantics, latency_ms, created_at
                 FROM proxy_request_logs WHERE request_id = 'zcode:u1'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        // input 原样保存（含缓存读取），output 合并 reasoning，语义为 total
        assert_eq!(
            (input, output, cache_read, semantics),
            (26486, 303, 24896, INPUT_TOKEN_SEMANTICS_TOTAL)
        );
        assert_eq!((latency, created_at), (500, 1000));
        // error 行（即使有 token）不导入
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'zcode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn zcode_usage_is_idempotent_and_catches_late_completing_rows() {
        let source = setup_source();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            2
        );
        // 水位线已到 1001000；无新行时再同步导入 0 条
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            0
        );
        // 晚完成的请求：started_at 早于水位线（在回看窗口内），仍要被捞回
        source
            .execute(
                "INSERT INTO model_usage VALUES
                    ('u4','s1','glm-5','completed',999500,600, 2000,100,0,1500,0)",
                [],
            )
            .unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            1
        );
        // 同一行不会重复导入
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            0
        );
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'zcode'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn zcode_usage_resolves_cost_from_model_pricing() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE model_usage (
                    id text primary key, session_id text not null, model_id text not null,
                    status text not null, started_at integer not null, duration_ms integer,
                    input_tokens integer not null default 0, output_tokens integer not null default 0,
                    reasoning_tokens integer not null default 0,
                    cache_read_input_tokens integer not null default 0,
                    cache_creation_input_tokens integer not null default 0);
                INSERT INTO model_usage VALUES
                    ('u1','s1','test-model','completed',1000000,100, 200,100,0,0,0);",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
                                            output_cost_per_million,
                                            cache_read_cost_per_million, cache_creation_cost_per_million)
                 VALUES ('test-model', 'Test Model', '1', '2', '0.1', '1')",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            sync_from_database(&db, &source, "zcode-pricing")
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        // input 含缓存读取时 fresh input = 200；cost = 200×1/1M + 100×2/1M = 0.0004
        let cost: String = conn
            .query_row(
                "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'zcode:u1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cost, "0.0004");
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;

    #[test]
    #[ignore = "requires the real ZCode CLI database on this machine"]
    fn zcode_reads_real_database_and_stays_idempotent() -> Result<(), AppError> {
        let path = zcode_db_path();
        if !path.exists() {
            eprintln!("ZCode 数据库不存在，跳过: {}", path.display());
            return Ok(());
        }
        let db = Database::memory()?;
        let first = sync_zcode_usage(&db)?;
        assert!(
            first.imported > 0,
            "真实库应有已完成请求可导入，实际 {}",
            first.imported
        );
        assert_eq!(sync_zcode_usage(&db)?.imported, 0, "第二次同步必须幂等");
        let conn = lock_conn!(db.conn);
        let (rows, input, output, cache_read): (i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),              COALESCE(SUM(cache_read_tokens), 0) FROM proxy_request_logs WHERE app_type = 'zcode'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        eprintln!("真实数据: {rows} 行, input={input}, output={output}, cache_read={cache_read}");
        Ok(())
    }
}
