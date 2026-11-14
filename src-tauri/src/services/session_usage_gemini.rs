//! Gemini CLI 会话日志使用追踪
//!
//! 从 ~/.gemini/tmp/<project_hash>/chats/session-*.json(l) 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.gemini/tmp/*/chats/session-*.json(l) → 全量解析 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 与 Claude/Codex 解析器的差异
//! - 旧版 JSON 是单个对象；新版 JSONL 按记录回放还原成同样的 messages 数组
//!   （见 [`crate::session_manager::providers::gemini::parse_session_document`]）
//! - 无需 delta 计算：tokens 字段是 per-message 独立值
//! - 无需状态恢复：不依赖前一条消息的累计值
//! - 天然去重：每条消息有唯一 id 字段

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, has_matching_proxy_usage_log, DedupKey};
use crate::session_manager::providers::gemini::{is_session_file, parse_session_document};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const GEMINI_DATA_SOURCE: &str = "gemini_session";

/// 从 Gemini message 中提取的 token 数据
#[derive(Debug)]
struct GeminiTokens {
    input: u32,
    output: u32,
    cached: u32,
    thoughts: u32,
}

/// 同步 Gemini 使用数据（从 JSON 会话日志）
pub fn sync_gemini_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let gemini_dir = get_gemini_dir();

    let files = collect_gemini_session_files(&gemini_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    if files.is_empty() {
        return Ok(result);
    }

    let cursors = crate::services::session_usage::load_sync_cursors(db)?;

    for file_path in &files {
        let last_modified = cursors
            .get(file_path.to_string_lossy().as_ref())
            .map_or(0, |c| c.last_modified);
        match sync_single_gemini_file(db, file_path, last_modified) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Gemini 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[GEMINI-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[GEMINI-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Gemini 会话 JSON 文件
fn collect_gemini_session_files(gemini_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.is_dir() {
        return files;
    }

    // 遍历 tmp/<project_hash>/chats/session-*.json(l)
    let project_dirs = match fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in project_dirs.flatten() {
        let chats_dir = entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let chat_files = match fs::read_dir(&chats_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            let is_session = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("session-"))
                && is_session_file(&path);
            if is_session {
                files.push(path);
            }
        }
    }

    files
}

/// 同步单个 Gemini 会话 JSON 文件，返回 (imported, skipped)。
///
/// `last_modified` 来自调用方批量预取的游标（见 [`crate::services::session_usage::load_sync_cursors`]）。
fn sync_single_gemini_file(
    db: &Database,
    file_path: &Path,
    last_modified: i64,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 读取并解析整个 JSON 文件
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let value = parse_session_document(&content)
        .ok_or_else(|| AppError::Config("JSON 解析失败".to_string()))?;

    // 提取顶层 sessionId
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 遍历 messages 数组
    let messages = match value.get("messages").and_then(|v| v.as_array()) {
        Some(msgs) => msgs,
        None => return Ok((0, 0)),
    };

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;
    let mut gemini_msg_count: i64 = 0;

    for msg in messages {
        // 只处理 type == "gemini" 的消息
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        // 提取 tokens 对象
        let tokens_obj = match msg.get("tokens") {
            Some(t) if t.is_object() => t,
            _ => continue,
        };

        let tokens = parse_gemini_tokens(tokens_obj);
        if tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0 {
            continue; // 跳过全零的空 token 消息
        }

        gemini_msg_count += 1;

        // 提取消息 ID 和模型
        let message_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let timestamp = msg.get("timestamp").and_then(|v| v.as_str());

        // 生成唯一 request_id
        let session_id_str = session_id.as_deref().unwrap_or("unknown");
        let request_id = format!("gemini_session:{session_id_str}:{message_id}");

        match insert_gemini_session_entry(
            db,
            &request_id,
            &tokens,
            model,
            session_id.as_deref(),
            timestamp,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[GEMINI-SYNC] 插入失败 ({}): {e}", request_id);
                skipped += 1;
            }
        }
    }

    // 更新同步状态
    update_sync_state(db, &file_path_str, file_modified, gemini_msg_count)?;

    Ok((imported, skipped))
}

/// 从 tokens JSON 对象中提取 token 数据
fn parse_gemini_tokens(tokens: &serde_json::Value) -> GeminiTokens {
    GeminiTokens {
        input: tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cached: tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        thoughts: tokens.get("thoughts").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
    }
}

/// 插入单条 Gemini 会话记录到 proxy_request_logs
fn insert_gemini_session_entry(
    db: &Database,
    request_id: &str,
    tokens: &GeminiTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
) -> Result<bool, AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Gemini 会话日志事务失败: {e}")))?;

    let created_at = timestamp
        .and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
        })
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    // 合并 thoughts 到 output（思考 token 按输出计费）
    let output_tokens = tokens.output + tokens.thoughts;

    let existing_row: Option<(String, String, i64, i64, i64)> = tx
        .prepare_cached(
            "SELECT COALESCE(data_source, 'proxy'), model, input_tokens, output_tokens, cache_read_tokens
             FROM proxy_request_logs
             WHERE request_id = ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_row(rusqlite::params![request_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
        })
        .optional()
        .map_err(|e| AppError::Database(format!("查询 Gemini 会话日志失败: {e}")))?;

    if let Some((existing_source, old_model, old_input, old_output, old_cached)) = existing_row {
        if existing_source != GEMINI_DATA_SOURCE {
            return Ok(false);
        }

        // 确保未剪枝的存量明细也拥有长期去重收据，后续归档删除明细后不会重导
        record_gemini_dedup_tombstone(&tx, request_id)?;

        if old_model == model
            && old_input == i64::from(tokens.input)
            && old_output == i64::from(output_tokens)
            && old_cached == i64::from(tokens.cached)
        {
            tx.commit()
                .map_err(|e| AppError::Database(format!("提交 Gemini 会话日志事务失败: {e}")))?;
            return Ok(false);
        }

        let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
            compute_gemini_entry_costs(&tx, model, tokens.input, output_tokens, tokens.cached);

        let updated = tx
            .prepare_cached(
                "UPDATE proxy_request_logs SET
                    model = ?2,
                    request_model = ?2,
                    input_tokens = ?3,
                    output_tokens = ?4,
                    cache_read_tokens = ?5,
                    input_cost_usd = ?6,
                    output_cost_usd = ?7,
                    cache_read_cost_usd = ?8,
                    cache_creation_cost_usd = ?9,
                    total_cost_usd = ?10
                WHERE request_id = ?1 AND data_source = ?11",
            )
            .and_then(|mut stmt| {
                stmt.execute(rusqlite::params![
                    request_id,
                    model,
                    tokens.input,
                    output_tokens,
                    tokens.cached,
                    input_cost,
                    output_cost,
                    cache_read_cost,
                    cache_creation_cost,
                    total_cost,
                    GEMINI_DATA_SOURCE,
                ])
            })
            .map_err(|e| AppError::Database(format!("更新 Gemini 会话日志失败: {e}")))?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Gemini 会话日志事务失败: {e}")))?;
        return Ok(updated > 0);
    }

    // 明细不存在时检查长期去重收据：已被 rollup_and_prune 归档删除的历史消息不得重复导入
    let already_recorded: bool = tx
        .prepare_cached(
            "SELECT EXISTS(
                SELECT 1 FROM session_usage_dedup
                WHERE data_source = ?1 AND request_id = ?2
            )",
        )
        .and_then(|mut stmt| {
            stmt.query_row(rusqlite::params![GEMINI_DATA_SOURCE, request_id], |row| {
                row.get(0)
            })
        })
        .map_err(|e| AppError::Database(format!("查询 Gemini 会话去重收据失败: {e}")))?;
    if already_recorded {
        return Ok(false);
    }

    let dedup_key = DedupKey {
        app_type: "gemini",
        data_source: GEMINI_DATA_SOURCE,
        model,
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        created_at,
    };
    if has_matching_proxy_usage_log(&tx, &dedup_key)? {
        record_gemini_dedup_tombstone(&tx, request_id)?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Gemini 会话日志事务失败: {e}")))?;
        return Ok(false);
    }

    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        compute_gemini_entry_costs(&tx, model, tokens.input, output_tokens, tokens.cached);

    record_gemini_dedup_tombstone(&tx, request_id)?;

    tx.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        rusqlite::params![
            request_id,
            "_gemini_session",   // provider_id
            "gemini",            // app_type
            model,
            model,               // request_model = model
            tokens.input,
            output_tokens,
            tokens.cached,
            0i64,                // cache_creation_tokens
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,                // latency_ms
            Option::<i64>::None, // first_token_ms
            200i64,              // status_code
            Option::<String>::None, // error_message
            session_id.map(|s| s.to_string()),
            Some(GEMINI_DATA_SOURCE), // provider_type
            1i64,                // is_streaming
            "1.0",               // cost_multiplier
            created_at,
            GEMINI_DATA_SOURCE,  // data_source
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Gemini 会话日志失败: {e}")))?;

    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Gemini 会话日志事务失败: {e}")))?;
    Ok(true)
}

