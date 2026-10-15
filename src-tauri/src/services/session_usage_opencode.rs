//! OpenCode 会话日志使用追踪
//!
//! 从 ~/.local/share/opencode/opencode.db (SQLite) 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.local/share/opencode/opencode.db
//!   → session / session_v2 表获取所有会话（按存储布局选择）
//!   → message / session_message 表获取 assistant 消息
//!   → 解析 data JSON 提取 tokens/cost/model
//!   → proxy_request_logs 表
//! ```
//!
//! OpenCode V2 把会话数据迁移到 `session_v2` / `session_message` 并冻结 V1
//! 表。用户继续使用 1.x 时仍会写入 V1，因此两套表共存时合并读取。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::fs;
use std::time::SystemTime;

/// 从 opencode message.data JSON 中提取的 token 和费用数据
struct OpenCodeMessageData {
    input_tokens: u32,
    output_tokens: u32,
    reasoning_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    cost: f64,
    model_id: String,
    timestamp_ms: i64,
    /// time.completed - time.created；离线回填只有整条消息的起止时间，
    /// 没有首字时间，所以 latency 有值而 first_token_ms 恒为空
    latency_ms: i64,
}

struct OpenCodeMessageQueryResult {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

/// OpenCode 数据库的存储布局。
///
/// V1 使用 `session` / `message` 表；V2（OpenCode 2.x）把会话数据迁移到
/// `session_v2` / `session_message`。迁移后继续运行 1.x 时，V1 也可能更新。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenCodeSchema {
    V1,
    V2,
    Mixed,
}

/// 两套完整的会话/消息表共存时同时读取，不能把残留 V2 表当作活跃版本。
fn detect_schema(conn: &rusqlite::Connection) -> Result<OpenCodeSchema, AppError> {
    if sqlite_table_exists(conn, "session_v2")? && sqlite_table_exists(conn, "session_message")? {
        if sqlite_table_exists(conn, "session")? && sqlite_table_exists(conn, "message")? {
            Ok(OpenCodeSchema::Mixed)
        } else {
            Ok(OpenCodeSchema::V2)
        }
    } else {
        Ok(OpenCodeSchema::V1)
    }
}

fn sqlite_table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询表 {table} 失败: {error}")))
}

/// 同步 OpenCode 使用数据
pub fn sync_opencode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_opencode_usage_from_path(db, &get_opencode_db_path())
}

