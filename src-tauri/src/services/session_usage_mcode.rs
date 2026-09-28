//! Import MCode's committed token-usage projection without modifying its database.
//!
//! `local_runtime_token_usage` 的 model 列上游长期为 NULL，模型名按四级回退补全：
//! 行内 model → 同 turn 消息遥测（`context_usage_telemetry.model`，请求级）→
//! 会话 `effectiveModel` → `'unknown'`。同 turn 仅首行携带 turn 级耗时，
//! 保证 `SUM(latency_ms)` 的延迟统计语义。存量 `unknown` 行在每次同步时
//! 幂等回填（模型 + 命中定价时重算成本）。
use std::collections::{HashMap, HashSet};

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::{session_usage::SessionSyncResult, usage_stats::find_model_pricing};
use crate::session_manager::providers::mcode;
use rusqlite::params;
use rust_decimal::Decimal;

/// 同一 turn 的请求级遥测，来自 `local_runtime_message_rows` 的 assistant 行。
struct TurnMeta {
    model: String,
    latency_ms: i64,
    first_token_ms: i64,
}

/// (turn 级遥测映射, 会话级兜底模型映射)。
type SourceMeta = (HashMap<String, TurnMeta>, HashMap<String, String>);

/// 剥离 provider 命名空间前缀：
/// `minimax/MiniMax-M3` → `MiniMax-M3`，
/// `custom_provider:zhipu-ai-coding-plan/glm-5.3-flash` → `glm-5.3-flash`。
fn strip_model_namespace(native_model: &str) -> &str {
    native_model
        .split_once('/')
        .map_or(native_model, |(_, model)| model)
}

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// 从源库构建 turn 级遥测与会话级兜底模型映射。
/// `local_runtime_message_rows` / `local_runtime_sessions` 表缺失（老版本 mcode）
/// 时对应映射为空，模型退回 `'unknown'`。
fn load_source_meta(source: &rusqlite::Connection) -> Result<SourceMeta, AppError> {
    let mut turns: HashMap<String, TurnMeta> = HashMap::new();
    if table_exists(source, "local_runtime_message_rows") {
        let mut stmt = source.prepare(
            "SELECT turn_id,
                    MAX(json_extract(data_json, '$.context_usage_telemetry.model')),
                    COALESCE(MAX(json_extract(data_json, '$.usage.request_duration_ms')), 0),
                    COALESCE(MAX(json_extract(data_json, '$.thinking_duration_ms')), 0)
             FROM local_runtime_message_rows
             WHERE role = 'assistant' AND turn_id IS NOT NULL
             GROUP BY turn_id
             HAVING MAX(json_extract(data_json, '$.context_usage_telemetry.model')) IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                TurnMeta {
                    model: row.get::<_, String>(1)?,
                    latency_ms: row.get::<_, i64>(2)?,
                    first_token_ms: row.get::<_, i64>(3)?,
                },
            ))
        })?;
        for row in rows {
            let (turn_id, meta) = row?;
            turns.insert(turn_id, meta);
        }
    }

    let mut sessions: HashMap<String, String> = HashMap::new();
    if table_exists(source, "local_runtime_sessions") {
        let mut stmt = source.prepare(
            "SELECT session_id, json_extract(extra_data_json, '$.effectiveModel')
             FROM local_runtime_sessions
             WHERE json_extract(extra_data_json, '$.effectiveModel') IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (session_id, model) = row?;
            sessions.insert(session_id, model);
        }
    }
    Ok((turns, sessions))
}

