//! Import ZCode's per-request model usage from its local SQLite database.
//!
//! ZCode 在 `~/.zcode/cli/db/db.sqlite` 的 `model_usage` 表里按「请求尝试」记录
//! token 用量（一次逻辑请求的每次重试各一行）。这里只读导入，绝不写它的库。
//!
//! 与 MCode 同为 SQLite 数据源：游标存源库的 rowid 水位。差异是 ZCode 的行在
//! 请求开始时就以 `status = 'running'` 落库、完成后再更新，所以只导入终态行，
//! 且水位不允许越过仍在 running 的最小 rowid——否则该行完成后永远扫不到。

use crate::config::get_home_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::{session_usage::SessionSyncResult, usage_stats::find_model_pricing};
use rusqlite::{params, Connection, OpenFlags};
use rust_decimal::Decimal;
use std::path::{Path, PathBuf};

fn database_path() -> PathBuf {
    std::env::var_os("ZCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".zcode"))
        .join("cli/db/db.sqlite")
}

/// 只读打开 ZCode 的活库。它默认跑在 WAL 模式，`-shm`/`-wal` 齐全时只读打开
/// 与写入方互不阻塞；ZCode 退出后若残留 `-wal` 而缺 `-shm`，只读打开会失败，
/// 退回 `immutable=1` 尽力读（至多少读最近未 checkpoint 的提交，不会重复计费）。
fn open_database(path: &Path) -> rusqlite::Result<Connection> {
    match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => Ok(conn),
        Err(error) => {
            log::debug!("zcode db read-only open failed ({error}), retrying immutable");
            let uri = format!("file:{}?immutable=1", path.display());
            Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            )
        }
    }
}

pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let path = database_path();
    if !path.exists() {
        return Ok(SessionSyncResult::default());
    }
    let source = open_database(&path)?;
    let key = format!("zcode:{}", path.display());
    sync_from_database(db, &source, &key)
}

