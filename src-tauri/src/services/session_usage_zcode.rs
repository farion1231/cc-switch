//! ZCode 会话日志使用追踪
//!
//! 从 `~/.zcode/cli/db/db.sqlite` (SQLite) 的 `model_usage` 表中提取精确的
//! token 使用数据，映射到统一的 `proxy_request_logs` 表。
//!
//! ## 数据流
//! ```text
//! ~/.zcode/cli/db/db.sqlite
//!   → model_usage 表（每行一次推理请求，含 tokens / duration / status）
//!   → 过滤 status != 'running' 的已完成记录
//!   → proxy_request_logs 表
//! ```
//!
//! ## WAL 模式说明
//! ZCode 的数据库运行在 WAL 模式：新提交先落在 `-wal` 文件里，主库文件
//! 只有在 checkpoint 时才更新。因此必须同时考虑 `-wal` 的 mtime，否则
//! 会在 checkpoint 之前漏掉刚写入的记录。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use crate::zcode_config::get_zcode_usage_db_path;
use rust_decimal::Decimal;
use std::fs;
use std::time::SystemTime;

/// 从 ZCode model_usage 表中提取的单条使用记录
struct ZCodeUsageRow {
    id: String,
    provider_id: Option<String>,
    model_id: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    duration_ms: Option<i64>,
    time_to_first_token_ms: Option<i64>,
    status: String,
    started_at: Option<i64>,
    session_id: Option<String>,
}

/// 同步 ZCode 使用数据
pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_zcode_usage_db_path();

    if !db_path.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            suspected_duplicates: 0,
            deferred_files: 0,
            errors: vec![],
        });
    }

    let db_path_str = db_path.to_string_lossy().to_string();

    // 检查文件修改时间（含 -wal 侧车文件，原因见模块文档）
    let metadata = fs::metadata(&db_path)
        .map_err(|e| AppError::Config(format!("无法读取 ZCode db.sqlite 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    let wal_path = db_path.with_extension("sqlite-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    let (last_modified, _last_offset) = get_sync_state(db, &db_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 1,
            suspected_duplicates: 0,
            deferred_files: 0,
            errors: vec![],
        });
    }

    // 打开 ZCode 的 SQLite 数据库（只读）
    let zcode_conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 ZCode db.sqlite: {e}")))?;

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    // 查询所有已完成（status != 'running'）的记录
    let rows = match query_model_usage(&zcode_conn) {
        Ok(rows) => rows,
        Err(e) => {
            // 表不存在（旧版 ZCode 或空库）时静默跳过，不当作错误
            log::debug!("[ZCODE-SYNC] 无法查询 model_usage 表（可能 ZCode 版本不支持）: {e}");
            return Ok(result);
        }
    };

    let mut has_error = false;
    for row in rows {
        match insert_zcode_usage(db, &row) {
            Ok(true) => result.imported += 1,
            Ok(false) => result.skipped += 1,
            Err(e) => {
                let msg = format!("ZCode 用量插入失败 {}: {e}", row.id);
                log::warn!("[ZCODE-SYNC] {msg}");
                result.errors.push(msg);
                result.skipped += 1;
                has_error = true;
            }
        }
    }

    // 仅在本轮没有错误时推进文件级状态；否则保留下次重试入口。
    if !has_error {
        update_sync_state(db, &db_path_str, file_modified, 0)?;
    }

    if result.imported > 0 {
        log::info!(
            "[ZCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条",
            result.imported,
            result.skipped
        );
    }

    Ok(result)
}

/// 查询所有已完成的 model_usage 记录（status != 'running'）
fn query_model_usage(conn: &rusqlite::Connection) -> Result<Vec<ZCodeUsageRow>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, provider_id, model_id,
                    COALESCE(input_tokens, 0), COALESCE(output_tokens, 0),
                    COALESCE(reasoning_tokens, 0),
                    COALESCE(cache_read_input_tokens, 0),
                    COALESCE(cache_creation_input_tokens, 0),
                    duration_ms, time_to_first_token_ms,
                    COALESCE(status, 'completed'), started_at, session_id
             FROM model_usage
             WHERE status IS NULL OR status != 'running'",
        )
        .map_err(|e| AppError::Database(format!("准备 model_usage 查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ZCodeUsageRow {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model_id: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                reasoning_tokens: row.get(5)?,
                cache_read_tokens: row.get(6)?,
                cache_creation_tokens: row.get(7)?,
                duration_ms: row.get(8)?,
                time_to_first_token_ms: row.get(9)?,
                status: row.get(10)?,
                started_at: row.get(11)?,
                session_id: row.get(12)?,
            })
        })
        .map_err(|e| AppError::Database(format!("查询 model_usage 失败: {e}")))?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| AppError::Database(format!("读取 model_usage 行失败: {e}")))?);
    }

    Ok(result)
}

