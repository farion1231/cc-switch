//! Codex 使用数据导入
//!
//! 从 ~/.codex/state_5.sqlite 的 threads 表读取 token 使用数据，
//! 导入到 proxy_request_logs 表。
//!
//! ## 限制
//! - Thread 级粒度（非请求级）
//! - 只有总 token 数，使用估算比例分 input/output

use crate::codex_config::get_codex_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::SessionSyncResult;
use rust_decimal::Decimal;

/// 默认 input/output 比例（70% input, 30% output）
const DEFAULT_INPUT_RATIO: f64 = 0.7;

/// 同步 Codex 使用数据
pub fn sync_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let codex_dir = get_codex_config_dir();
    let state_db_path = codex_dir.join("state_5.sqlite");

    if !state_db_path.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            errors: vec![],
        });
    }

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        errors: vec![],
    };

    // 只读打开 Codex SQLite 数据库
    let codex_conn = match rusqlite::Connection::open_with_flags(
        &state_db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            result.errors.push(format!("无法打开 Codex 数据库: {e}"));
            return Ok(result);
        }
    };

    // 查询所有 thread
    let mut stmt = match codex_conn.prepare(
        "SELECT id, model, model_provider, tokens_used, created_at, title
         FROM threads
         WHERE tokens_used > 0
         ORDER BY created_at DESC",
    ) {
        Ok(s) => s,
        Err(e) => {
            result
                .errors
                .push(format!("查询 Codex threads 失败: {e}"));
            return Ok(result);
        }
    };

    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,  // id
            row.get::<_, Option<String>>(1)?, // model (nullable)
            row.get::<_, String>(2)?,  // model_provider
            row.get::<_, i64>(3)?,     // tokens_used
            row.get::<_, i64>(4)?,     // created_at (unix seconds)
            row.get::<_, String>(5)?,  // title
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            result.errors.push(format!("遍历 threads 失败: {e}"));
            return Ok(result);
        }
    };

    let conn = lock_conn!(db.conn);

    for row_result in rows {
        let (thread_id, model, _provider, tokens_used, created_at, _title) = match row_result {
            Ok(r) => r,
            Err(e) => {
                result.errors.push(format!("读取行失败: {e}"));
                continue;
            }
        };

        let request_id = format!("codex_thread:{thread_id}");
        let model = model.unwrap_or_else(|| "unknown".to_string());

        // 检查是否已存在
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
                rusqlite::params![request_id],
                |row| row.get::<_, i64>(0).map(|c| c > 0),
            )
            .unwrap_or(false);

        if exists {
            result.skipped += 1;
            continue;
        }

        // 使用估算比例分配 input/output
        let input_tokens = (tokens_used as f64 * DEFAULT_INPUT_RATIO) as u32;
        let output_tokens = tokens_used as u32 - input_tokens;

        let usage = TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: Some(model.clone()),
        };

        // 查找定价
        let pricing = find_codex_pricing(&conn, &model);
        let multiplier = Decimal::from(1);
        let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
            match pricing {
                Some(p) => {
                    let cost = CostCalculator::calculate(&usage, &p, multiplier);
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

        let insert_result = conn.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at, data_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                request_id,
                "_codex_session",
                "codex",
                model,
                model,
                input_tokens,
                output_tokens,
                0i64,    // cache_read_tokens
                0i64,    // cache_creation_tokens
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,    // latency_ms
                Option::<i64>::None,
                200i64,  // status_code
                Option::<String>::None,
                Some(thread_id),  // session_id = thread_id
                Some("codex_db"),
                0i64,    // is_streaming: unknown
                "1.0",
                created_at,
                "codex_db",
            ],
        );

        match insert_result {
            Ok(changed) if changed > 0 => result.imported += 1,
            Ok(_) => result.skipped += 1,
            Err(e) => {
                result.errors.push(format!("插入失败: {e}"));
                result.skipped += 1;
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[CODEX-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条",
            result.imported,
            result.skipped
        );
    }

    Ok(result)
}

/// 查找 Codex 模型定价
fn find_codex_pricing(
    conn: &rusqlite::Connection,
    model_id: &str,
) -> Option<ModelPricing> {
    // 精确匹配
    let result = conn.query_row(
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
    );

    match result {
        Ok((input, output, cache_read, cache_creation)) => {
            ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation).ok()
        }
        Err(_) => {
            // 尝试 LIKE 匹配
            let pattern = format!("{model_id}%");
            conn.query_row(
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
            )
            .ok()
            .and_then(|(i, o, cr, cc)| ModelPricing::from_strings(&i, &o, &cr, &cc).ok())
        }
    }
}