fn sync_opencode_usage_from_path(
    db: &Database,
    db_path: &std::path::Path,
) -> Result<SessionSyncResult, AppError> {
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

    // 检查文件修改时间。
    // opencode 的数据库运行在 WAL 模式：新提交先落在 -wal 文件里，
    // 主库文件只有在 checkpoint 时才更新。因此必须同时考虑 -wal 的
    // mtime，否则会在 checkpoint 之前漏掉刚写入的会话。
    let metadata = fs::metadata(db_path)
        .map_err(|e| AppError::Config(format!("无法读取 opencode.db 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    let wal_path = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    // 打开 opencode 的 SQLite 数据库（只读）
    let opencode_conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?;
    let schema = detect_schema(&opencode_conn)?;
    // 旧版可能在漏读 V1 后推进了文件游标。混合库首次补扫时只切换文件级
    // 游标，保留会话级游标，避免重放已经归档并删除明细的历史会话。
    let file_sync_key = if schema == OpenCodeSchema::Mixed {
        format!("{db_path_str}:mixed-v1-v2")
    } else {
        db_path_str.clone()
    };
    let cursors = crate::services::session_usage::load_sync_cursors(db)?;
    let last_modified = cursors.get(&file_sync_key).map_or(0, |c| c.last_modified);

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

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };
    let mut has_sync_errors = false;

    // 查询所有会话
    let sessions = query_sessions(&opencode_conn, schema)?;

    for (session_id, time_updated) in &sessions {
        // 检查会话是否需要重新同步
        let sync_key = format!("{db_path_str}:{session_id}");
        let sess_last_modified = cursors.get(&sync_key).map_or(0, |c| c.last_modified);
        if *time_updated <= sess_last_modified {
            continue; // 会话未更新，跳过
        }

        let mut session_had_error = false;

        // 查询该会话的所有 assistant 消息
        let mut session_has_incomplete_usage = false;
        match query_assistant_messages(&opencode_conn, schema, session_id) {
            Ok(query_result) => {
                session_has_incomplete_usage = query_result.has_incomplete_usage;
                for (message_id, msg_data) in &query_result.messages {
                    let request_id = format!("opencode_session:{session_id}:{message_id}");

                    match insert_opencode_message(db, &request_id, msg_data, session_id) {
                        Ok(true) => result.imported += 1,
                        Ok(false) => result.skipped += 1,
                        Err(e) => {
                            let msg = format!("OpenCode 消息插入失败 {request_id}: {e}");
                            log::warn!("[OPENCODE-SYNC] {msg}");
                            result.errors.push(msg);
                            result.skipped += 1;
                            session_had_error = true;
                        }
                    }
                }
            }
            Err(e) => {
                let msg = format!("OpenCode 会话消息查询失败 {session_id}: {e}");
                log::warn!("[OPENCODE-SYNC] {msg}");
                result.errors.push(msg);
                session_had_error = true;
            }
        }

        if session_had_error {
            has_sync_errors = true;
            continue;
        }

        if session_has_incomplete_usage {
            continue;
        }

        // 更新会话级同步状态。失败时不要推进文件级状态，确保下次可重试。
        if let Err(e) = update_sync_state(db, &sync_key, *time_updated, 0) {
            let msg = format!("OpenCode 会话同步状态更新失败 {session_id}: {e}");
            log::warn!("[OPENCODE-SYNC] {msg}");
            result.errors.push(msg);
            has_sync_errors = true;
        }
    }

    // 仅在本轮完全成功时推进文件级状态；否则保留下次重试入口。
    if !has_sync_errors {
        update_sync_state(db, &file_sync_key, file_modified, 0)?;
    }

    if result.imported > 0 {
        log::info!(
            "[OPENCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个会话",
            result.imported,
            result.skipped,
            sessions.len()
        );
    }

    Ok(result)
}

/// 查询所有会话的 (id, sync_watermark)
fn query_sessions(
    conn: &rusqlite::Connection,
    schema: OpenCodeSchema,
) -> Result<Vec<(String, i64)>, AppError> {
    let tables: &[(&str, &str)] = match schema {
        OpenCodeSchema::V1 => &[("session", "message")],
        OpenCodeSchema::V2 => &[("session_v2", "session_message")],
        OpenCodeSchema::Mixed => &[("session_v2", "session_message"), ("session", "message")],
    };
    let queries = tables
        .iter()
        .map(|(sessions_table, messages_table)| {
            format!(
                "SELECT s.id,
                MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
         FROM {sessions_table} s
         LEFT JOIN {messages_table} m ON m.session_id = s.id
         GROUP BY s.id"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    // 同一会话可能在迁移后由两个版本继续写入，任一侧消息更新都需要扫描。
    let sql = format!(
        "SELECT id, MAX(sync_watermark) AS sync_watermark FROM ({queries})
         GROUP BY id ORDER BY sync_watermark, id"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AppError::Database(format!("准备会话查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询会话失败: {e}")))?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|e| AppError::Database(format!("读取会话行失败: {e}")))?);
    }

    Ok(sessions)
}

/// 查询某会话的已完成计费消息（V1: assistant，V2: assistant + compaction），
/// 并标记是否还有未完成 usage 消息。
fn query_assistant_messages(
    conn: &rusqlite::Connection,
    schema: OpenCodeSchema,
    session_id: &str,
) -> Result<OpenCodeMessageQueryResult, AppError> {
    // V2 的消息类型记录在 `type` 列上，上下文压缩请求的用量写在
    // type='compaction' 行（V1 中压缩摘要是 assistant 消息，无需特判）；
    // V1 需要按 data.role 过滤。
    let sql = match schema {
        OpenCodeSchema::V1 => {
            "SELECT id, data, NULL FROM message WHERE session_id = ?1 ORDER BY time_created"
        }
        OpenCodeSchema::V2 => {
            "SELECT id, data, type FROM session_message \
             WHERE session_id = ?1 AND type IN ('assistant', 'compaction') \
             ORDER BY time_created"
        }
        OpenCodeSchema::Mixed => {
            // 消息 ID 在迁移时保持不变。先读更新时间最新的副本，时间相同
            // 时优先 V2；去重必须在完成状态判断之前，防止导入过期快照。
            "SELECT id, data, type FROM (
                 SELECT id, data, type, time_updated, 0 AS layout_priority
                 FROM session_message
                 WHERE session_id = ?1 AND type IN ('assistant', 'compaction')
                 UNION ALL
                 SELECT id, data, NULL AS type, time_updated, 1 AS layout_priority
                 FROM message WHERE session_id = ?1
             ) ORDER BY time_updated DESC, layout_priority, id"
        }
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Database(format!("准备消息查询失败: {e}")))?;

    let rows = stmt
        .query_map([session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| AppError::Database(format!("查询消息失败: {e}")))?;

    let mut messages = Vec::new();
    let mut has_incomplete_usage = false;
    let mut seen = HashSet::new();
    for row in rows {
        let (message_id, data_json, message_type) =
            row.map_err(|e| AppError::Database(format!("读取消息行失败: {e}")))?;
        if !seen.insert(message_id.clone()) {
            continue;
        }

        // V2 的 assistant 消息没有 role 字段（由 `type` 列标识），
        // V1 消息仍按 data.role 过滤。
        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if message_type.is_none() && value.get("role").and_then(|r| r.as_str()) != Some("assistant")
        {
            continue;
        }

        // 必须有 tokens 字段
        if value.get("tokens").is_none() {
            continue;
        }

        // 跳过未完成的消息：进行中只有半截 token，且因 INSERT OR IGNORE 无法回填。
        // assistant 用 time.completed 判终态；V2 compaction 用 status 终态
        // （completed/failed），没有 time.completed 字段。
        let is_completed = if message_type.as_deref() == Some("compaction") {
            matches!(
                value.get("status").and_then(|s| s.as_str()),
                Some("completed") | Some("failed")
            )
        } else {
            value.get("time").and_then(|t| t.get("completed")).is_some()
        };
        if !is_completed {
            has_incomplete_usage = true;
            continue;
        }

        if let Some(msg_data) = parse_message_data(&value) {
            messages.push((message_id, msg_data));
        }
    }

    Ok(OpenCodeMessageQueryResult {
        messages,
        has_incomplete_usage,
    })
}

/// 解析 opencode message.data JSON 为结构化数据
fn parse_message_data(value: &serde_json::Value) -> Option<OpenCodeMessageData> {
    let tokens = value.get("tokens")?;

    let input_tokens = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output_tokens = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let reasoning_tokens = tokens
        .get("reasoning")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let cache_obj = tokens.get("cache");
    let cache_read_tokens = cache_obj
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_write_tokens = cache_obj
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // 跳过全零 token 的消息
    if input_tokens == 0
        && output_tokens == 0
        && reasoning_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
    {
        return None;
    }

    let cost = value.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);

    // V1 把模型 ID 放在 `modelID` 字符串里；V2 的 `model` 是
    // `{ id, providerID, variant }` 对象（也兼容纯字符串形态）。
    let model_id = value
        .get("modelID")
        .and_then(|v| v.as_str())
        .or_else(|| match value.get("model") {
            Some(serde_json::Value::String(model)) => Some(model.as_str()),
            Some(model) => model.get("id").and_then(|v| v.as_str()),
            None => None,
        })
        .unwrap_or("unknown")
        .to_string();

    let timestamp_ms = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    // 调用方已过滤掉没有 time.completed 的未完成消息；这里再兜底：
    // created 缺失（timestamp_ms 回退为 0，差值会变成毫秒纪元值）或
    // 时钟倒挂（completed 不晚于 created）时按 0 处理
    let latency_ms = match value
        .get("time")
        .and_then(|t| t.get("completed"))
        .and_then(|v| v.as_i64())
    {
        Some(completed) if timestamp_ms > 0 && completed > timestamp_ms => completed - timestamp_ms,
        _ => 0,
    };

    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost,
        model_id,
        timestamp_ms,
        latency_ms,
    })
}

/// 插入单条 OpenCode 消息记录到 proxy_request_logs
fn insert_opencode_message(
    db: &Database,
    request_id: &str,
    msg: &OpenCodeMessageData,
    session_id: &str,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let created_at = if msg.timestamp_ms > 0 {
        msg.timestamp_ms / 1000
    } else {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };

    // OpenCode 使用 Anthropic 风格：input 是新鲜输入，cache 单独计
    // output 包含 reasoning tokens（按输出计费）
    let output_with_reasoning = msg.output_tokens + msg.reasoning_tokens;

    let dedup_key = DedupKey {
        app_type: "opencode",
        model: &msg.model_id,
        input_tokens: msg.input_tokens,
        output_tokens: output_with_reasoning,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_write_tokens,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    // 如果 opencode 已经提供了费用，直接使用；否则从模型定价计算
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        if msg.cost > 0.0 {
            // opencode 已计算费用，直接使用
            // 简化处理：全部放入 total_cost（opencode 的 cost 是聚合值，无法精确拆分）
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                msg.cost.to_string(),
            )
        } else {
            // opencode 费用为 0（如免费模型），尝试用 cc-switch 自带的模型定价计算
            let usage = TokenUsage {
                input_tokens: msg.input_tokens,
                output_tokens: output_with_reasoning,
                cache_read_tokens: msg.cache_read_tokens,
                cache_creation_tokens: msg.cache_write_tokens,
                cache_creation_1h_tokens: 0,
                model: Some(msg.model_id.clone()),
                message_id: None,
            };

            match find_model_pricing(&conn, &msg.model_id) {
                Some(pricing) => {
                    let cost = CostCalculator::calculate_for_app(
                        "opencode",
                        &usage,
                        &pricing,
                        Decimal::from(1),
                    );
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
        };

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
            "_opencode_session",   // provider_id
            "opencode",            // app_type
            msg.model_id,
            msg.model_id,          // request_model = model
            msg.input_tokens,
            output_with_reasoning,
            msg.cache_read_tokens,
            msg.cache_write_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            msg.latency_ms,
            // opencode.db 里没有首字时间字段，离线回填无法测量 first_token_ms
            Option::<i64>::None,   // first_token_ms
            200i64,                // status_code
            Option::<String>::None,// error_message
            Some(session_id.to_string()),
            Some("opencode_session"), // provider_type
            1i64,                  // is_streaming
            "1.0",                 // cost_multiplier
            created_at,
            "opencode_session",    // data_source
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 OpenCode 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed_database() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, time_updated INTEGER);
             CREATE TABLE message (
                 id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                 time_updated INTEGER, data TEXT
             );
             CREATE TABLE session_v2 (id TEXT PRIMARY KEY, time_updated INTEGER);
             CREATE TABLE session_message (
                 id TEXT PRIMARY KEY, session_id TEXT, type TEXT,
                 time_created INTEGER, time_updated INTEGER, data TEXT
             );",
        )
        .unwrap();
        conn
    }

    fn usage_message(input: u32, completed: bool) -> String {
        let mut value = serde_json::json!({
            "role": "assistant", "modelID": "test-model", "cost": 0.01,
            "tokens": { "input": input, "output": 10 },
            "time": { "created": 1000 }
        });
        if completed {
            value["time"]["completed"] = serde_json::json!(2000);
        }
        value.to_string()
    }

    #[test]
    fn test_mixed_sessions_merge_both_message_watermarks() {
        let conn = mixed_database();
        conn.execute_batch(
            "INSERT INTO session VALUES ('shared', 100), ('v1-only', 100);
             INSERT INTO session_v2 VALUES ('shared', 200), ('v2-only', 200);
             INSERT INTO message VALUES ('v1-new', 'shared', 100, 400, '{}');
             INSERT INTO session_message VALUES ('v2-new', 'shared', 'assistant', 200, 300, '{}');",
        )
        .unwrap();
        let schema = detect_schema(&conn).unwrap();
        let sessions = query_sessions(&conn, schema).unwrap();
        assert_eq!(
            sessions,
            vec![
                ("v1-only".to_string(), 100),
                ("v2-only".to_string(), 200),
                ("shared".to_string(), 400),
            ]
        );
    }

    #[test]
    fn test_mixed_messages_use_latest_copy_and_keep_both_branches() {
        let conn = mixed_database();
        for (id, updated, completed) in [("shared", 400, true), ("v1-only", 500, true)] {
            conn.execute(
                "INSERT INTO message VALUES (?1, 's1', 100, ?2, ?3)",
                rusqlite::params![id, updated, usage_message(20, completed)],
            )
            .unwrap();
        }
        // The V2 snapshot of 'shared' was incomplete before 1.x resumed it.
        for (id, updated, completed) in [("shared", 200, false), ("v2-only", 300, true)] {
            conn.execute(
                "INSERT INTO session_message VALUES (?1, 's1', 'assistant', 100, ?2, ?3)",
                rusqlite::params![id, updated, usage_message(30, completed)],
            )
            .unwrap();
        }
        let result = query_assistant_messages(&conn, detect_schema(&conn).unwrap(), "s1").unwrap();
        let messages: std::collections::BTreeMap<_, _> = result
            .messages
            .iter()
            .map(|(id, data)| (id.as_str(), data.input_tokens))
            .collect();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages["shared"], 20);
        assert_eq!(messages["v1-only"], 20);
        assert_eq!(messages["v2-only"], 30);
        assert!(!result.has_incomplete_usage);
    }

    #[test]
    fn test_mixed_messages_do_not_import_stale_completed_copy() {
        let conn = mixed_database();
        conn.execute(
            "INSERT INTO message VALUES ('m1', 's1', 100, 300, ?1)",
            [usage_message(20, false)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message VALUES ('m1', 's1', 'assistant', 100, 200, ?1)",
            [usage_message(10, true)],
        )
        .unwrap();
        let result = query_assistant_messages(&conn, detect_schema(&conn).unwrap(), "s1").unwrap();
        assert!(result.messages.is_empty());
        assert!(result.has_incomplete_usage);
    }

    #[test]
    fn test_mixed_messages_prefer_v2_on_ties_and_filter_non_usage_rows() {
        let conn = mixed_database();
        conn.execute(
            "INSERT INTO message VALUES ('m1', 's1', 100, 200, ?1),
                                        ('u1', 's1', 100, 200, ?2)",
            rusqlite::params![
                usage_message(10, true),
                serde_json::json!({"role": "user", "tokens": {"input": 999}}).to_string()
            ],
        )
        .unwrap();
        let compaction = serde_json::json!({
            "status": "completed", "tokens": {"input": 50, "output": 5},
            "model": {"id": "test-model"}, "time": {"created": 100}
        });
        conn.execute(
            "INSERT INTO session_message VALUES
                 ('m1', 's1', 'assistant', 100, 200, ?1),
                 ('c1', 's1', 'compaction', 100, 200, ?2),
                 ('u2', 's1', 'user', 100, 200, ?1)",
            rusqlite::params![usage_message(20, true), compaction.to_string()],
        )
        .unwrap();
        let result = query_assistant_messages(&conn, detect_schema(&conn).unwrap(), "s1").unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].0, "c1");
        assert_eq!(result.messages[0].1.input_tokens, 50);
        assert_eq!(result.messages[1].0, "m1");
        assert_eq!(result.messages[1].1.input_tokens, 20);
        assert!(!result.has_incomplete_usage);
    }

    #[test]
    fn test_mixed_sync_backfills_unchanged_database_without_resetting_session_cursors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        let source = mixed_database();
        source
            .execute_batch(
                "INSERT INTO session VALUES ('shared', 300), ('v1-only', 400);
             INSERT INTO session_v2 VALUES ('shared', 100), ('archived', 100);",
            )
            .unwrap();
        for (id, sid, updated) in [
            ("migrated", "shared", 100),
            ("continued", "shared", 300),
            ("new", "v1-only", 400),
        ] {
            source
                .execute(
                    "INSERT INTO message VALUES (?1, ?2, 100, ?3, ?4)",
                    rusqlite::params![id, sid, updated, usage_message(20, true)],
                )
                .unwrap();
        }
        for (id, sid) in [("migrated", "shared"), ("old", "archived")] {
            source
                .execute(
                    "INSERT INTO session_message VALUES (?1, ?2, 'assistant', 100, 100, ?3)",
                    rusqlite::params![id, sid, usage_message(20, true)],
                )
                .unwrap();
        }
        source
            .backup(rusqlite::DatabaseName::Main, &path, None)
            .unwrap();
        let db = Database::memory().unwrap();
        let base = path.to_string_lossy();
        // Simulate a successful V2-only scan: the file cursor is current, but
        // the continued/shared and V1-only usage was never imported.
        update_sync_state(
            &db,
            &base,
            metadata_modified_nanos(&fs::metadata(&path).unwrap()),
            0,
        )
        .unwrap();
        update_sync_state(&db, &format!("{base}:shared"), 100, 0).unwrap();
        update_sync_state(&db, &format!("{base}:archived"), 100, 0).unwrap();
        let migrated =
            parse_message_data(&serde_json::from_str(&usage_message(20, true)).unwrap()).unwrap();
        insert_opencode_message(&db, "opencode_session:shared:migrated", &migrated, "shared")
            .unwrap();

        let result = sync_opencode_usage_from_path(&db, &path).unwrap();
        assert_eq!(result.imported, 2);
        assert!(result.errors.is_empty());
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM proxy_request_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 3,
            "migrated message is counted once; archived session stays skipped"
        );
        drop(conn);
        assert_eq!(
            sync_opencode_usage_from_path(&db, &path).unwrap().imported,
            0
        );
    }

    #[test]
    fn test_parse_message_data_full() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "cost": 0.0023113,
            "tokens": {
                "total": 56554,
                "input": 3272,
                "output": 383,
                "reasoning": 419,
                "cache": {
                    "write": 0,
                    "read": 52480
                }
            },
            "modelID": "deepseek-v4-pro",
            "providerID": "deepseek",
            "time": {
                "created": 1779755333700i64,
                "completed": 1779755350639i64
            }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 3272);
        assert_eq!(data.output_tokens, 383);
        assert_eq!(data.reasoning_tokens, 419);
        assert_eq!(data.cache_read_tokens, 52480);
        assert_eq!(data.cache_write_tokens, 0);
        assert!((data.cost - 0.0023113).abs() < 1e-10);
        assert_eq!(data.model_id, "deepseek-v4-pro");
        assert_eq!(data.timestamp_ms, 1779755333700);
        assert_eq!(data.latency_ms, 1779755350639 - 1779755333700);
    }

    #[test]
    fn test_parse_message_data_latency_fallbacks() {
        // 没有 time.completed：latency 回退为 0
        let no_completed = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "created": 1000i64 }
        });
        let data = parse_message_data(&no_completed).unwrap();
        assert_eq!(data.latency_ms, 0);

        // completed 早于 created：兜底为 0，不出负数
        let negative = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "created": 2000i64, "completed": 1000i64 }
        });
        let data = parse_message_data(&negative).unwrap();
        assert_eq!(data.latency_ms, 0);

        // 有 completed 但缺 created：差值不能拿毫秒纪元值来算
        let no_created = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "completed": 1779755350639i64 }
        });
        let data = parse_message_data(&no_created).unwrap();
        assert_eq!(data.timestamp_ms, 0);
        assert_eq!(data.latency_ms, 0);
    }

    #[test]
    fn test_parse_message_data_missing_cache() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "cost": 0.0,
            "tokens": {
                "input": 1000,
                "output": 200
            },
            "modelID": "mimo-v2.5-pro",
            "time": { "created": 1779755333700i64 }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 1000);
        assert_eq!(data.output_tokens, 200);
        assert_eq!(data.reasoning_tokens, 0);
        assert_eq!(data.cache_read_tokens, 0);
        assert_eq!(data.cache_write_tokens, 0);
        // 未完成消息（无 time.completed）没有可用耗时
        assert_eq!(data.latency_ms, 0);
    }

    #[test]
    fn test_parse_message_data_skips_zero_tokens() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": {
                "input": 0,
                "output": 0,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "modelID": "test"
        });
        assert!(parse_message_data(&json).is_none());
    }

    #[test]
    fn test_parse_message_data_ignores_role() {
        // parse_message_data does not filter by role; that's the caller's job
        let json: serde_json::Value = serde_json::json!({
            "role": "user",
            "tokens": { "input": 100, "output": 0 }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 100);
    }

    #[test]
    fn test_query_assistant_messages_skips_incomplete() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();

        let done = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "created": 1, "completed": 2 }
        })
        .to_string();
        let in_progress = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 500, "output": 0 },
            "modelID": "m",
            "time": { "created": 3 }
        })
        .to_string();

        conn.execute(
            "INSERT INTO message VALUES ('done', 's1', 1, ?1), ('wip', 's1', 2, ?2)",
            rusqlite::params![done, in_progress],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, OpenCodeSchema::V1, "s1").unwrap();
        // 只返回已完成（带 time.completed）的消息，半截的被跳过
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].0, "done");
        assert!(result.has_incomplete_usage);
    }

    #[test]
    fn test_query_sessions_uses_message_update_watermark() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT, time_updated INTEGER);
             CREATE TABLE message (
                 id TEXT,
                 session_id TEXT,
                 time_created INTEGER,
                 time_updated INTEGER,
                 data TEXT
             );
             INSERT INTO session VALUES ('s1', 100);
             INSERT INTO message VALUES ('m1', 's1', 90, 200, '{}');",
        )
        .unwrap();

        let sessions = query_sessions(&conn, OpenCodeSchema::V1).unwrap();
        assert_eq!(sessions, vec![("s1".to_string(), 200)]);
    }

    #[test]
    fn test_detect_schema_reads_both_layouts_when_present() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE session (id TEXT); CREATE TABLE message (id TEXT);")
            .unwrap();
        assert_eq!(detect_schema(&conn).unwrap(), OpenCodeSchema::V1);

        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT); CREATE TABLE session_message (id TEXT);",
        )
        .unwrap();
        assert_eq!(detect_schema(&conn).unwrap(), OpenCodeSchema::Mixed);
        conn.execute_batch("DROP TABLE session; DROP TABLE message;")
            .unwrap();
        assert_eq!(detect_schema(&conn).unwrap(), OpenCodeSchema::V2);
    }

    #[test]
    fn test_parse_message_data_v2_model_object() {
        // V2 的 assistant 消息没有 role 字段，model 是对象
        let json: serde_json::Value = serde_json::json!({
            "agent": "build",
            "cost": 0.00143667,
            "tokens": {
                "input": 1373,
                "output": 96,
                "reasoning": 0,
                "cache": { "read": 391040, "write": 0 }
            },
            "model": { "id": "deepseek-v4.1-flash", "providerID": "opencode-go", "variant": "max" },
            "time": {
                "created": 1789945285532i64,
                "streamed": 1789945288379i64,
                "completed": 1789945288526i64
            }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.model_id, "deepseek-v4.1-flash");
        assert_eq!(data.input_tokens, 1373);
        assert_eq!(data.cache_read_tokens, 391040);
        assert_eq!(data.timestamp_ms, 1789945285532);
    }

    #[test]
    fn test_parse_message_data_v2_model_string() {
        // 兼容 model 为纯字符串的形态
        let json: serde_json::Value = serde_json::json!({
            "tokens": { "input": 10, "output": 1 },
            "model": "some-model"
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.model_id, "some-model");
    }

    #[test]
    fn test_query_sessions_v2_uses_message_update_watermark() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT, time_updated INTEGER);
             CREATE TABLE session_message (
                 id TEXT,
                 session_id TEXT,
                 type TEXT,
                 time_created INTEGER,
                 time_updated INTEGER,
                 data TEXT
             );
             INSERT INTO session_v2 VALUES ('s1', 100);
             INSERT INTO session_message VALUES ('m1', 's1', 'assistant', 90, 200, '{}');",
        )
        .unwrap();

        let sessions = query_sessions(&conn, OpenCodeSchema::V2).unwrap();
        assert_eq!(sessions, vec![("s1".to_string(), 200)]);
    }

    #[test]
    fn test_query_assistant_messages_v2_filters_by_type() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_message (
                 id TEXT,
                 session_id TEXT,
                 type TEXT,
                 time_created INTEGER,
                 data TEXT
             );",
        )
        .unwrap();

        let done = serde_json::json!({
            "tokens": { "input": 1000, "output": 200 },
            "model": { "id": "m" },
            "time": { "created": 1, "completed": 2 }
        })
        .to_string();
        let in_progress = serde_json::json!({
            "tokens": { "input": 500, "output": 0 },
            "model": { "id": "m" },
            "time": { "created": 3 }
        })
        .to_string();
        let user = serde_json::json!({
            "tokens": { "input": 1, "output": 0 }
        })
        .to_string();

        conn.execute(
            "INSERT INTO session_message VALUES ('done', 's1', 'assistant', 1, ?1),
                                                ('wip', 's1', 'assistant', 2, ?2),
                                                ('u1', 's1', 'user', 3, ?3)",
            rusqlite::params![done, in_progress, user],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, OpenCodeSchema::V2, "s1").unwrap();
        // 只返回已完成（带 time.completed）的 assistant 消息；user 行被 type 过滤
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].0, "done");
        assert!(result.has_incomplete_usage);
    }

    #[test]
    fn test_query_assistant_messages_v2_includes_completed_compaction() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_message (
                 id TEXT,
                 session_id TEXT,
                 type TEXT,
                 time_created INTEGER,
                 data TEXT
             );",
        )
        .unwrap();

        // V2 compaction：终态由 status 表示，没有 time.completed
        let compaction_done = serde_json::json!({
            "status": "completed",
            "tokens": { "input": 741, "output": 2604 },
            "cost": 0.004736382,
            "model": { "id": "deepseek-v4.1-flash" },
            "time": { "created": 10 }
        })
        .to_string();
        let compaction_wip = serde_json::json!({
            "status": "pending",
            "tokens": { "input": 100, "output": 0 },
            "model": { "id": "deepseek-v4.1-flash" },
            "time": { "created": 11 }
        })
        .to_string();
        // 失败的压缩请求同样产生了计费用量，也要导入
        let compaction_failed = serde_json::json!({
            "status": "failed",
            "tokens": { "input": 300, "output": 0 },
            "cost": 0.001,
            "model": { "id": "deepseek-v4.1-flash" },
            "time": { "created": 12 }
        })
        .to_string();
        let assistant = serde_json::json!({
            "tokens": { "input": 1000, "output": 200 },
            "model": { "id": "m" },
            "time": { "created": 13, "completed": 14 }
        })
        .to_string();

        conn.execute(
            "INSERT INTO session_message VALUES ('c1', 's1', 'compaction', 1, ?1),
                                                ('c2', 's1', 'compaction', 2, ?2),
                                                ('c3', 's1', 'compaction', 3, ?3),
                                                ('a1', 's1', 'assistant', 4, ?4)",
            rusqlite::params![
                compaction_done,
                compaction_wip,
                compaction_failed,
                assistant
            ],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, OpenCodeSchema::V2, "s1").unwrap();
        // compaction(completed/failed) 与 assistant 都要采到；pending 的压缩被跳过
        let ids: Vec<&str> = result.messages.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c3", "a1"]);
        assert_eq!(result.messages[0].1.input_tokens, 741);
        assert_eq!(result.messages[0].1.model_id, "deepseek-v4.1-flash");
        assert_eq!(result.messages[1].1.input_tokens, 300);
        assert!(result.has_incomplete_usage);
    }
}