/// 将 status 文本映射为 HTTP 风格的 status_code
fn status_to_code(status: &str) -> i64 {
    match status {
        "completed" => 200,
        "error" => 500,
        "cancelled" => 499,
        // running 已在查询中过滤，兜底按成功处理
        _ => 200,
    }
}

/// 插入单条 ZCode 用量记录到 proxy_request_logs
fn insert_zcode_usage(db: &Database, row: &ZCodeUsageRow) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    // 跳过全零 token 的记录（无意义的空用量）
    if row.input_tokens == 0
        && row.output_tokens == 0
        && row.reasoning_tokens == 0
        && row.cache_read_tokens == 0
        && row.cache_creation_tokens == 0
    {
        return Ok(false);
    }

    let model_id = row
        .model_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // created_at：ZCode 的 started_at 是毫秒级时间戳，转成秒
    let created_at = row
        .started_at
        .filter(|t| *t > 0)
        .map(|t| t / 1000)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    // ZCode 使用 Anthropic 风格：output 不含 reasoning，单独累加
    let output_with_reasoning = row.output_tokens + row.reasoning_tokens;

    // request_id 加前缀防跨源冲突
    let request_id = format!("zcode_session:{}", row.id);

    let dedup_key = DedupKey {
        app_type: "zcode",
        model: &model_id,
        input_tokens: row.input_tokens as u32,
        output_tokens: output_with_reasoning as u32,
        cache_read_tokens: row.cache_read_tokens as u32,
        cache_creation_tokens: row.cache_creation_tokens as u32,
        created_at,
    };
    if should_skip_session_insert(&conn, &request_id, &dedup_key)? {
        return Ok(false);
    }

    // ZCode 不提供费用，使用 cc-switch 自带的模型定价计算
    let usage = TokenUsage {
        input_tokens: row.input_tokens as u32,
        output_tokens: output_with_reasoning as u32,
        cache_read_tokens: row.cache_read_tokens as u32,
        cache_creation_tokens: row.cache_creation_tokens as u32,
        model: Some(model_id.clone()),
        message_id: None,
    };

    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match find_model_pricing(&conn, &model_id) {
            Some(pricing) => {
                let cost =
                    CostCalculator::calculate_for_app("zcode", &usage, &pricing, Decimal::from(1));
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

    let provider_id = row
        .provider_id
        .clone()
        .unwrap_or_else(|| "_zcode_session".to_string());

    let status_code = status_to_code(&row.status);

    let inserted_rows = conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        rusqlite::params![
            request_id,
            provider_id,
            "zcode",              // app_type
            model_id,
            model_id,             // request_model = model
            row.input_tokens,
            output_with_reasoning,
            row.cache_read_tokens,
            row.cache_creation_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            row.duration_ms.unwrap_or(0),
            row.time_to_first_token_ms,
            status_code,
            Option::<String>::None,
            row.session_id,
            Some("zcode_session"),// provider_type
            1i64,                 // is_streaming
            "1.0",                // cost_multiplier
            created_at,
            "zcode_session",      // data_source
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 ZCode 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_to_code_mapping() {
        assert_eq!(status_to_code("completed"), 200);
        assert_eq!(status_to_code("error"), 500);
        assert_eq!(status_to_code("cancelled"), 499);
        // unknown / running (filtered upstream) fall back to 200
        assert_eq!(status_to_code("running"), 200);
        assert_eq!(status_to_code("something_else"), 200);
    }

    #[test]
    fn test_query_model_usage_skips_running() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE model_usage (
                id TEXT, provider_id TEXT, model_id TEXT,
                input_tokens INTEGER, output_tokens INTEGER, reasoning_tokens INTEGER,
                cache_read_input_tokens INTEGER, cache_creation_input_tokens INTEGER,
                duration_ms INTEGER, time_to_first_token_ms INTEGER,
                status TEXT, started_at INTEGER, session_id TEXT
             );
             INSERT INTO model_usage VALUES
                ('done', 'bigmodel', 'GLM-5.2', 1000, 200, 50, 300, 0, 1200, 300, 'completed', 1700000000000, 's1'),
                ('wip',  'bigmodel', 'GLM-5.2', 500, 0, 0, 0, 0, NULL, NULL, 'running', 1700000001000, 's1');",
        )
        .unwrap();

        let rows = query_model_usage(&conn).unwrap();
        // running 行被过滤，只剩 completed
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "done");
        assert_eq!(rows[0].input_tokens, 1000);
        assert_eq!(rows[0].reasoning_tokens, 50);
        assert_eq!(rows[0].cache_read_tokens, 300);
    }
}
