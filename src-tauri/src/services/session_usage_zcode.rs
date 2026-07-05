//! ZCode 会话日志使用追踪
//!
//! 从 ZCode CLI 的本地 SQLite 数据库 (`~/.zcode/cli/db/db.sqlite`) 读取
//! `model_usage` 精确 token 记录，并写入 CC Switch 的统一使用统计表。
//!
//! ## 数据流
//! ```text
//! ~/.zcode/cli/db/db.sqlite:model_usage → 费用计算 → proxy_request_logs 表
//! ```

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

struct ZCodeUsageRow {
    id: String,
    session_id: Option<String>,
    provider_id: String,
    model_id: String,
    status: String,
    started_at_ms: Option<i64>,
    first_token_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
    duration_ms: Option<i64>,
    time_to_first_token_ms: Option<i64>,
    input_tokens: u32,
    output_tokens: u32,
    cache_creation_tokens: u32,
    cache_read_tokens: u32,
    error_message: Option<String>,
}

/// 同步 ZCode 使用数据。
pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_zcode_usage_db_path();

    if !db_path.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            errors: vec![],
        });
    }

    let db_path_str = db_path.to_string_lossy().to_string();
    let metadata = fs::metadata(&db_path)
        .map_err(|e| AppError::Config(format!("无法读取 ZCode 数据库元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    // SQLite WAL 模式下新数据可能只落在 -wal 文件里。
    for sidecar in [
        db_path.with_extension("sqlite-wal"),
        db_path.with_extension("sqlite-shm"),
    ] {
        if let Ok(meta) = fs::metadata(&sidecar) {
            file_modified = file_modified.max(metadata_modified_nanos(&meta));
        }
    }

    let (last_modified, _) = get_sync_state(db, &db_path_str)?;
    if file_modified <= last_modified {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 1,
            errors: vec![],
        });
    }

    let zcode_conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 ZCode 使用数据库: {e}")))?;

    let rows = query_model_usage_rows(&zcode_conn)?;
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        errors: vec![],
    };

    for row in &rows {
        match insert_zcode_usage(db, row) {
            Ok(true) => result.imported += 1,
            Ok(false) => result.skipped += 1,
            Err(e) => {
                let msg = format!("ZCode 使用记录插入失败 {}: {e}", row.id);
                log::warn!("[ZCODE-SYNC] {msg}");
                result.errors.push(msg);
                result.skipped += 1;
            }
        }
    }

    if result.errors.is_empty() {
        update_sync_state(db, &db_path_str, file_modified, rows.len() as i64)?;
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

fn query_model_usage_rows(conn: &rusqlite::Connection) -> Result<Vec<ZCodeUsageRow>, AppError> {
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_usage')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !has_table {
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare(
            "SELECT id,
                    session_id,
                    COALESCE(provider_id, '_zcode_session') AS provider_id,
                    COALESCE(model_id, 'unknown') AS model_id,
                    COALESCE(status, 'completed') AS status,
                    started_at,
                    first_token_at,
                    completed_at,
                    duration_ms,
                    time_to_first_token_ms,
                    COALESCE(input_tokens, 0) AS input_tokens,
                    COALESCE(output_tokens, 0) AS output_tokens,
                    COALESCE(cache_creation_input_tokens, 0) AS cache_creation_input_tokens,
                    COALESCE(cache_read_input_tokens, 0) AS cache_read_input_tokens,
                    error_message
             FROM model_usage
             WHERE completed_at IS NOT NULL
               AND status IN ('completed', 'cancelled', 'error')
               AND (
                   COALESCE(input_tokens, 0) > 0
                OR COALESCE(output_tokens, 0) > 0
                OR COALESCE(cache_creation_input_tokens, 0) > 0
                OR COALESCE(cache_read_input_tokens, 0) > 0
               )
             ORDER BY COALESCE(completed_at, started_at, 0), id",
        )
        .map_err(|e| AppError::Database(format!("准备 ZCode 使用记录查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            let provider_id = row.get::<_, String>(2)?;
            Ok(ZCodeUsageRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                provider_id: if provider_id.trim().is_empty() {
                    "_zcode_session".to_string()
                } else {
                    provider_id
                },
                model_id: row.get(3)?,
                status: row.get(4)?,
                started_at_ms: row.get(5)?,
                first_token_at_ms: row.get(6)?,
                completed_at_ms: row.get(7)?,
                duration_ms: row.get(8)?,
                time_to_first_token_ms: row.get(9)?,
                input_tokens: row.get::<_, i64>(10)?.max(0) as u32,
                output_tokens: row.get::<_, i64>(11)?.max(0) as u32,
                cache_creation_tokens: row.get::<_, i64>(12)?.max(0) as u32,
                cache_read_tokens: row.get::<_, i64>(13)?.max(0) as u32,
                error_message: row.get(14)?,
            })
        })
        .map_err(|e| AppError::Database(format!("查询 ZCode 使用记录失败: {e}")))?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| AppError::Database(format!("读取 ZCode 使用记录失败: {e}")))?);
    }

    Ok(result)
}

fn insert_zcode_usage(db: &Database, row: &ZCodeUsageRow) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let request_id = format!("zcode_session:{}", row.id);
    let created_at = row
        .completed_at_ms
        .or(row.started_at_ms)
        .map(|ms| ms / 1000)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    let dedup_key = DedupKey {
        app_type: "zcode",
        model: &row.model_id,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cache_read_tokens: row.cache_read_tokens,
        cache_creation_tokens: row.cache_creation_tokens,
        created_at,
    };
    if should_skip_session_insert(&conn, &request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cache_read_tokens: row.cache_read_tokens,
        cache_creation_tokens: row.cache_creation_tokens,
        model: Some(row.model_id.clone()),
        message_id: None,
    };

    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match find_model_pricing(&conn, &row.model_id) {
            Some(pricing) => {
                let cost = CostCalculator::calculate_for_app("zcode", &usage, &pricing, multiplier);
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

    let latency_ms = row
        .duration_ms
        .or_else(|| Some(row.completed_at_ms? - row.started_at_ms?))
        .unwrap_or(0)
        .max(0);
    let first_token_ms = row.time_to_first_token_ms.or_else(|| {
        let started = row.started_at_ms?;
        let first = row.first_token_at_ms?;
        (first >= started).then_some(first - started)
    });
    let status_code = match row.status.as_str() {
        "completed" => 200i64,
        "cancelled" => 499i64,
        _ => 500i64,
    };

    let inserted_rows = conn
        .execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, duration_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at, data_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
                request_id,
                row.provider_id,
                "zcode",
                row.model_id,
                row.model_id,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                latency_ms,
                first_token_ms,
                latency_ms,
                status_code,
                row.error_message,
                row.session_id,
                Some("zcode_session"),
                1i64,
                "1.0",
                created_at,
                "zcode_session",
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 ZCode 使用记录失败: {e}")))?;

    if inserted_rows > 0 {
        crate::usage_events::notify_log_recorded();
    }

    Ok(inserted_rows > 0)
}
