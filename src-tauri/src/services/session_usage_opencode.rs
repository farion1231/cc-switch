//! OpenCode 会话日志使用追踪
//!
//! 从 ~/.local/share/opencode/opencode.db (SQLite) 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.local/share/opencode/opencode.db
//!   → session 表获取所有会话
//!   → message 表获取 assistant 消息
//!   → 解析 data JSON 提取 tokens/cost/model
//!   → proxy_request_logs 表
//! ```

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
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
    agent: Option<String>,
    timestamp_ms: i64,
}

struct OpenCodeMessageQueryResult {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

/// 同步 OpenCode 使用数据
pub fn sync_opencode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_opencode_db_path();
    log::info!("[OPENCODE-SYNC] start, db_path={}", db_path.display());

    if !db_path.exists() {
        log::info!(
            "[OPENCODE-SYNC] opencode.db 不存在，跳过同步：{}",
            db_path.display()
        );
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            errors: vec![],
        });
    }
    log::info!("[OPENCODE-SYNC] opencode.db 存在: {}", db_path.display());

    let db_path_str = db_path.to_string_lossy().to_string();

    // 检查文件修改时间。
    // opencode 的数据库运行在 WAL 模式：新提交先落在 -wal 文件里，
    // 主库文件只有在 checkpoint 时才更新。因此必须同时考虑 -wal 的
    // mtime，否则会在 checkpoint 之前漏掉刚写入的会话。
    let metadata = fs::metadata(&db_path)
        .map_err(|e| AppError::Config(format!("无法读取 opencode.db 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    let wal_path = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    let (last_modified, _last_offset) = get_sync_state(db, &db_path_str)?;
    log::info!(
        "[OPENCODE-SYNC] mtime: file_modified={}, last_modified={}, delta={}ns, will_sync={}",
        file_modified,
        last_modified,
        file_modified - last_modified,
        file_modified > last_modified
    );

    // 文件未变化则跳过
    if file_modified <= last_modified {
        log::info!("[OPENCODE-SYNC] 文件未变化（<= last_modified），跳过");
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 1,
            errors: vec![],
        });
    }

    // 打开 opencode 的 SQLite 数据库（只读）
    let opencode_conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?;

    // 诊断：dump 一行消息样本数据（仅第一次），帮用户验证 schema 是否与代码预期一致
    match opencode_conn.query_row(
        "SELECT id, session_id, time_created, data FROM message ORDER BY time_created DESC LIMIT 1",
        [],
        |row| {
            let id: String = row.get(0)?;
            let sid: String = row.get(1)?;
            let tc: i64 = row.get(2)?;
            let data: String = row.get(3)?;
            Ok((id, sid, tc, data))
        },
    ) {
        Ok((id, sid, tc, data)) => {
            log::debug!(
                "[OPENCODE-SYNC] 样本消息: id={}, session_id={}, time_created={}, data前300字符: {}",
                id, sid, tc,
                data.chars().take(300).collect::<String>()
            );
        }
        Err(e) => {
            log::warn!("[OPENCODE-SYNC] 无法读取样本消息（schema 可能不同?）: {e}");
        }
    }

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 1,
        errors: vec![],
    };
    let mut has_sync_errors = false;

    // 查询所有会话
    let sessions = query_sessions(&opencode_conn)?;
    log::info!("[OPENCODE-SYNC] 发现 {} 个 session", sessions.len());

    for (session_id, time_updated) in &sessions {
        // 检查会话是否需要重新同步
        let sync_key = format!("{db_path_str}:{session_id}");
        let (sess_last_modified, _) = get_sync_state(db, &sync_key)?;
        if *time_updated <= sess_last_modified {
            log::debug!(
                "[OPENCODE-SYNC] session={} 未更新 ({} <= {}), 跳过",
                session_id, time_updated, sess_last_modified
            );
            continue; // 会话未更新，跳过
        }
        log::debug!(
            "[OPENCODE-SYNC] 处理 session={}, time_updated={}",
            session_id, time_updated
        );

        let mut session_had_error = false;

        // 查询该会话的所有 assistant 消息
        let mut session_has_incomplete_usage = false;
        match query_assistant_messages(&opencode_conn, session_id) {
            Ok(query_result) => {
                session_has_incomplete_usage = query_result.has_incomplete_usage;
                log::info!(
                    "[OPENCODE-SYNC]   session={} 解析到 {} 条 assistant 消息, has_incomplete_usage={}",
                    session_id,
                    query_result.messages.len(),
                    query_result.has_incomplete_usage
                );
                for (message_id, msg_data) in &query_result.messages {
                    let request_id = format!("opencode_session:{session_id}:{message_id}");

                    match insert_opencode_message(db, &request_id, msg_data, session_id) {
                        Ok(true) => {
                            result.imported += 1;
                            log::debug!(
                                "[OPENCODE-SYNC]     inserted msg={} (in:{} out:{} cache_r:{} cache_w:{} model={} agent={:?})",
                                message_id,
                                msg_data.input_tokens,
                                msg_data.output_tokens,
                                msg_data.cache_read_tokens,
                                msg_data.cache_write_tokens,
                                msg_data.model_id,
                                msg_data.agent
                            );
                        }
                        Ok(false) => {
                            result.skipped += 1;
                            log::debug!(
                                "[OPENCODE-SYNC]     skipped msg={} (dedup hit 或 INSERT OR IGNORE 冲突)",
                                message_id
                            );
                        }
                        Err(e) => {
                            let msg = format!("OpenCode 消息插入失败 {request_id}: {e}");
                            log::warn!("[OPENCODE-SYNC]     {msg}");
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
            log::info!(
                "[OPENCODE-SYNC]   session={} 有未完成消息，跳过状态推进（下轮重试）",
                session_id
            );
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
        update_sync_state(db, &db_path_str, file_modified, 0)?;
    }

    log::info!(
        "[OPENCODE-SYNC] 完成: imported={}, skipped={}, errors={}, sessions_scanned={}",
        result.imported,
        result.skipped,
        result.errors.len(),
        sessions.len()
    );

    Ok(result)
}

/// 查询所有会话的 (id, sync_watermark)
fn query_sessions(conn: &rusqlite::Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id,
                    MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
             FROM session s
             LEFT JOIN message m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY sync_watermark",
        )
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

/// 兼容 OpenCode 实际存储的嵌套格式 `{ message: { role, tokens, time, ... }, parts: [...] }`。
/// 老版本/部分自定义 schema 可能把字段放在顶层，这里回退一下。
fn message_obj<'a>(value: &'a serde_json::Value) -> &'a serde_json::Value {
    value.get("message").unwrap_or(value)
}

/// 查询某会话的已完成 assistant 消息，并标记是否还有未完成 usage 消息。
fn query_assistant_messages(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<OpenCodeMessageQueryResult, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created")
        .map_err(|e| AppError::Database(format!("准备消息查询失败: {e}")))?;

    let rows = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询消息失败: {e}")))?;

    let mut messages = Vec::new();
    let mut has_incomplete_usage = false;
    // 诊断计数器：每条消息被丢的具体原因
    let mut skipped_json_err = 0usize;
    let mut skipped_role = 0usize;
    let mut skipped_no_tokens = 0usize;
    let mut skipped_no_completed = 0usize;
    let mut skipped_all_zero = 0usize;
    for row in rows {
        let (message_id, data_json) =
            row.map_err(|e| AppError::Database(format!("读取消息行失败: {e}")))?;

        // 解析整体 data 容器
        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(v) => v,
            Err(e) => {
                log::debug!(
                    "[OPENCODE-SYNC]   msg={} skipped: JSON 解析失败: {}",
                    message_id, e
                );
                skipped_json_err += 1;
                continue;
            }
        };

        // OpenCode 实际 schema 把 role/tokens/time 嵌套在 `message` 字段下。
        // 顶层 fallback 兼容旧版 schema。
        let m = message_obj(&value);

        // 只处理 assistant 消息
        let role = m.get("role").and_then(|r| r.as_str());
        if role != Some("assistant") {
            log::debug!(
                "[OPENCODE-SYNC]   msg={} skipped: role={:?} (非 assistant)",
                message_id, role
            );
            skipped_role += 1;
            continue;
        }

        // 必须有 tokens 字段
        if m.get("tokens").is_none() {
            // 没有 tokens 字段时，把 data 的顶层 key 列表打出来帮用户诊断 schema
            let top_keys: Vec<&str> = value
                .as_object()
                .map(|o| o.keys().map(|s| s.as_str()).collect())
                .unwrap_or_default();
            let inner_keys: Vec<&str> = m
                .as_object()
                .map(|o| o.keys().map(|s| s.as_str()).collect())
                .unwrap_or_default();
            log::debug!(
                "[OPENCODE-SYNC]   msg={} skipped: 没有 tokens 字段. 顶层 keys={:?}, 解析后 keys={:?}",
                message_id, top_keys, inner_keys
            );
            skipped_no_tokens += 1;
            continue;
        }

        // 跳过未完成的消息：进行中只有半截 token，且因 INSERT OR IGNORE 无法回填
        if m.get("time").and_then(|t| t.get("completed")).is_none() {
            has_incomplete_usage = true;
            log::debug!(
                "[OPENCODE-SYNC]   msg={} skipped: 没有 time.completed（消息未完成）",
                message_id
            );
            skipped_no_completed += 1;
            continue;
        }

        match parse_message_data(&value) {
            Some(msg_data) => messages.push((message_id, msg_data)),
            None => {
                log::debug!(
                    "[OPENCODE-SYNC]   msg={} skipped: parse_message_data 返回 None（全零 token 或字段缺失）",
                    message_id
                );
                skipped_all_zero += 1;
            }
        }
    }

    // 把这一轮的 skip 统计记下来（INFO 级别，确保用户能看到总数）
    if skipped_json_err + skipped_role + skipped_no_tokens + skipped_no_completed + skipped_all_zero
        > 0
    {
        log::info!(
            "[OPENCODE-SYNC]   session={} 跳过统计: json_err={}, role={}, no_tokens={}, no_completed={}, all_zero={}",
            session_id, skipped_json_err, skipped_role, skipped_no_tokens, skipped_no_completed, skipped_all_zero
        );
    }

    Ok(OpenCodeMessageQueryResult {
        messages,
        has_incomplete_usage,
    })
}

/// 解析 opencode message.data JSON 为结构化数据
///
/// 支持 OpenCode 实际 schema（字段在 `message` 嵌套对象下）和扁平 schema 双形态。
fn parse_message_data(value: &serde_json::Value) -> Option<OpenCodeMessageData> {
    let m = message_obj(value);
    let tokens = m.get("tokens")?;

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

    let cost = m.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let model_id = m
        .get("modelID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let agent = m
        .get("agent")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let timestamp_ms = m
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost,
        model_id,
        agent,
        timestamp_ms,
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
            provider_type, is_streaming, cost_multiplier, created_at, data_source, agent
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
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
            0i64,                  // latency_ms
            Option::<i64>::None,   // first_token_ms
            200i64,                // status_code
            Option::<String>::None,// error_message
            Some(session_id.to_string()),
            Some("opencode_session"), // provider_type
            1i64,                  // is_streaming
            "1.0",                 // cost_multiplier
            created_at,
            "opencode_session",    // data_source
            msg.agent.clone(),     // agent (OpenCode-specific, e.g. "build"/"plan"/"sisyphus")
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 OpenCode 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
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
    }

    #[test]
    fn test_parse_message_data_extracts_agent() {
        // Build agent present
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 100, "output": 50 },
            "modelID": "claude-opus-4-5",
            "agent": "build",
            "time": { "created": 100, "completed": 200 }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.agent, Some("build".to_string()));

        // Plan agent
        let json2: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 100, "output": 50 },
            "modelID": "claude-opus-4-5",
            "agent": "plan",
            "time": { "created": 100, "completed": 200 }
        });
        let data2 = parse_message_data(&json2).unwrap();
        assert_eq!(data2.agent, Some("plan".to_string()));

        // Missing agent field
        let json3: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 100, "output": 50 },
            "modelID": "m",
            "time": { "created": 100, "completed": 200 }
        });
        let data3 = parse_message_data(&json3).unwrap();
        assert_eq!(data3.agent, None);

        // agent is JSON null
        let json4: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 100, "output": 50 },
            "modelID": "m",
            "agent": null,
            "time": { "created": 100, "completed": 200 }
        });
        let data4 = parse_message_data(&json4).unwrap();
        assert_eq!(data4.agent, None);
    }

    /// 回归测试：OpenCode 真实 schema 把 role/tokens/time 嵌套在 `message` 字段下。
    /// 之前版本直接 `value.get("role")` 永远拿到 None，导致所有消息被跳过。
    #[test]
    fn test_parse_message_data_nested_message_format() {
        // 来自用户实测的真实 OpenCode message.data JSON
        let json: serde_json::Value = serde_json::json!({
            "message": {
                "id": "msg_ea25e940f001xVPLg7Pw6yl2qT",
                "parentID": "msg_ea25d91e90013EoZFwg9vnWSZR",
                "role": "assistant",
                "mode": "Atlas - Plan Executor",
                "agent": "Atlas - Plan Executor",
                "path": {
                    "cwd": "D:\\code-py\\forkcc",
                    "root": "D:\\code-py\\forkcc"
                },
                "cost": 0,
                "tokens": {
                    "input": 1127,
                    "output": 1173,
                    "reasoning": 0,
                    "cache": {
                        "read": 286194,
                        "write": 0
                    },
                    "total": 288494
                },
                "modelID": "oc/minimax-m3-free",
                "providerID": "9router",
                "time": {
                    "created": 1780840567823i64,
                    "completed": 1780840609494i64
                },
                "sessionID": "ses_15ec1005effeLGOqD5IuNEqO5j",
                "finish": "stop"
            },
            "parts": [
                {
                    "id": "prt_ea25ec1d5001GlzZnLY4Mhdo5A",
                    "messageID": "msg_ea25e940f001xVPLg7Pw6yl2qT",
                    "sessionID": "ses_15ec1005effeLGOqD5IuNEqO5j",
                    "type": "text",
                    "text": "输出内容",
                    "time": {
                        "start": 1780840579541i64,
                        "end": 1780840608790i64
                    }
                }
            ]
        });

        let data = parse_message_data(&json).expect("parse must succeed for nested format");
        assert_eq!(data.input_tokens, 1127, "input tokens should be 1127");
        assert_eq!(data.output_tokens, 1173, "output tokens should be 1173");
        assert_eq!(data.reasoning_tokens, 0);
        assert_eq!(data.cache_read_tokens, 286194, "cache.read should be 286194");
        assert_eq!(data.cache_write_tokens, 0);
        assert_eq!(data.cost, 0.0);
        assert_eq!(data.model_id, "oc/minimax-m3-free");
        assert_eq!(data.agent, Some("Atlas - Plan Executor".to_string()));
        assert_eq!(data.timestamp_ms, 1780840567823);
    }

    /// 回归测试：query_assistant_messages 对嵌套 schema 也要正确处理。
    /// 之前 version.role check 失败导致整条消息被跳过。
    #[test]
    fn test_query_assistant_messages_handles_nested_message_format() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();

        let nested = serde_json::json!({
            "message": {
                "id": "msg_nested_1",
                "role": "assistant",
                "tokens": { "input": 100, "output": 200, "cache": { "read": 0, "write": 0 } },
                "modelID": "m",
                "agent": "build",
                "time": { "created": 100, "completed": 200 }
            },
            "parts": []
        })
        .to_string();

        conn.execute(
            "INSERT INTO message VALUES ('m1', 's1', 1, ?1)",
            rusqlite::params![nested],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, "s1").unwrap();
        // 关键断言：嵌套 schema 也能解析出来
        assert_eq!(result.messages.len(), 1, "nested format must produce 1 message");
        assert_eq!(result.messages[0].0, "m1");
        assert_eq!(result.messages[0].1.input_tokens, 100);
        assert_eq!(result.messages[0].1.output_tokens, 200);
        assert_eq!(result.messages[0].1.agent, Some("build".to_string()));
        assert!(!result.has_incomplete_usage);
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

        let result = query_assistant_messages(&conn, "s1").unwrap();
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

        let sessions = query_sessions(&conn).unwrap();
        assert_eq!(sessions, vec![("s1".to_string(), 200)]);
    }
}