/// 收集 ccs 库中 `model='unknown'` 的 mcode 存量行的完整 request_id。
/// 必须保留完整身份：切换 `MINIMAX_DATA_DIR` 后不同源库可能复用相同数字 id，
/// 仅凭 id 无法区分请求，会把新库的行误判为回填而静默漏记。
fn collect_backfill_request_ids(
    tx: &rusqlite::Transaction<'_>,
) -> Result<HashSet<String>, AppError> {
    let mut stmt = tx.prepare(
        "SELECT request_id FROM proxy_request_logs
         WHERE data_source = 'mcode_session' AND model = 'unknown'",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut ids = HashSet::new();
    for row in rows {
        ids.insert(row?);
    }
    Ok(ids)
}

/// 源库中每个 turn 的最小行 id —— turn 级耗时的稳定归属行。
/// 以全源库 `MIN(id)` 判定而非"本轮首次遇到"，保证同 turn 分片跨同步轮次
/// 拆分导入时，耗时仍只随首行落库一次，`SUM(latency_ms)` 不随同步时机变化。
fn load_turn_first_ids(source: &rusqlite::Connection) -> Result<HashMap<String, i64>, AppError> {
    let mut first_ids = HashMap::new();
    let mut stmt = source.prepare(
        "SELECT turn_id, MIN(id) FROM local_runtime_token_usage
         WHERE turn_id IS NOT NULL GROUP BY turn_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (turn_id, min_id) = row?;
        first_ids.insert(turn_id, min_id);
    }
    Ok(first_ids)
}

/// 成本解析：native cost 仅在明确为正时优先（mcode 上游不计算成本，全为 0），
/// 否则按补全后的模型查定价计算；无定价则落 '0'。
fn resolve_cost(
    tx: &rusqlite::Transaction<'_>,
    native_cost: Option<f64>,
    usage: &TokenUsage,
    model: &str,
) -> Result<String, AppError> {
    Ok(match native_cost {
        Some(cost) if cost.is_finite() && cost > 0.0 => cost.to_string(),
        _ => find_model_pricing(tx, model)
            .map(|pricing| {
                CostCalculator::calculate_for_app("mcode", usage, &pricing, Decimal::ONE)
                    .total_cost
                    .to_string()
            })
            .unwrap_or_else(|| "0".into()),
    })
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
    let mut result = SessionSyncResult::default();
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut cursor = tx.query_row(
        "SELECT COALESCE(MAX(last_line_offset), 0) FROM session_log_sync WHERE file_path = ?1",
        [&key],
        |row| row.get::<_, i64>(0),
    )?;
    let (turn_meta, session_models) = load_source_meta(source)?;
    let turn_first_ids = load_turn_first_ids(source)?;
    // 存量 unknown 行在本次事务中一并重读源库并回填，幂等：
    // 回填成功后 model 不再是 'unknown'，下一轮自然不再命中。
    let backfill_request_ids = collect_backfill_request_ids(&tx)?;
    // 数字 id 仅用于扩大源库查询范围（覆盖已入库的 unknown 行）；
    // 是否走回填 UPDATE 分支必须以完整 request_id 判定：切换数据目录后
    // 不同源库可能复用相同 id 但属于不同请求，此时应正常 INSERT 而非漏记。
    let backfill_source_ids: HashSet<i64> = backfill_request_ids
        .iter()
        .filter_map(|request_id| request_id.strip_prefix("mcode:"))
        .filter_map(|rest| rest.rsplit_once(':'))
        .filter_map(|(_, id)| id.parse::<i64>().ok())
        .collect();
    // id 均为内部解析出的 i64，直接拼接无注入风险。
    let id_filter = if backfill_source_ids.is_empty() {
        format!("id > {cursor}")
    } else {
        let ids = backfill_source_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("id > {cursor} OR id IN ({ids})")
    };
    let mut query = source.prepare(&format!(
        "SELECT id, session_id, model, ts, input_tokens,
                output_tokens, reasoning_tokens, cache_read_tokens, cache_write_tokens,
                cost_usd, turn_id
         FROM local_runtime_token_usage WHERE {id_filter} ORDER BY id"
    ))?;
    let mut rows = query.query([])?;
    result.files_scanned = 1;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let session_id: String = row.get(1)?;
        let native_model: Option<String> = row.get(2)?;
        let turn_id: Option<String> = row.get(10)?;
        let meta = turn_id.as_deref().and_then(|turn| turn_meta.get(turn));
        // 模型四级回退：行内 model → 同 turn 消息遥测 → 会话 effectiveModel → unknown
        let model = native_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(strip_model_namespace)
            .or_else(|| meta.map(|meta| meta.model.as_str()))
            .or_else(|| {
                session_models
                    .get(&session_id)
                    .map(|model| strip_model_namespace(model))
            })
            .unwrap_or("unknown");
        let usage = TokenUsage {
            input_tokens: row.get(4)?,
            output_tokens: row.get::<_, u32>(5)?.saturating_add(row.get(6)?),
            cache_read_tokens: row.get(7)?,
            cache_creation_tokens: row.get(8)?,
            model: Some(model.into()),
            message_id: None,
        };
        let native_cost: Option<f64> = row.get(9)?;
        let cost = resolve_cost(&tx, native_cost, &usage, model)?;
        // 同 turn 仅首行（全源库最小 id 的行）携带 turn 级耗时
        // （request_duration_ms / thinking_duration_ms），其余分片行保持 0，
        // 保证 SUM(latency_ms) 的平均延迟统计不被放大，且跨同步轮次结果一致。
        let carries_latency = turn_id.as_deref().is_some_and(|turn| {
            meta.is_some() && turn_first_ids.get(turn).is_some_and(|&min_id| min_id == id)
        });
        let (latency_ms, first_token_ms) = if carries_latency {
            (
                meta.map(|meta| meta.latency_ms.max(0)).unwrap_or(0),
                meta.map(|meta| meta.first_token_ms.max(0)).unwrap_or(0),
            )
        } else {
            (0, 0)
        };
        let request_id = format!("mcode:{session_id}:{id}");
        if backfill_request_ids.contains(&request_id) {
            // 存量回填：仅在能确定模型时改写，避免反复触碰无信息的行。
            if model == "unknown" {
                result.skipped += 1;
            } else {
                let changed = if carries_latency {
                    tx.execute(
                        "UPDATE proxy_request_logs
                         SET model = ?1, request_model = ?1, total_cost_usd = ?2,
                             latency_ms = ?3, first_token_ms = ?4
                         WHERE request_id = ?5 AND model = 'unknown'",
                        params![model, cost, latency_ms, first_token_ms, request_id],
                    )?
                } else {
                    tx.execute(
                        "UPDATE proxy_request_logs
                         SET model = ?1, request_model = ?1, total_cost_usd = ?2
                         WHERE request_id = ?3 AND model = 'unknown'",
                        params![model, cost, request_id],
                    )?
                };
                result.imported += changed as u32;
                result.skipped += u32::from(changed == 0);
            }
        } else {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, first_token_ms, status_code, session_id,
                    provider_type, is_streaming, cost_multiplier, created_at, data_source,
                    input_token_semantics
                 ) VALUES (?1, '_mcode_session', 'mcode', ?2, ?2, ?3, ?4, ?5, ?6,
                           '0', '0', '0', '0', ?7, ?8, ?9, 200, ?10, 'mcode_session', 1, '1',
                           ?11, 'mcode_session', ?12)",
                params![
                    request_id,
                    model,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens,
                    cost,
                    latency_ms,
                    first_token_ms,
                    session_id,
                    row.get::<_, i64>(3)? / 1000,
                    INPUT_TOKEN_SEMANTICS_FRESH
                ],
            )?;
            result.imported += changed as u32;
            result.skipped += u32::from(changed == 0);
        }
        // 回填行 id 可能小于水位，绝不回退 watermark。
        cursor = cursor.max(id);
    }
    // Commit the watermark with the imported rows; pruned logs must never be reimported.
    tx.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, ?2, unixepoch()) ON CONFLICT(file_path) DO UPDATE SET
         last_line_offset = excluded.last_line_offset, last_synced_at = excluded.last_synced_at",
        params![key, cursor],
    )?;
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_source_with_usage() -> rusqlite::Connection {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES
            (1,'s1','custom_provider:router/vendor/model',100000,1,2,0,0,0,0.25,'t1'),
            (2,'s1','custom_provider:router/another/model',101000,1,2,0,0,0,0.5,'t2');",
            )
            .unwrap();
        source
    }

    #[test]
    fn mcode_usage_preserves_model_namespaces() {
        let source = memory_source_with_usage();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "namespaces")
                .unwrap()
                .imported,
            2
        );
        let conn = db.conn.lock().unwrap();
        let mut query = conn.prepare("SELECT model, SUM(CAST(total_cost_usd AS REAL)) FROM proxy_request_logs GROUP BY model ORDER BY model").unwrap();
        let costs = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            costs,
            vec![("another/model".into(), 0.5), ("vendor/model".into(), 0.25)]
        );
    }

    #[test]
    fn mcode_usage_preserves_native_zero_cost_and_does_not_reimport_pruned_rows() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source.execute_batch("CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES (1,'s1','custom_provider:test/model',100000,10,20,3,40,5,0,'t9');").unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let values=conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,total_cost_usd FROM proxy_request_logs WHERE app_type='mcode'",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?))).unwrap();
            // native 0 视为未提供，无定价可命中时落 '0'
            assert_eq!(values, (10, 23, 40, 5, "0".into()));
            let semantics: i64 = conn
                .query_row(
                    "SELECT input_token_semantics FROM proxy_request_logs WHERE app_type='mcode'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_FRESH);
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            0
        );
        source.execute_batch("INSERT INTO local_runtime_token_usage VALUES (2,'s1','custom_provider:test/model',101000,1,2,0,0,0,0.25,'t9')").unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "mcode-test")
                .unwrap()
                .imported,
            1
        );
    }

    /// 构造带 message_rows / sessions 的源库，验证四级模型回退与首行耗时携带。
    #[test]
    fn model_fallback_chain_and_first_row_latency() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES
             (1,'s1',NULL,100000,100,200,0,0,0,0,'t1'),
             (2,'s1',NULL,101000,10,20,0,0,0,0,'t1'),
             (3,'s1',NULL,102000,1,2,0,0,0,0,'t2'),
             (4,'s2',NULL,103000,5,6,0,0,0,0,'t3');
            CREATE TABLE local_runtime_message_rows (
            id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,msg_id TEXT,role TEXT,
            turn_id TEXT,created_at_ms INTEGER,data_json TEXT);
            INSERT INTO local_runtime_message_rows (session_id,msg_id,role,turn_id,created_at_ms,data_json) VALUES
             ('s1','m1','assistant','t1',100000,'{\"usage\":{\"input\":1,\"output\":2,\"request_duration_ms\":8500},\"thinking_duration_ms\":3200,\"context_usage_telemetry\":{\"model\":\"MiniMax-M3\"}}'),
             ('s1','m2','assistant','t2',101000,'{\"usage\":{\"input\":1,\"output\":2}}');
            CREATE TABLE local_runtime_sessions (
            session_id TEXT PRIMARY KEY,record_json TEXT NOT NULL,updated_at_ms INTEGER NOT NULL,
            extra_data_json TEXT NOT NULL DEFAULT '{}');
            INSERT INTO local_runtime_sessions (session_id,record_json,updated_at_ms,extra_data_json) VALUES
             ('s1','{}',1,'{\"effectiveModel\":\"minimax/MiniMax-M2.7-highspeed\"}');",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "fallback")
                .unwrap()
                .imported,
            4
        );
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT model, latency_ms, first_token_ms FROM proxy_request_logs
                 ORDER BY request_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        // t1 遥测（请求级）优先于会话 effectiveModel；同 turn 仅首行携带耗时
        assert_eq!(rows[0], ("MiniMax-M3".into(), 8500, Some(3200)));
        assert_eq!(rows[1], ("MiniMax-M3".into(), 0, Some(0)));
        // t2 无遥测 → 回退会话 effectiveModel（剥离 minimax/ 前缀）
        assert_eq!(rows[2], ("MiniMax-M2.7-highspeed".into(), 0, Some(0)));
        // s2 无任何信息 → unknown
        assert_eq!(rows[3], ("unknown".into(), 0, Some(0)));
    }

    /// 存量 unknown 行回填：补模型、命中定价时重算成本，且整个流程幂等。
    #[test]
    fn backfills_existing_unknown_rows_with_pricing_and_is_idempotent() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES
             (1,'s1',NULL,100000,1000,200,0,0,0,0,'t1');
            CREATE TABLE local_runtime_message_rows (
            id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,msg_id TEXT,role TEXT,
            turn_id TEXT,created_at_ms INTEGER,data_json TEXT);
            INSERT INTO local_runtime_message_rows (session_id,msg_id,role,turn_id,created_at_ms,data_json) VALUES
             ('s1','m1','assistant','t1',100000,'{\"usage\":{\"request_duration_ms\":4200},\"context_usage_telemetry\":{\"model\":\"MiniMax-M3\"}}');",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        // 第一轮：模型补全 + 内置定价命中（minimax-m3），成本为计算值
        assert_eq!(
            sync_from_database(&db, &source, "backfill")
                .unwrap()
                .imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let (model, cost): (String, String) = conn
                .query_row(
                    "SELECT model, total_cost_usd FROM proxy_request_logs
                     WHERE data_source = 'mcode_session'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(model, "MiniMax-M3");
            assert!(
                cost.parse::<f64>().unwrap() > 0.0,
                "内置定价应命中，实际 {cost}"
            );
        }
        // 模拟"旧版本入库的 unknown 存量行"
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE proxy_request_logs SET model = 'unknown', request_model = 'unknown',
                        total_cost_usd = '0', latency_ms = 0",
                [],
            )
            .unwrap();
        }
        // 第二轮：回填选中存量行，模型/耗时/成本一并修复
        assert_eq!(
            sync_from_database(&db, &source, "backfill")
                .unwrap()
                .imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let (model, cost, latency, first_token): (String, String, i64, i64) = conn
                .query_row(
                    "SELECT model, total_cost_usd, latency_ms, COALESCE(first_token_ms, 0)
                     FROM proxy_request_logs WHERE data_source = 'mcode_session'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap();
            assert_eq!(model, "MiniMax-M3");
            assert_eq!(latency, 4200);
            assert_eq!(first_token, 0);
            let cost_val: f64 = cost.parse().unwrap();
            assert!(cost_val > 0.0, "命中定价后成本应重算，实际 {cost}");
        }
        // 第三轮：模型已非 unknown，回填集合为空，幂等
        assert_eq!(
            sync_from_database(&db, &source, "backfill")
                .unwrap()
                .imported,
            0
        );
    }

    /// 同 turn 分片跨同步轮次导入时，turn 级耗时仍只归属全源库最小 id 的行，
    /// 与一次性导入结果一致，SUM(latency_ms) 不随同步时机变化。
    #[test]
    fn turn_latency_owner_stable_across_sync_rounds() {
        let source = rusqlite::Connection::open_in_memory().unwrap();
        source
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES
             (1,'s1',NULL,100000,100,200,0,0,0,0,'t1');
            CREATE TABLE local_runtime_message_rows (
            id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,msg_id TEXT,role TEXT,
            turn_id TEXT,created_at_ms INTEGER,data_json TEXT);
            INSERT INTO local_runtime_message_rows (session_id,msg_id,role,turn_id,created_at_ms,data_json) VALUES
             ('s1','m1','assistant','t1',100000,'{\"usage\":{\"request_duration_ms\":8500},\"thinking_duration_ms\":3200,\"context_usage_telemetry\":{\"model\":\"MiniMax-M3\"}}');",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        // 第一轮：仅导入 id=1，携带 turn 耗时
        assert_eq!(
            sync_from_database(&db, &source, "rounds").unwrap().imported,
            1
        );
        // 第二轮：追加同 turn 的 id=2，耗时归属仍由全源库 MIN(id)=1 决定
        source
            .execute(
                "INSERT INTO local_runtime_token_usage VALUES
                 (2,'s1',NULL,101000,10,20,0,0,0,0,'t1')",
                [],
            )
            .unwrap();
        assert_eq!(
            sync_from_database(&db, &source, "rounds").unwrap().imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, latency_ms, COALESCE(first_token_ms, 0)
                 FROM proxy_request_logs WHERE data_source = 'mcode_session' ORDER BY request_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("mcode:s1:1".into(), 8500, 3200),
                ("mcode:s1:2".into(), 0, 0),
            ]
        );
    }

    /// 切换数据目录后，新源库复用相同数字 id 但 session 不同的行必须正常 INSERT，
    /// 不能被旧库的 unknown 回填集合误判为回填行而静默漏记。
    #[test]
    fn backfill_matches_full_request_identity_across_sources() {
        // 源库 A：id=1 / session-A / model NULL，无遥测与会话表 → 入库 unknown
        let source_a = rusqlite::Connection::open_in_memory().unwrap();
        source_a
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES (1,'session-A',NULL,100000,1,2,0,0,0,0,NULL);",
            )
            .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_database(&db, &source_a, "mcode:path-a")
                .unwrap()
                .imported,
            1
        );
        // 源库 B：复用 id=1 但 session 不同，行内自带模型
        let source_b = rusqlite::Connection::open_in_memory().unwrap();
        source_b
            .execute_batch(
                "CREATE TABLE local_runtime_token_usage (
            id INTEGER PRIMARY KEY,session_id TEXT,model TEXT,ts INTEGER,input_tokens INTEGER,
            output_tokens INTEGER,reasoning_tokens INTEGER,cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,cost_usd REAL,turn_id TEXT);
            INSERT INTO local_runtime_token_usage VALUES (1,'session-B','minimax/MiniMax-M3',100000,5,6,0,0,0,0,NULL);",
            )
            .unwrap();
        let result = sync_from_database(&db, &source_b, "mcode:path-b").unwrap();
        assert_eq!(result.imported, 1, "新库行必须 INSERT 而非漏记");
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, model FROM proxy_request_logs
                 WHERE data_source = 'mcode_session' ORDER BY request_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("mcode:session-A:1".into(), "unknown".into()),
                ("mcode:session-B:1".into(), "MiniMax-M3".into()),
            ]
        );
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;
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