fn sync_from_database(
    db: &Database,
    source: &Connection,
    key: &str,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut cursor = tx.query_row(
        "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
        [&key],
        |row| row.get::<_, i64>(0),
    )?;
    let mut query = source.prepare(
        "SELECT rowid, id, session_id, model_id, status, started_at, duration_ms,
                time_to_first_token_ms, error_message, input_tokens, output_tokens,
                reasoning_tokens, cache_read_input_tokens, cache_creation_input_tokens
         FROM model_usage
         WHERE rowid > ?1 AND status != 'running'
         ORDER BY rowid",
    )?;
    let mut rows = query.query([cursor])?;
    result.files_scanned = 1;
    while let Some(row) = rows.next()? {
        let rowid: i64 = row.get(0)?;
        let id: String = row.get(1)?;
        let session_id: String = row.get(2)?;
        let model: String = row.get(3)?;
        let status: String = row.get(4)?;
        let started_at: i64 = row.get(5)?;
        let duration_ms: Option<i64> = row.get(6)?;
        let first_token_ms: Option<i64> = row.get(7)?;
        let error_message: Option<String> = row.get(8)?;
        let usage = TokenUsage {
            input_tokens: row.get(9)?,
            output_tokens: row.get::<_, u32>(10)?.saturating_add(row.get(11)?),
            cache_read_tokens: row.get(12)?,
            cache_creation_tokens: row.get(13)?,
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        };
        let (status_code, error_message) = match status.as_str() {
            "completed" => (200, None),
            "cancelled" => (499, None),
            // ZCode 只在 error_message 里带原因文本，截断避免整段堆栈进库
            _ => (
                500,
                error_message.map(|m| m.chars().take(200).collect::<String>()),
            ),
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
                total_cost_usd, latency_ms, first_token_ms, duration_ms, status_code,
                error_message, session_id, provider_type, is_streaming, cost_multiplier,
                created_at, data_source, input_token_semantics
             ) VALUES (?1, '_zcode_session', 'zcode', ?2, ?2, ?3, ?4, ?5, ?6,
                       '0', '0', '0', '0', ?7, ?8, ?9, ?8, ?10, ?11, ?12,
                       'zcode_session', 1, '1', ?13, 'zcode_session', ?14)",
            params![
                request_id,
                model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                cost,
                duration_ms.unwrap_or(0),
                first_token_ms,
                status_code,
                error_message,
                session_id,
                started_at / 1000,
                INPUT_TOKEN_SEMANTICS_TOTAL
            ],
        )?;
        result.imported += changed as u32;
        result.skipped += u32::from(changed == 0);
        cursor = rowid;
    }
    // 水位不得越过仍处 running 的最小 rowid：那行完成后还得能被下一轮扫到。
    let min_running: Option<i64> = source.query_row(
        "SELECT MIN(rowid) FROM model_usage WHERE status = 'running'",
        [],
        |row| row.get(0),
    )?;
    let watermark = min_running.map_or(cursor, |running| cursor.min(running - 1));
    // 水位随导入行一起提交；被 rollup 剪掉的旧行绝不能重导。
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

    const SCHEMA: &str = "CREATE TABLE model_usage (
        id text primary key,
        session_id text not null,
        model_id text not null,
        status text not null check(status in ('running', 'completed', 'error', 'cancelled')),
        started_at integer not null,
        duration_ms integer,
        time_to_first_token_ms integer,
        error_message text,
        input_tokens integer not null default 0,
        output_tokens integer not null default 0,
        reasoning_tokens integer not null default 0,
        cache_read_input_tokens integer not null default 0,
        cache_creation_input_tokens integer not null default 0);";

    fn insert(source: &Connection, sql: &str) {
        source.execute_batch(sql).unwrap();
    }

    #[test]
    fn zcode_usage_imports_cache_inclusive_tokens() {
        let source = Connection::open_in_memory().unwrap();
        insert(
            &source,
            &format!(
                "{SCHEMA} INSERT INTO model_usage VALUES
            ('r1','s1','GLM-5.3','completed',1792510633226,4120,180,NULL,74916,1412,0,72384,0);"
            ),
        );
        let db = Database::memory().unwrap();
        {
            // 内存库会播种内置定价；这条用例验证「无定价」路径，先清掉
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM model_pricing", []).unwrap();
        }
        assert_eq!(
            sync_from_database(&db, &source, "zcode-test")
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT model, input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, status_code, data_source,
                        input_token_semantics, latency_ms, first_token_ms, duration_ms,
                        created_at, CAST(total_cost_usd AS REAL)
                 FROM proxy_request_logs WHERE app_type='zcode'",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, Option<i64>>(9)?,
                        r.get::<_, Option<i64>>(10)?,
                        r.get::<_, i64>(11)?,
                        r.get::<_, f64>(12)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "GLM-5.3");
        assert_eq!((row.1, row.2, row.3, row.4), (74916, 1412, 72384, 0));
        assert_eq!(row.5, 200);
        assert_eq!(row.6, "zcode_session");
        assert_eq!(row.7, INPUT_TOKEN_SEMANTICS_TOTAL);
        assert_eq!(
            (row.8, row.9, row.10),
            (4120i64, Some(180i64), Some(4120i64))
        );
        assert_eq!(row.11, 1792510633);
        // 无定价时费用记 0，token 照常入库
        assert_eq!(row.12, 0.0);
        // cache-inclusive 语义：fresh input 应扣掉 cache read
        let fresh: i64 = conn
            .query_row(
                &format!(
                    "SELECT {} FROM proxy_request_logs WHERE app_type='zcode'",
                    crate::services::sql_helpers::fresh_input_sql("")
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fresh, 74916 - 72384);
    }

    #[test]
    fn zcode_usage_prices_output_with_reasoning_tokens() {
        let source = Connection::open_in_memory().unwrap();
        insert(
            &source,
            &format!(
                "{SCHEMA} INSERT INTO model_usage VALUES
            ('r1','s1','GLM-5.3','completed',1792510633226,1000,100,NULL,1000,1412,588,0,0);"
            ),
        );
        let db = Database::memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            // 定价查找会把模型名归一成小写，种子表里的 glm-5.3 就是这么命中的；
            // 覆盖小写这行才能真正替换生效
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (model_id, display_name, input_cost_per_million,
                    output_cost_per_million, cache_read_cost_per_million, cache_creation_cost_per_million)
                 VALUES ('glm-5.3','GLM-5.3','1','2','0.1','0')",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            sync_from_database(&db, &source, "pricing")
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let (output_tokens, cost) = conn
            .query_row(
                "SELECT output_tokens, CAST(total_cost_usd AS REAL) FROM proxy_request_logs",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
            )
            .unwrap();
        // reasoning 并入 output 计费：input 1000×$1/M + output (1412+588)×$2/M = $0.005
        assert_eq!(output_tokens, 2000);
        assert!((cost - 0.005).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn zcode_usage_maps_error_and_cancelled_status() {
        let source = Connection::open_in_memory().unwrap();
        insert(
            &source,
            &format!(
                "{SCHEMA} INSERT INTO model_usage VALUES
            ('r1','s1','GLM-5.3','error',1792510633226,100,NULL,'boom: stack overflow',0,0,0,0,0),
            ('r2','s1','GLM-5.3','cancelled',1792510640000,50,10,NULL,0,0,0,0,0);"
            ),
        );
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "status").unwrap().imported,
            2
        );
        let conn = db.conn.lock().unwrap();
        let rows = {
            let mut stmt = conn
                .prepare("SELECT status_code, error_message, latency_ms FROM proxy_request_logs ORDER BY status_code")
                .unwrap();
            stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 499);
        assert_eq!(rows[1].0, 500);
        assert_eq!(rows[1].1.as_deref(), Some("boom: stack overflow"));
        assert_eq!(rows[1].2, 100);
    }

    #[test]
    fn zcode_usage_watermark_waits_for_running_rows_and_needs_reimports() {
        let source = Connection::open_in_memory().unwrap();
        // rowid 1 先 running；rowid 2/3 已终态
        insert(
            &source,
            &format!(
                "{SCHEMA} INSERT INTO model_usage VALUES
            ('r1','s1','GLM-5.3','running',1792510000000,NULL,NULL,NULL,0,0,0,0,0),
            ('r2','s1','GLM-5.3','completed',1792510633226,100,10,NULL,10,10,0,0,0),
            ('r3','s1','GLM-5.3','completed',1792510640000,100,10,NULL,10,10,0,0,0);"
            ),
        );
        let db = Database::memory().unwrap();
        let first = sync_from_database(&db, &source, "watermark").unwrap();
        assert_eq!(first.imported, 2);
        {
            // 水位停在 running 行之前：min(3, 1-1) = 0
            let conn = db.conn.lock().unwrap();
            let watermark: i64 = conn
                .query_row(
                    "SELECT last_line_offset FROM session_log_sync WHERE file_path='watermark'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(watermark, 0);
        }
        // running 行完成后能被补扫；已导入的 2 行靠 request_id 去重
        source
            .execute(
                "UPDATE model_usage SET status='completed', input_tokens=7 WHERE id='r1'",
                [],
            )
            .unwrap();
        let second = sync_from_database(&db, &source, "watermark").unwrap();
        assert_eq!(second.imported, 1);
        assert_eq!(second.skipped, 2);
        // 再来一轮：全量去重，不重导
        let third = sync_from_database(&db, &source, "watermark").unwrap();
        assert_eq!(third.imported, 0);
        // 新增行照常增量导入
        insert(
            &source,
            "INSERT INTO model_usage VALUES
            ('r4','s1','GLM-5.3','completed',1792510700000,100,10,NULL,5,5,0,0,0);",
        );
        let fourth = sync_from_database(&db, &source, "watermark").unwrap();
        assert_eq!(fourth.imported, 1);
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;

    #[test]
    #[ignore = "requires the real ZCode database; set ZCODE_HOME to run"]
    fn zcode_reads_real_usage_database() -> Result<(), AppError> {
        assert!(std::env::var("ZCODE_HOME").is_ok());
        let db = Database::memory()?;
        let imported = sync_zcode_usage(&db)?;
        assert!(imported.imported > 0);
        assert_eq!(sync_zcode_usage(&db)?.imported, 0);
        let native = open_database(&database_path())?;
        let expected = native.query_row(
            "SELECT SUM(input_tokens), SUM(output_tokens + reasoning_tokens),
                    SUM(cache_read_input_tokens), SUM(cache_creation_input_tokens)
             FROM model_usage WHERE status != 'running'",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?;
        let conn = lock_conn!(db.conn);
        let actual = conn.query_row(
            "SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens),
                    SUM(cache_creation_tokens)
             FROM proxy_request_logs WHERE app_type='zcode'",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
