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
//! 表，因此检测到 V2 表时优先读取 V2；未迁移的库继续走 V1 布局。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use chrono::{Local, TimeZone};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use std::fs;
use std::path::Path;
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
    /// 耗时终点优先取 time.streamed（流式结束，不含工具执行），缺失或异常时
    /// 退回 time.completed；离线回填只有整条消息的起止时间，没有首字时间，
    /// 所以 latency 有值而 first_token_ms 恒为空
    latency_ms: i64,
}

struct OpenCodeMessageQueryResult {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

/// OpenCode 数据库的存储布局。
///
/// V1 使用 `session` / `message` 表；V2（OpenCode 2.x）把会话数据迁移到
/// `session_v2` / `session_message` 并停止写入 V1 表。迁移后 V1 表仍然存在
/// 但内容冻结，因此检测到 V2 表时必须优先读取 V2。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenCodeSchema {
    V1,
    V2,
}

/// 探测数据库使用的存储布局：`session_v2` 与 `session_message` 同时存在
/// 视为 V2，否则回退 V1。
fn detect_schema(conn: &rusqlite::Connection) -> Result<OpenCodeSchema, AppError> {
    if sqlite_table_exists(conn, "session_v2")? && sqlite_table_exists(conn, "session_message")? {
        Ok(OpenCodeSchema::V2)
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

const OPENCODE_HISTORY_REPAIRED_KEY: &str = "opencode_history_rollups_repaired_v1";

/// 同步 OpenCode 使用数据
pub fn sync_opencode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_opencode_usage_with_path(db, &get_opencode_db_path())
}

pub(crate) fn sync_opencode_usage_with_path(
    db: &Database,
    db_path: &Path,
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

    // 针对死循环导致的被重复累加的历史聚合数据进行一次性自愈校准，并补齐去重账本
    if !db.get_bool_flag(OPENCODE_HISTORY_REPAIRED_KEY)? {
        match repair_opencode_history_rollups(db, db_path) {
            Ok(count) => {
                if count > 0 {
                    log::info!(
                        "[OPENCODE-SYNC] 历史聚合数据自愈完成：成功校准 {count} 条被放大的聚合记录"
                    );
                }
                db.set_setting(OPENCODE_HISTORY_REPAIRED_KEY, "true")?;
            }
            Err(e) => {
                log::warn!("[OPENCODE-SYNC] 历史聚合数据自愈校准失败: {e}");
            }
        }
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

    let cursors = crate::services::session_usage::load_sync_cursors(db)?;
    let last_modified = cursors.get(&db_path_str).map_or(0, |c| c.last_modified);

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

    // 打开 opencode 的 SQLite 数据库（只读）
    let opencode_conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?;

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };
    let mut has_sync_errors = false;

    let schema = detect_schema(&opencode_conn)?;

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

        // 如果存在未完成消息（如流式生成中），且该会话最近（5 分钟内）仍有更新，
        // 则暂时不推进会话游标，等待生成完成；若会话早已静止（> 5 分钟），说明是历史中断或异常悬挂，
        // 应当推进游标避免每次文件更新都无休止地全量重扫该会话（已导入消息已有持久去重账本保护）。
        if session_has_incomplete_usage {
            let now_ms = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if now_ms.saturating_sub(*time_updated) < 300_000 {
                continue;
            }
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
        update_sync_state(db, &db_path_str, file_modified, 0)?;
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
    let sessions_table = match schema {
        OpenCodeSchema::V1 => "session",
        OpenCodeSchema::V2 => "session_v2",
    };
    let messages_table = match schema {
        OpenCodeSchema::V1 => "message",
        OpenCodeSchema::V2 => "session_message",
    };
    let sql = format!(
        "SELECT s.id,
                MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
         FROM {sessions_table} s
         LEFT JOIN {messages_table} m ON m.session_id = s.id
         GROUP BY s.id
         ORDER BY sync_watermark"
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
    for row in rows {
        let (message_id, data_json, message_type) =
            row.map_err(|e| AppError::Database(format!("读取消息行失败: {e}")))?;

        // V2 的 assistant 消息没有 role 字段（由 `type` 列标识），
        // V1 消息仍按 data.role 过滤。
        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if schema == OpenCodeSchema::V1
            && value.get("role").and_then(|r| r.as_str()) != Some("assistant")
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

    // 耗时终点优先取 time.streamed（流式结束），它不含工具执行时间；
    // 缺失或早于 created（时钟异常）时退回 time.completed（含工具执行，估速偏低）。
    // 调用方已过滤掉没有 time.completed 的未完成消息；这里再兜底：
    // created 缺失（timestamp_ms 回退为 0，差值会变成毫秒纪元值）或
    // 时钟倒挂（终点不晚于 created）时按 0 处理
    let time = value.get("time");
    let streamed_ms = time
        .and_then(|t| t.get("streamed"))
        .and_then(|v| v.as_i64());
    let completed_ms = time
        .and_then(|t| t.get("completed"))
        .and_then(|v| v.as_i64());
    let end_ms = streamed_ms
        .filter(|&end| end > timestamp_ms)
        .or(completed_ms);
    let latency_ms = match end_ms {
        Some(end) if timestamp_ms > 0 && end > timestamp_ms => end - timestamp_ms,
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
    if should_skip_session_insert(&conn, "opencode_session", request_id, &dedup_key)? {
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

    // 登记持久化去重账本：防止后续 30 天明细 prune 导致重扫时重复导入
    crate::services::usage_stats::record_session_usage_dedup(
        &conn,
        "opencode_session",
        request_id,
    )?;

    Ok(inserted_rows > 0)
}

#[derive(Default)]
struct RealDayStats {
    count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    total_cost: Decimal,
}

/// 对 OpenCode 历史聚合数据进行自愈校准：
///
/// 当因为 30 天清理与会话重扫发生死循环放大时，`usage_daily_rollups` 中受影响日期的
/// 聚合数据（请求数与 Tokens）会被重复叠加。本函数直接从 `opencode.db` 重新聚合
/// 真实数值，仅对虚高的日期行进行原子校准纠正，并将历史消息 ID 全量补录到 `session_usage_dedup`。
pub(crate) fn repair_opencode_history_rollups(
    db: &Database,
    opencode_db_path: &Path,
) -> Result<u32, AppError> {
    let opencode_conn = rusqlite::Connection::open_with_flags(
        opencode_db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| AppError::Database(format!("打开 opencode.db 失败: {e}")))?;

    let schema = detect_schema(&opencode_conn)?;

    let sql = match schema {
        OpenCodeSchema::V1 => {
            "SELECT session_id, id, data, NULL FROM message WHERE time_created > 0"
        }
        OpenCodeSchema::V2 => {
            "SELECT session_id, id, data, type FROM session_message WHERE type IN ('assistant', 'compaction')"
        }
    };

    let mut stmt = opencode_conn
        .prepare(sql)
        .map_err(|e| AppError::Database(format!("准备历史消息查询失败: {e}")))?;

    let mut daily_stats: std::collections::HashMap<(String, String), RealDayStats> =
        std::collections::HashMap::new();
    let mut date_model_req_ids: std::collections::HashMap<(String, String), Vec<String>> =
        std::collections::HashMap::new();

    let cc_conn = lock_conn!(db.conn);
    let mut pricing_cache: std::collections::HashMap<String, Option<ModelPricing>> =
        std::collections::HashMap::new();

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| AppError::Database(format!("查询历史消息失败: {e}")))?;

    for row in rows {
        let (session_id, message_id, data_json, message_type) = match row {
            Ok(r) => r,
            Err(_) => continue,
        };

        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if schema == OpenCodeSchema::V1
            && value.get("role").and_then(|r| r.as_str()) != Some("assistant")
        {
            continue;
        }

        let is_completed = if message_type.as_deref() == Some("compaction") {
            matches!(
                value.get("status").and_then(|s| s.as_str()),
                Some("completed") | Some("failed")
            )
        } else {
            value.get("time").and_then(|t| t.get("completed")).is_some()
        };
        if !is_completed {
            continue;
        }

        let msg_data = match parse_message_data(&value) {
            Some(d) => d,
            None => continue,
        };

        let request_id = format!("opencode_session:{session_id}:{message_id}");
        let created_at_sec = msg_data.timestamp_ms / 1000;
        let date_str = match Local.timestamp_opt(created_at_sec, 0) {
            chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d").to_string(),
            chrono::LocalResult::Ambiguous(earliest, _) => earliest.format("%Y-%m-%d").to_string(),
            chrono::LocalResult::None => String::new(),
        };

        if date_str.is_empty() {
            continue;
        }

        let output_with_reasoning = (msg_data.output_tokens + msg_data.reasoning_tokens) as i64;
        let model_id = msg_data.model_id.clone();
        date_model_req_ids
            .entry((date_str.clone(), model_id.clone()))
            .or_default()
            .push(request_id);

        let cost_dec = if msg_data.cost > 0.0 {
            Decimal::try_from(msg_data.cost).unwrap_or(Decimal::ZERO)
        } else {
            let pricing = pricing_cache
                .entry(model_id.clone())
                .or_insert_with(|| find_model_pricing(&cc_conn, &model_id));
            if let Some(p) = pricing {
                let usage = TokenUsage {
                    input_tokens: msg_data.input_tokens,
                    output_tokens: output_with_reasoning as u32,
                    cache_read_tokens: msg_data.cache_read_tokens,
                    cache_creation_tokens: msg_data.cache_write_tokens,
                    cache_creation_1h_tokens: 0,
                    model: Some(model_id.clone()),
                    message_id: None,
                };
                let cost = CostCalculator::calculate_for_app("opencode", &usage, p, Decimal::ONE);
                cost.total_cost
            } else {
                Decimal::ZERO
            }
        };

        let stat = daily_stats.entry((date_str, model_id)).or_default();
        stat.count += 1;
        stat.input_tokens += msg_data.input_tokens as i64;
        stat.output_tokens += output_with_reasoning;
        stat.cache_read_tokens += msg_data.cache_read_tokens as i64;
        stat.cache_creation_tokens += msg_data.cache_write_tokens as i64;
        stat.total_cost += cost_dec;
    }

    cc_conn
        .execute("SAVEPOINT opencode_repair;", [])
        .map_err(|e| AppError::Database(e.to_string()))?;

    let repair_res = (|| -> Result<u32, AppError> {
        // 1. 将 proxy_request_logs 中已实际计入统计的 OpenCode 会话日志 request_id 回填到持久化账本 session_usage_dedup。
        // 切勿盲目将源库 opencode.db 里的全部消息提前入账，否则未导入的新消息与断线积压消息会被误拦截丢弃。
        cc_conn.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id)
             SELECT data_source, request_id, request_id, 0
             FROM proxy_request_logs
             WHERE data_source = 'opencode_session' AND request_id IS NOT NULL",
            [],
        )?;

        // 2. 遍历真实聚合值，精准纠正确凿发生重复叠加放大的行
        let mut repaired_count = 0u32;
        let mut dedup_stmt = cc_conn.prepare_cached(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id)
             VALUES ('opencode_session', ?1, ?1, 0)",
        )?;
        let mut check_stmt = cc_conn.prepare_cached(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
             FROM usage_daily_rollups
             WHERE date = ?1 AND app_type = 'opencode' AND provider_id = '_opencode_session' AND model = ?2",
        )?;
        let mut update_stmt = cc_conn.prepare_cached(
            "UPDATE usage_daily_rollups
             SET request_count = ?1,
                 success_count = ?1,
                 input_tokens = ?2,
                 output_tokens = ?3,
                 cache_read_tokens = ?4,
                 cache_creation_tokens = ?5,
                 total_cost_usd = ?6
             WHERE date = ?7 AND app_type = 'opencode' AND provider_id = '_opencode_session' AND model = ?8",
        )?;

        for ((date, model), real) in &daily_stats {
            let curr_row: Option<(i64, i64, i64, i64, i64)> = check_stmt
                .query_row(rusqlite::params![date, model], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .optional()
                .map_err(|e| AppError::Database(e.to_string()))?;

            if let Some((curr_count, curr_in, curr_out, curr_cr, curr_cc)) = curr_row {
                // 必须能区分「重复累加」与「源数据被删除」：
                // 如果用户删除了会话，源数据会变少，但已记录的历史汇总用量是真实的，绝对不能下调。
                // 仅当能确凿判定为死循环重复叠加（请求数是现存消息数的整数倍 k >= 2，且
                // 所有的 Token 指标 input/output/cache_read/cache_creation 均严格呈 k 倍放大）时，
                // 才确认为重复累加导致的虚高并进行校准，避免误伤已删除会话的正常历史。
                if real.count > 0 && curr_count > real.count && curr_count % real.count == 0 {
                    let k = curr_count / real.count;
                    let tokens_match = (real.input_tokens == 0 && curr_in == 0
                        || (real.input_tokens > 0 && curr_in == k * real.input_tokens))
                        && (real.output_tokens == 0 && curr_out == 0
                            || (real.output_tokens > 0 && curr_out == k * real.output_tokens))
                        && (real.cache_read_tokens == 0 && curr_cr == 0
                            || (real.cache_read_tokens > 0
                                && curr_cr == k * real.cache_read_tokens))
                        && (real.cache_creation_tokens == 0 && curr_cc == 0
                            || (real.cache_creation_tokens > 0
                                && curr_cc == k * real.cache_creation_tokens));

                    if tokens_match {
                        update_stmt
                            .execute(rusqlite::params![
                                real.count,
                                real.input_tokens,
                                real.output_tokens,
                                real.cache_read_tokens,
                                real.cache_creation_tokens,
                                real.total_cost.to_string(),
                                date,
                                model,
                            ])
                            .map_err(|e| AppError::Database(e.to_string()))?;
                        repaired_count += 1;

                        // 将这批已在汇总中修复且明细已被 prune 的历史消息 request_id 登记入账本，防止后续再次重复重扫
                        if let Some(req_ids) =
                            date_model_req_ids.get(&(date.clone(), model.clone()))
                        {
                            for req_id in req_ids {
                                dedup_stmt.execute([req_id])?;
                            }
                        }

                        log::info!(
                            "[OPENCODE-REPAIR] 已校准 {date} {model} (放大 {k} 倍): 请求数 {curr_count} -> {}, Tokens(In={}, Out={}, CR={})",
                            real.count,
                            real.input_tokens,
                            real.output_tokens,
                            real.cache_read_tokens,
                        );
                    }
                }
            }
        }

        Ok(repaired_count)
    })();

    match repair_res {
        Ok(count) => {
            cc_conn
                .execute("RELEASE opencode_repair;", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
            if count > 0 {
                crate::usage_events::notify_log_recorded();
            }
            Ok(count)
        }
        Err(e) => {
            cc_conn.execute("ROLLBACK TO opencode_repair;", []).ok();
            cc_conn.execute("RELEASE opencode_repair;", []).ok();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_detect_schema_prefers_v2_when_present() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE session (id TEXT); CREATE TABLE message (id TEXT);")
            .unwrap();
        assert_eq!(detect_schema(&conn).unwrap(), OpenCodeSchema::V1);

        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT); CREATE TABLE session_message (id TEXT);",
        )
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
        // 有 streamed 时终点取 streamed（流式结束），不含 completed 之后的收尾
        assert_eq!(data.latency_ms, 1789945288379 - 1789945285532);
    }

    #[test]
    fn test_parse_message_data_prefers_streamed_for_latency() {
        // 同时有 streamed 和 completed：终点取 streamed，排除 completed 之后的工具执行时间
        let with_streamed = serde_json::json!({
            "tokens": { "input": 1000, "output": 674 },
            "model": { "id": "m" },
            "time": { "created": 1000i64, "streamed": 2000i64, "completed": 120000i64 }
        });
        let data = parse_message_data(&with_streamed).unwrap();
        assert_eq!(data.latency_ms, 1000);

        // 只有 completed：退回 completed
        let only_completed = serde_json::json!({
            "tokens": { "input": 1000, "output": 674 },
            "model": { "id": "m" },
            "time": { "created": 1000i64, "completed": 3000i64 }
        });
        let data = parse_message_data(&only_completed).unwrap();
        assert_eq!(data.latency_ms, 2000);

        // streamed 早于 created（时钟异常）：忽略 streamed，退回 completed
        let skewed = serde_json::json!({
            "tokens": { "input": 1000, "output": 674 },
            "model": { "id": "m" },
            "time": { "created": 5000i64, "streamed": 4000i64, "completed": 9000i64 }
        });
        let data = parse_message_data(&skewed).unwrap();
        assert_eq!(data.latency_ms, 4000);
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

    #[test]
    fn test_repair_opencode_history_rollups() -> Result<(), AppError> {
        let temp_dir = tempfile::tempdir().unwrap();
        let opencode_db_path = temp_dir.path().join("opencode.db");

        // 1. 初始化模拟的 opencode.db (V2 架构)
        {
            let oc_conn = rusqlite::Connection::open(&opencode_db_path).unwrap();
            oc_conn.execute_batch(
                "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, time_updated INTEGER);
                 CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, time_updated INTEGER, data TEXT);
                 INSERT INTO session_v2 VALUES ('ses_1', 1788750000000);"
            ).unwrap();

            // 构造一条 2026-09-06 (时间戳 1788750000000 毫秒) 的 assistant 消息
            let msg_data = serde_json::json!({
                "model": { "id": "gemini-3.8-flash-high" },
                "cost": 0.05,
                "tokens": {
                    "input": 1000,
                    "output": 200,
                    "reasoning": 50,
                    "cache": { "read": 5000, "write": 0 }
                },
                "time": {
                    "created": 1788750000000i64,
                    "completed": 1788750005000i64
                }
            })
            .to_string();

            oc_conn.execute(
                "INSERT INTO session_message VALUES ('msg_1', 'ses_1', 'assistant', 1788750000000, ?1)",
                [msg_data],
            ).unwrap();
        }

        // 2. 初始化 cc-switch 内存数据库，并注入一条被死循环放大的虚高记录，以及一条正常代理记录
        let db = Database::memory().unwrap();
        let date_str = Local
            .timestamp_opt(1788750000, 0)
            .single()
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES (?1, 'opencode', '_opencode_session', 'gemini-3.8-flash-high', 25000, 25000, 25000000, 6250000, 125000000, 0, '125.0', 100)",
                [&date_str],
            ).unwrap();

            // 注入正常代理渠道产生的记录，验证不会被自愈误伤
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES (?1, 'opencode', 'real_proxy', 'gemini-3.8-flash-high', 5, 5, 500, 50, 0, 0, '0.01', 80)",
                [&date_str],
            ).unwrap();
        }

        // 3. 执行自愈校准
        let repaired = repair_opencode_history_rollups(&db, &opencode_db_path).unwrap();
        assert_eq!(repaired, 1, "应成功校准 1 条被放大的行");

        // 4. 验证 usage_daily_rollups 中的数值已被精准纠正回真实值
        {
            let conn = lock_conn!(db.conn);
            let (req_count, input, output, cache_read, cost): (i64, i64, i64, i64, String) = conn.query_row(
                "SELECT request_count, input_tokens, output_tokens, cache_read_tokens, total_cost_usd
                 FROM usage_daily_rollups
                 WHERE date = ?1 AND app_type = 'opencode' AND provider_id = '_opencode_session' AND model = 'gemini-3.8-flash-high'",
                [&date_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ).unwrap();

            assert_eq!(req_count, 1, "请求数应被校准回真实的 1 次");
            assert_eq!(input, 1000, "input_tokens 应为真实 1000");
            assert_eq!(output, 250, "output_tokens 应为 200 + 50 = 250");
            assert_eq!(cache_read, 5000, "cache_read 应为 5000");
            assert_eq!(cost, "0.05", "费用应为真实的 0.05");

            // 验证真实代理记录未被篡改
            let real_proxy_count: i64 = conn.query_row(
                "SELECT request_count FROM usage_daily_rollups
                 WHERE date = ?1 AND app_type = 'opencode' AND provider_id = 'real_proxy' AND model = 'gemini-3.8-flash-high'",
                [&date_str],
                |r| r.get(0),
            ).unwrap();
            assert_eq!(real_proxy_count, 5, "正常代理记录不应受影响");

            // 5. 验证被校准的历史消息 request_id 已被登记到持久去重账本 session_usage_dedup
            let dedup_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_usage_dedup WHERE data_source = 'opencode_session' AND request_id = 'opencode_session:ses_1:msg_1')",
                [],
                |r| r.get(0),
            ).unwrap();
            assert!(
                dedup_exists,
                "被校准的历史消息 ID 必须补录到 session_usage_dedup"
            );
        }

        Ok(())
    }

    #[test]
    fn test_repair_opencode_history_rollups_preserves_deleted_session_dates() -> Result<(), AppError>
    {
        let temp_dir = tempfile::tempdir().unwrap();
        let opencode_db_path = temp_dir.path().join("opencode.db");

        // 1. 初始化 opencode.db：用户原本发了 2 次请求，后来在 OpenCode 删除了其中 1 个会话，现存库里只剩 1 条消息
        {
            let oc_conn = rusqlite::Connection::open(&opencode_db_path).unwrap();
            oc_conn.execute_batch(
                "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, time_updated INTEGER);
                 CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, time_updated INTEGER, data TEXT);
                 INSERT INTO session_v2 VALUES ('ses_remained', 1788750000000);"
            ).unwrap();

            let msg_data = serde_json::json!({
                "model": { "id": "gemini-3.8-flash-high" },
                "cost": 0.05,
                "tokens": {
                    "input": 1000,
                    "output": 200,
                    "reasoning": 50,
                    "cache": { "read": 5000, "write": 0 }
                },
                "time": {
                    "created": 1788750000000i64,
                    "completed": 1788750005000i64
                }
            })
            .to_string();

            oc_conn.execute(
                "INSERT INTO session_message VALUES ('msg_remained', 'ses_remained', 'assistant', 1788750000000, ?1)",
                [msg_data],
            ).unwrap();
        }

        // 2. 初始化 cc-switch 数据库：
        // 历史上实际记录过 2 次请求（总共 2500 input tokens，与剩下来的 1000 并非整数倍放大关系）
        let db = Database::memory().unwrap();
        let date_str = Local
            .timestamp_opt(1788750000, 0)
            .single()
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, success_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, avg_latency_ms
                ) VALUES (?1, 'opencode', '_opencode_session', 'gemini-3.8-flash-high', 2, 2, 2500, 450, 8000, 0, '0.12', 100)",
                [&date_str],
            ).unwrap();
        }

        // 3. 执行自愈校准
        let repaired = repair_opencode_history_rollups(&db, &opencode_db_path).unwrap();
        assert_eq!(
            repaired, 0,
            "删过会话导致的源数据减少不应被误判为死循环放大，校准数为 0"
        );

        // 4. 验证 usage_daily_rollups 中的数值未被篡改，依然保持 2 次真实请求
        {
            let conn = lock_conn!(db.conn);
            let (req_count, input): (i64, i64) = conn.query_row(
                "SELECT request_count, input_tokens
                 FROM usage_daily_rollups
                 WHERE date = ?1 AND app_type = 'opencode' AND provider_id = '_opencode_session' AND model = 'gemini-3.8-flash-high'",
                [&date_str],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).unwrap();

            assert_eq!(
                req_count, 2,
                "删过会话的历史用量是真实的，请求数必须保持 2 次"
            );
            assert_eq!(
                input, 2500,
                "input_tokens 必须保持原本记录的 2500，不能被覆盖缩小"
            );
        }

        Ok(())
    }

    #[test]
    fn test_sync_opencode_usage_empty_db_first_sync() -> Result<(), AppError> {
        let temp_dir = tempfile::tempdir().unwrap();
        let opencode_db_path = temp_dir.path().join("opencode.db");

        // 1. 初始化 opencode.db，包含 1 条已完成消息
        {
            let oc_conn = rusqlite::Connection::open(&opencode_db_path).unwrap();
            oc_conn.execute_batch(
                "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, time_updated INTEGER);
                 CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
                 INSERT INTO session_v2 VALUES ('ses_first', 1788750000000);"
            ).unwrap();

            let msg_data = serde_json::json!({
                "model": { "id": "gemini-3.8-flash-high" },
                "cost": 0.05,
                "tokens": {
                    "input": 1000,
                    "output": 200,
                    "reasoning": 50,
                    "cache": { "read": 5000, "write": 0 }
                },
                "time": {
                    "created": 1788750000000i64,
                    "completed": 1788750005000i64
                }
            })
            .to_string();

            oc_conn.execute(
                "INSERT INTO session_message VALUES ('msg_first', 'ses_first', 'assistant', 1788750000000, 1788750000000, ?1)",
                [msg_data],
            ).unwrap();
        }

        // 2. 空的 cc-switch 数据库进行首次同步
        let db = Database::memory().unwrap();
        let result = sync_opencode_usage_with_path(&db, &opencode_db_path)?;

        // 3. 验证首次同步成功导入 1 条，跳过 0 条
        assert_eq!(result.imported, 1, "空库首次同步应成功导入 1 条消息");
        assert_eq!(
            result.skipped, 0,
            "空库首次同步跳过数应为 0，不能被账本误跳过"
        );

        {
            let conn = lock_conn!(db.conn);
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'opencode_session'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(count, 1, "明细表应成功写入 1 条");

            let dedup_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_usage_dedup WHERE data_source = 'opencode_session' AND request_id = 'opencode_session:ses_first:msg_first')",
                [],
                |r| r.get(0),
            )?;
            assert!(dedup_exists, "导入后应登记到持久去重账本");
        }

        Ok(())
    }
}