fn record_gemini_dedup_tombstone(
    conn: &rusqlite::Connection,
    request_id: &str,
) -> Result<(), AppError> {
    conn.prepare_cached(
        "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?2, 1)",
    )
    .and_then(|mut stmt| stmt.execute(rusqlite::params![GEMINI_DATA_SOURCE, request_id]))
    .map_err(|e| AppError::Database(format!("记录 Gemini 会话去重收据失败: {e}")))?;
    Ok(())
}

fn compute_gemini_entry_costs(
    conn: &rusqlite::Connection,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
) -> (String, String, String, String, String) {
    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    match find_gemini_pricing(conn, model) {
        Some(pricing) => {
            let cost =
                CostCalculator::calculate_for_app("gemini", &usage, &pricing, Decimal::from(1));
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
}

/// 查找 Gemini 模型定价
fn find_gemini_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_gemini_session_files_nonexistent() {
        let files = collect_gemini_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_insert_gemini_session_skips_matching_proxy_log() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "gemini-proxy",
                    "google",
                    "gemini",
                    "gemini-2.5-pro",
                    "gemini-2.5-pro",
                    10,
                    7,
                    1,
                    0,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let tokens = GeminiTokens {
            input: 10,
            output: 2,
            cached: 1,
            thoughts: 5,
        };
        let inserted = insert_gemini_session_entry(
            &db,
            "gemini-session-dup",
            &tokens,
            "gemini-2.5-pro",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
        )?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_parse_gemini_tokens() {
        let json: serde_json::Value = serde_json::json!({
            "input": 8522,
            "output": 29,
            "cached": 3138,
            "thoughts": 405,
            "tool": 0,
            "total": 8956
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 8522);
        assert_eq!(tokens.output, 29);
        assert_eq!(tokens.cached, 3138);
        assert_eq!(tokens.thoughts, 405);
        // output + thoughts = 29 + 405 = 434（用于计费）
        assert_eq!(tokens.output + tokens.thoughts, 434);
    }

    #[test]
    fn test_parse_gemini_tokens_missing_fields() {
        // 缺少某些字段时应返回 0
        let json: serde_json::Value = serde_json::json!({
            "input": 100,
            "output": 50
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 100);
        assert_eq!(tokens.output, 50);
        assert_eq!(tokens.cached, 0);
        assert_eq!(tokens.thoughts, 0);
    }

    #[test]
    fn test_parse_gemini_tokens_all_zero() {
        let json: serde_json::Value = serde_json::json!({
            "input": 0,
            "output": 0,
            "cached": 0,
            "thoughts": 0,
            "tool": 0,
            "total": 0
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 0);
        assert_eq!(tokens.output, 0);
        // 全零（包括 cached=0）会被 sync 逻辑跳过
        assert!(
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0
        );
    }

    #[test]
    fn test_parse_gemini_tokens_cache_only_not_skipped() {
        // 纯缓存命中消息（input/output/thoughts=0 但 cached>0）不应被跳过
        let json: serde_json::Value = serde_json::json!({
            "input": 0,
            "output": 0,
            "cached": 5000,
            "thoughts": 0
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.cached, 5000);
        // 跳过条件：所有四个字段都为 0 才跳过
        let should_skip =
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0;
        assert!(!should_skip, "纯缓存命中记录不应被跳过");
    }

    #[test]
    fn test_sync_single_gemini_file_does_not_double_count_after_rollup_and_prune(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempfile::tempdir().expect("tempdir");
        let session_path = temp.path().join("session-archived.json");

        fs::write(
            &session_path,
            serde_json::json!({
                "sessionId": "sess-1",
                "messages": [
                    {
                        "id": "msg-old",
                        "type": "gemini",
                        "model": "gemini-2.5-pro",
                        "timestamp": "2020-01-15T10:00:00Z",
                        "tokens": { "input": 100, "output": 25, "cached": 10, "thoughts": 5 }
                    }
                ]
            })
            .to_string(),
        )
        .expect("write initial session");

        let (imported, skipped) = sync_single_gemini_file(&db, &session_path, 0)?;
        assert_eq!((imported, skipped), (1, 0));

        let pruned = db.rollup_and_prune(30)?;
        assert_eq!(pruned, 1);

        {
            let conn = lock_conn!(db.conn);
            let live_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))?;
            let (rollup_reqs, rollup_out): (i64, i64) = conn.query_row(
                "SELECT COALESCE(SUM(request_count), 0), COALESCE(SUM(output_tokens), 0)
                 FROM usage_daily_rollups WHERE app_type = 'gemini'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(live_count, 0);
            assert_eq!(rollup_reqs, 1);
            assert_eq!(rollup_out, 30);
        }

        // User resumes the old session: mtime advances (pass last_modified = 0 to force re-scan)
        // and a second old-timestamp message is appended alongside msg-old.
        fs::write(
            &session_path,
            serde_json::json!({
                "sessionId": "sess-1",
                "messages": [
                    {
                        "id": "msg-old",
                        "type": "gemini",
                        "model": "gemini-2.5-pro",
                        "timestamp": "2020-01-15T10:00:00Z",
                        "tokens": { "input": 100, "output": 25, "cached": 10, "thoughts": 5 }
                    },
                    {
                        "id": "msg-new",
                        "type": "gemini",
                        "model": "gemini-2.5-pro",
                        "timestamp": "2020-01-15T10:05:00Z",
                        "tokens": { "input": 40, "output": 12, "cached": 0, "thoughts": 3 }
                    }
                ]
            })
            .to_string(),
        )
        .expect("write resumed session");

        let (imported_after, skipped_after) = sync_single_gemini_file(&db, &session_path, 0)?;
        assert_eq!(
            (imported_after, skipped_after),
            (1, 1),
            "pruned msg-old must be skipped while msg-new is imported"
        );

        let pruned_second = db.rollup_and_prune(30)?;
        assert_eq!(pruned_second, 1);

        let conn = lock_conn!(db.conn);
        let (rollup_reqs, rollup_out): (i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0), COALESCE(SUM(output_tokens), 0)
             FROM usage_daily_rollups WHERE app_type = 'gemini'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(
            rollup_reqs, 2,
            "archived msg-old must not be double-counted"
        );
        assert_eq!(rollup_out, 45);
        Ok(())
    }

    #[test]
    fn test_insert_gemini_session_updates_retained_entry_in_place() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini_session:sess-live:msg-1";

        let initial = GeminiTokens {
            input: 100,
            output: 20,
            cached: 10,
            thoughts: 5,
        };
        assert!(insert_gemini_session_entry(
            &db,
            request_id,
            &initial,
            "unknown",
            Some("sess-live"),
            Some("2026-04-10T12:00:00Z"),
        )?);

        // Identical re-read is a no-op skip
        assert!(!insert_gemini_session_entry(
            &db,
            request_id,
            &initial,
            "unknown",
            Some("sess-live"),
            Some("2026-04-10T12:00:00Z"),
        )?);

        // Updated token counters and resolved model update the retained row in place
        let updated = GeminiTokens {
            input: 160,
            output: 50,
            cached: 30,
            thoughts: 10,
        };
        assert!(insert_gemini_session_entry(
            &db,
            request_id,
            &updated,
            "gemini-2.5-pro",
            Some("sess-live"),
            Some("2026-04-10T12:00:00Z"),
        )?);

        let conn = lock_conn!(db.conn);
        let (count, model, req_model, inp, out, cached, total_cost): (
            i64,
            String,
            String,
            i64,
            i64,
            i64,
            String,
        ) = conn.query_row(
            "SELECT COUNT(*), model, COALESCE(request_model, ''), input_tokens, output_tokens, cache_read_tokens, total_cost_usd
             FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )?;
        assert_eq!(count, 1);
        assert_eq!(model, "gemini-2.5-pro");
        assert_eq!(req_model, "gemini-2.5-pro");
        assert_eq!(inp, 160);
        assert_eq!(out, 60);
        assert_eq!(cached, 30);
        assert_ne!(total_cost, "0");
        Ok(())
    }

    #[test]
    fn test_legacy_gemini_row_without_dedup_tombstone_is_protected_on_prune_and_never_overwrites_proxy(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let legacy_id = "gemini_session:legacy-sess:msg-legacy";
        let proxy_id = "gemini_session:collision:msg-proxy";

        {
            let conn = lock_conn!(db.conn);
            // Simulate a <= 4.0.31 database row created before session_usage_dedup tombstones
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?1, '_gemini_session', 'gemini', 'gemini-2.5-pro', 'gemini-2.5-pro',
                          80, 20, 0, 0, '0.01', 0, 200, 946684800, 'gemini_session')",
                [legacy_id],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?1, 'google', 'gemini', 'gemini-2.5-pro', 'gemini-2.5-pro',
                          50, 10, 0, 0, '0.02', 50, 200, 1893456000, 'proxy')",
                [proxy_id],
            )?;
        }

        // Pruning old legacy row records its tombstone before deleting detail
        assert_eq!(db.rollup_and_prune(30)?, 1);

        let tokens = GeminiTokens {
            input: 80,
            output: 20,
            cached: 0,
            thoughts: 0,
        };
        assert!(
            !insert_gemini_session_entry(
                &db,
                legacy_id,
                &tokens,
                "gemini-2.5-pro",
                Some("legacy-sess"),
                Some("2000-01-01T00:00:00Z"),
            )?,
            "pruned legacy gemini_session row must not be re-imported"
        );

        // Non-gemini_session row with same request_id is never overwritten
        let changed_tokens = GeminiTokens {
            input: 999,
            output: 999,
            cached: 0,
            thoughts: 0,
        };
        assert!(!insert_gemini_session_entry(
            &db,
            proxy_id,
            &changed_tokens,
            "gemini-2.5-pro",
            Some("collision"),
            Some("2030-01-01T00:00:00Z"),
        )?);

        let conn = lock_conn!(db.conn);
        let proxy_inp: i64 = conn.query_row(
            "SELECT input_tokens FROM proxy_request_logs WHERE request_id = ?1",
            [proxy_id],
            |r| r.get(0),
        )?;
        assert_eq!(proxy_inp, 50);
        Ok(())
    }
}
