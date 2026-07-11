//! Gemini CLI 会话日志使用追踪
//!
//! 从 Gemini CLI JSON 会话和 Antigravity DB 中提取 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.gemini/tmp/*/chats/session-*.json
//! ~/.gemini/{antigravity,antigravity-cli,antigravity-ide}/conversations/*.db
//!   → 解析 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 与 Claude/Codex 解析器的差异
//! - JSON 格式（非 JSONL）：每个文件是单个 JSON 对象，包含 messages 数组
//! - 无需 delta 计算：tokens 字段是 per-message 独立值
//! - 无需状态恢复：不依赖前一条消息的累计值
//! - 天然去重：每条消息有唯一 id 字段

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{
    find_model_pricing, has_matching_antigravity_proxy_usage_log, is_placeholder_pricing_model,
    resolve_antigravity_pricing_placeholder, should_skip_session_insert, DedupKey,
};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    let antigravity_files = collect_antigravity_db_files(&gemini_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: (files.len() + antigravity_files.len()) as u32,
        errors: vec![],
    };

    for file_path in &files {
        match sync_single_gemini_file(db, file_path) {
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

    for file_path in &antigravity_files {
        match sync_single_antigravity_db(db, file_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!(
                    "Antigravity 会话数据库解析失败 {}: {e}",
                    file_path.display()
                );
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

    // 遍历 tmp/<project_hash>/chats/session-*.json
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
                .map(|n| n.starts_with("session-") && n.ends_with(".json"))
                .unwrap_or(false);
            if is_session {
                files.push(path);
            }
        }
    }

    files
}

/// 同步单个 Gemini 会话 JSON 文件，返回 (imported, skipped)
fn sync_single_gemini_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态
    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 读取并解析整个 JSON 文件
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("JSON 解析失败: {e}")))?;

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
    let conn = lock_conn!(db.conn);

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

    let dedup_key = DedupKey {
        app_type: "gemini",
        model,
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_gemini_pricing(&conn, model);
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app("gemini", &usage, &p, multiplier);
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

    // 使用 UPSERT：新记录插入，已存在记录更新 token 和费用（Gemini 全量重读可能携带更新值）
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model",
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
            Some("gemini_session"), // provider_type
            1i64,                // is_streaming
            "1.0",               // cost_multiplier
            created_at,
            "gemini_session",    // data_source
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Gemini 会话日志失败: {e}")))?;

    // changes() > 0 表示新插入或已更新，== 0 表示值完全相同（无实际变更）
    let changed = conn.changes() > 0;
    if changed {
        crate::usage_events::notify_log_recorded();
    }
    Ok(changed)
}

/// 查找 Gemini 模型定价
fn find_gemini_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

// Storage invariant: except for `tempmediaStorage` (temporary media cache),
// session directories and `.db`/`.pb` conversation files in these three roots
// use disjoint UUIDs.
const ANTIGRAVITY_ROOTS: [&str; 3] = ["antigravity", "antigravity-cli", "antigravity-ide"];
// Antigravity keeps rows mutable while the session DB has a running step.
// Observed DB state: status 2 means the step is still running; canceled,
// failed, or completed steps are treated as final and safe to checkpoint.
const ANTIGRAVITY_RUNNING_STATUS: i64 = 2;

#[derive(Debug, Clone)]
enum ProtoValue {
    Varint(u64),
    LengthDelimited(Vec<u8>),
}

struct ProtoParser<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ProtoParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn decode_varint(&mut self) -> Option<u64> {
        let mut result = 0u64;
        let mut shift = 0u32;
        while self.offset < self.data.len() {
            let byte = self.data[self.offset];
            self.offset += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
        None
    }

    fn next_field(&mut self) -> Option<(u32, ProtoValue)> {
        while self.offset < self.data.len() {
            let tag = self.decode_varint()?;
            let field_num = (tag >> 3) as u32;
            let wire_type = (tag & 0x7) as u32;

            match wire_type {
                0 => {
                    return self
                        .decode_varint()
                        .map(|value| (field_num, ProtoValue::Varint(value)));
                }
                1 => {
                    if self.offset + 8 > self.data.len() {
                        return None;
                    }
                    self.offset += 8;
                }
                2 => {
                    let length = self.decode_varint()? as usize;
                    if self.offset + length > self.data.len() {
                        return None;
                    }
                    let blob = &self.data[self.offset..self.offset + length];
                    self.offset += length;
                    return Some((field_num, ProtoValue::LengthDelimited(blob.to_vec())));
                }
                5 => {
                    if self.offset + 4 > self.data.len() {
                        return None;
                    }
                    self.offset += 4;
                }
                _ => return None,
            }
        }
        None
    }

    fn get_varint(&mut self, target_field: u32) -> Option<u64> {
        while let Some((field, value)) = self.next_field() {
            if field == target_field {
                if let ProtoValue::Varint(value) = value {
                    return Some(value);
                }
            }
        }
        None
    }

    fn get_nested(&mut self, target_field: u32) -> Option<Vec<u8>> {
        while let Some((field, value)) = self.next_field() {
            if field == target_field {
                if let ProtoValue::LengthDelimited(val) = value {
                    return Some(val);
                }
            }
        }
        None
    }
}

#[derive(Debug, Default)]
struct AntigravityTokenData {
    // Raw `gen_metadata` field f2: fresh input only. The importer combines it
    // with `cached_tokens` before persistence to match upstream Gemini rows.
    input_tokens: u32,
    output_tokens: u32,
    // Raw `gen_metadata` field f5: cache-read input.
    cached_tokens: u32,
    model: String,
}

impl AntigravityTokenData {
    fn has_tokens(&self) -> bool {
        self.input_tokens != 0 || self.output_tokens != 0 || self.cached_tokens != 0
    }
}

#[derive(Debug, Default)]
struct TrajectoryMetadata {
    session_id: Option<String>,
    created_at_seconds: Option<i64>,
}

#[derive(Debug)]
struct GenMetadataEntry {
    idx: i64,
    token_data: Option<AntigravityTokenData>,
}

#[derive(Debug, Default, Clone, Copy)]
struct AntigravityStepState {
    timestamp: Option<i64>,
}

struct AntigravityStepStates {
    by_gen_idx: HashMap<i64, AntigravityStepState>,
    has_running_step: bool,
}

fn collect_antigravity_db_files(gemini_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in ANTIGRAVITY_ROOTS {
        let conversations_dir = gemini_dir.join(root).join("conversations");
        let entries = match fs::read_dir(&conversations_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("db") {
                files.push(path);
            }
        }
    }
    files
}

fn sync_single_antigravity_db(db: &Database, db_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = db_path.to_string_lossy().to_string();
    let metadata =
        fs::metadata(db_path).map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);
    let wal_path = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }
    let file_modified_secs = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    let (last_modified, last_gen_idx) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let mut agy_conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Config(format!("无法只读打开 Antigravity DB: {e}")))?;

    // 设置 Busy Timeout 以防止并发冲突导致立即报错 SQLITE_BUSY
    let _ = agy_conn.busy_timeout(std::time::Duration::from_secs(2));

    // 使用只读事务包裹多次读取，确保数据视图的一致性快照
    let tx = agy_conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
        .map_err(|e| AppError::Database(format!("无法开启只读事务: {e}")))?;

    let trajectory_meta = read_trajectory_metadata(&tx);
    // `gen_metadata` and `steps` determine whether it is safe to advance the
    // checkpoint. Propagate failures instead of treating them as empty tables:
    // a transient SQLite error must leave this DB eligible for a later retry.
    let step_states = read_step_states(&tx)?;
    let gen_entries = read_gen_metadata_entries(&tx)?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("无法提交 Antigravity 只读事务: {e}")))?;
    if gen_entries.is_empty() {
        if !step_states.has_running_step {
            update_sync_state(db, &file_path_str, file_modified, 0)?;
        }
        return Ok((0, 0));
    }

    let session_id = trajectory_meta
        .as_ref()
        .and_then(|meta| meta.session_id.clone())
        .or_else(|| {
            db_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_string())
        });
    let fallback_created_at = trajectory_meta
        .as_ref()
        .and_then(|meta| meta.created_at_seconds)
        .unwrap_or(file_modified_secs);

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut has_sync_errors = false;
    let max_idx = gen_entries.last().map(|entry| entry.idx + 1).unwrap_or(0);

    for entry in &gen_entries {
        if entry.idx < last_gen_idx {
            continue;
        }
        let Some(token_data) = &entry.token_data else {
            // A missing token payload means this gen_metadata blob did not yet
            // contain usage fields. The checkpoint below only advances when no
            // step is running in the session DB, so mutable/in-flight blobs stay
            // eligible for the next sync pass.
            continue;
        };

        let session_id_str = session_id.as_deref().unwrap_or("unknown");
        let request_id = format!("gemini_antigravity_session:{session_id_str}:{}", entry.idx);
        let created_at = step_states
            .by_gen_idx
            .get(&entry.idx)
            .and_then(|state| state.timestamp)
            .unwrap_or(fallback_created_at);

        match insert_antigravity_session_entry(
            db,
            &request_id,
            token_data,
            session_id.as_deref(),
            created_at,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[GEMINI-SYNC] 插入 Antigravity 会话失败 ({request_id}): {e}");
                skipped += 1;
                has_sync_errors = true;
            }
        }
    }

    // Advance the gen_metadata checkpoint only after Antigravity's session DB
    // has no running steps. This preserves incomplete blobs while status 2 can
    // still be updated with final token data, but avoids replaying finalized
    // canceled/failed/completed rows that never receive usage fields.
    if !step_states.has_running_step && !has_sync_errors {
        update_sync_state(db, &file_path_str, file_modified, max_idx)?;
    }
    Ok((imported, skipped))
}

fn read_trajectory_metadata(conn: &rusqlite::Connection) -> Option<TrajectoryMetadata> {
    let mut stmt = conn
        .prepare("SELECT data FROM trajectory_metadata_blob WHERE id = 'main'")
        .ok()?;
    let data: Vec<u8> = stmt.query_row([], |row| row.get(0)).ok()?;
    let mut parser = ProtoParser::new(&data);
    let mut meta = TrajectoryMetadata::default();

    while let Some((field, value)) = parser.next_field() {
        match (field, value) {
            (2, ProtoValue::LengthDelimited(nested)) => {
                let mut timestamp = ProtoParser::new(&nested);
                meta.created_at_seconds = timestamp.get_varint(1).map(|value| value as i64);
            }
            (3, ProtoValue::LengthDelimited(session_id_bytes)) => {
                meta.session_id = String::from_utf8(session_id_bytes).ok();
            }
            _ => {}
        }
    }

    Some(meta)
}

fn read_gen_metadata_entries(
    conn: &rusqlite::Connection,
) -> Result<Vec<GenMetadataEntry>, AppError> {
    let mut entries = Vec::new();
    let mut stmt = conn
        .prepare("SELECT idx, data FROM gen_metadata ORDER BY idx")
        .map_err(|e| AppError::Database(format!("读取 Antigravity gen_metadata 失败: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            let idx: i64 = row.get(0)?;
            let data: Vec<u8> = row.get(1)?;
            Ok((idx, data))
        })
        .map_err(|e| AppError::Database(format!("查询 Antigravity gen_metadata 失败: {e}")))?;

    for row in rows {
        let (idx, data) = row.map_err(|e| {
            AppError::Database(format!("读取 Antigravity gen_metadata 行失败: {e}"))
        })?;
        entries.push(GenMetadataEntry {
            idx,
            token_data: parse_gen_metadata_blob(&data),
        });
    }

    Ok(entries)
}

fn read_step_states(conn: &rusqlite::Connection) -> Result<AntigravityStepStates, AppError> {
    let mut by_gen_idx = HashMap::new();
    let mut has_running_step = false;
    let mut stmt = conn
        .prepare("SELECT status, metadata FROM steps")
        .map_err(|e| AppError::Database(format!("读取 Antigravity steps 失败: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询 Antigravity steps 失败: {e}")))?;
    for row in rows {
        let (status, metadata) =
            row.map_err(|e| AppError::Database(format!("读取 Antigravity steps 行失败: {e}")))?;
        if status == ANTIGRAVITY_RUNNING_STATUS {
            has_running_step = true;
        }
        let Some(metadata) = metadata else {
            continue;
        };
        if let Some((gen_idx, timestamp)) = parse_step_metadata(&metadata) {
            by_gen_idx
                .entry(gen_idx)
                .and_modify(|state: &mut AntigravityStepState| {
                    if state
                        .timestamp
                        .map(|existing| timestamp < existing)
                        .unwrap_or(true)
                    {
                        state.timestamp = Some(timestamp);
                    }
                })
                .or_insert(AntigravityStepState {
                    timestamp: Some(timestamp),
                });
        }
    }
    Ok(AntigravityStepStates {
        by_gen_idx,
        has_running_step,
    })
}

fn parse_step_metadata(data: &[u8]) -> Option<(i64, i64)> {
    let mut parser = ProtoParser::new(data);
    let mut timestamp: Option<i64> = None;
    let mut gen_idx: Option<i64> = None;

    while let Some((field, value)) = parser.next_field() {
        match (field, value) {
            (1, ProtoValue::LengthDelimited(nested)) => {
                let mut ts = ProtoParser::new(&nested);
                timestamp = ts.get_varint(1).map(|value| value as i64);
            }
            (20, ProtoValue::LengthDelimited(nested)) => {
                let mut f20 = ProtoParser::new(&nested);
                gen_idx = f20.get_varint(3).map(|value| value as i64);
            }
            _ => {}
        }
    }

    match (gen_idx, timestamp) {
        (Some(gen_idx), Some(timestamp)) => Some((gen_idx, timestamp)),
        _ => None,
    }
}

fn parse_gen_metadata_blob(data: &[u8]) -> Option<AntigravityTokenData> {
    let mut parser = ProtoParser::new(data);
    let f1_blob = parser.get_nested(1)?;
    let mut f1 = ProtoParser::new(&f1_blob);
    let mut step_tokens = AntigravityTokenData::default();
    let mut cumulative_tokens = AntigravityTokenData::default();
    let mut model = String::new();

    while let Some((field, value)) = f1.next_field() {
        match (field, value) {
            (4, ProtoValue::LengthDelimited(nested)) => {
                extract_token_fields(&nested, &mut step_tokens);
            }
            (17, ProtoValue::LengthDelimited(nested)) => {
                let mut f17 = ProtoParser::new(&nested);
                if let Some(f2_blob) = f17.get_nested(2) {
                    extract_token_fields(&f2_blob, &mut cumulative_tokens);
                }
            }
            (19, ProtoValue::LengthDelimited(value_bytes)) => {
                if let Ok(value) = String::from_utf8(value_bytes) {
                    model = value;
                }
            }
            (20, ProtoValue::LengthDelimited(nested)) if model.is_empty() => {
                let mut tag_parser = ProtoParser::new(&nested);
                let mut tag_key = None;
                let mut tag_value = None;
                while let Some((field, value)) = tag_parser.next_field() {
                    if let ProtoValue::LengthDelimited(val) = value {
                        match field {
                            1 => tag_key = String::from_utf8(val).ok(),
                            2 => tag_value = String::from_utf8(val).ok(),
                            _ => {}
                        }
                    }
                }
                if tag_key.as_deref() == Some("model_enum") {
                    if let Some(value) = tag_value {
                        model = value;
                    }
                }
            }
            _ => {}
        }
    }

    let mut token_data = if step_tokens.has_tokens() {
        step_tokens
    } else {
        cumulative_tokens
    };
    if !token_data.has_tokens() {
        return None;
    }
    if model.trim().is_empty() {
        model = "unknown".to_string();
    }
    token_data.model = model;
    Some(token_data)
}

fn extract_token_fields(data: &[u8], tokens: &mut AntigravityTokenData) {
    let mut parser = ProtoParser::new(data);
    while let Some((field, value)) = parser.next_field() {
        let ProtoValue::Varint(value) = value else {
            continue;
        };
        match field {
            // Agy `gen_metadata` only: f2 is fresh input, f3 is total output,
            // and f5 is cache-read input. The importer later persists f2 + f5
            // as cache-inclusive Gemini input; this parser intentionally keeps
            // the raw counters separate. This does not describe Agy proxy
            // responses, which are intentionally outside this offline importer.
            2 => tokens.input_tokens = value as u32,
            3 => tokens.output_tokens = value as u32,
            5 => tokens.cached_tokens = value as u32,
            _ => {}
        }
    }
}

/// 归一化 Antigravity 离线会话中的计费模型名称。
///
/// 优先尝试使用 `resolve_antigravity_pricing_placeholder` 将平台级占位符或物理模型 ID
/// 映射至项目中已存在的对应计费模型（例如：将 `gemini-default` 或 `model_placeholder_m187`
/// 解析为 `gemini-3.5-flash`）。若无法解析，则使用旧版规则进行兜底归一化。
fn normalize_antigravity_pricing_model(raw_model: &str) -> String {
    let normalized = raw_model.trim().to_ascii_lowercase();
    // 优先映射占位符和对应的物理模型别名到现存的计费模型
    if let Some(resolved) = resolve_antigravity_pricing_placeholder(&normalized) {
        return resolved;
    }

    let without_thinking = normalized
        .strip_suffix("-thinking")
        .unwrap_or(&normalized)
        .to_string();
    if let Some(base) = without_thinking.strip_suffix("-a") {
        return format!("{base}-preview");
    }
    if let Some(base) = without_thinking.strip_suffix("-b") {
        return format!("{base}-preview");
    }
    without_thinking
}

fn insert_antigravity_session_entry(
    db: &Database,
    request_id: &str,
    token_data: &AntigravityTokenData,
    session_id: Option<&str>,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    // Agy gen_metadata 的 f3 已是完整输出（包含 thinking），无需额外合并 f9/f10。
    let output_tokens = token_data.output_tokens;
    // Persist Antigravity data with the same cache-inclusive input convention
    // used by upstream Gemini proxy/session rows. This keeps shared dashboard
    // aggregations and proxy/session fingerprint de-duplication compatible.
    let input_tokens = token_data
        .input_tokens
        .saturating_add(token_data.cached_tokens);
    // TODO(migration): Rows imported before this normalization retain the old
    // fresh-input-only representation. A data migration is intentionally out
    // of scope; normalizing those rows requires resetting their sync state.
    let raw_model = token_data.model.trim();
    let model = if raw_model.is_empty() {
        "unknown"
    } else {
        raw_model
    };
    let pricing_model = normalize_antigravity_pricing_model(model);

    let dedup_key = DedupKey {
        app_type: "gemini",
        model: &pricing_model,
        input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        created_at,
    };
    // This is the shared Gemini-session/proxy deduplication guard, reused to
    // avoid counting a generic Gemini proxy log and its offline session log
    // twice. It does not parse, infer token semantics for, or promise support
    // for Antigravity-specific proxy responses; this importer only handles
    // Antigravity's offline `gen_metadata` format above.
    if has_matching_antigravity_proxy_usage_log(&conn, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        model: Some(pricing_model.clone()),
        message_id: None,
    };

    let pricing = find_gemini_pricing(&conn, &pricing_model);
    if pricing.is_none() && !is_placeholder_pricing_model(&pricing_model) {
        log::warn!("[GEMINI-SYNC] Antigravity 模型未命中定价: {model} -> {pricing_model}");
    }

    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app("gemini", &usage, &pricing, multiplier);
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

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source, pricing_model
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            request_model = excluded.request_model,
            pricing_model = excluded.pricing_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            created_at = excluded.created_at
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR pricing_model IS NOT excluded.pricing_model
           OR created_at != excluded.created_at",
        rusqlite::params![
            request_id,
            "_gemini_antigravity_session",
            "gemini",
            model,
            model,
            input_tokens,
            output_tokens,
            token_data.cached_tokens,
            0i64,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            session_id.map(|value| value.to_string()),
            Some("antigravity_session"),
            1i64,
            "1.0",
            created_at,
            "antigravity_session",
            pricing_model,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Antigravity 会话日志失败: {e}")))?;

    let changed = conn.changes() > 0;
    if changed {
        crate::usage_events::notify_log_recorded();
    }
    Ok(changed)
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

    fn proto_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    fn proto_varint_field(field: u32, value: u64) -> Vec<u8> {
        let mut out = proto_varint(((field as u64) << 3) | 0);
        out.extend(proto_varint(value));
        out
    }

    fn proto_len_field(field: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut out = proto_varint(((field as u64) << 3) | 2);
        out.extend(proto_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn antigravity_gen_metadata(
        input: u64,
        output: u64,
        cache_read: u64,
        non_thinking_output: u64,
        thinking_output: u64,
    ) -> Vec<u8> {
        let mut usage = proto_varint_field(1, 1016);
        usage.extend(proto_varint_field(2, input));
        usage.extend(proto_varint_field(3, output));
        usage.extend(proto_varint_field(5, cache_read));
        usage.extend(proto_varint_field(9, non_thinking_output));
        usage.extend(proto_varint_field(10, thinking_output));

        let mut metadata = proto_len_field(4, usage);
        metadata.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        proto_len_field(1, metadata)
    }

    #[test]
    fn test_parse_antigravity_usage_uses_f2_f3_f5_only() {
        let data = antigravity_gen_metadata(8_741, 14_479, 28_519, 12_672, 1_807);

        let usage = parse_gen_metadata_blob(&data).expect("Agy usage should parse");
        assert_eq!(usage.input_tokens, 8_741);
        assert_eq!(usage.output_tokens, 14_479);
        assert_eq!(usage.cached_tokens, 28_519);
        assert_eq!(usage.model, "gemini-3-flash-a");
    }

    fn step_metadata(gen_idx: i64, timestamp: i64) -> Vec<u8> {
        let ts = proto_varint_field(1, timestamp as u64);
        let gen_ref = proto_varint_field(3, gen_idx as u64);
        let mut out = proto_len_field(1, ts);
        out.extend(proto_len_field(20, gen_ref));
        out
    }

    #[test]
    fn test_read_step_states_only_status_two_protects_running() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE steps (status INTEGER, metadata BLOB);")
            .unwrap();
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2), (?3, ?4)",
            rusqlite::params![6, step_metadata(1, 100), 7, step_metadata(2, 200)],
        )
        .unwrap();

        let states = read_step_states(&conn).expect("read step states");
        assert!(!states.has_running_step);
        assert_eq!(
            states.by_gen_idx.get(&1).and_then(|s| s.timestamp),
            Some(100)
        );
        assert_eq!(
            states.by_gen_idx.get(&2).and_then(|s| s.timestamp),
            Some(200)
        );

        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2)",
            rusqlite::params![ANTIGRAVITY_RUNNING_STATUS, step_metadata(3, 300)],
        )
        .unwrap();
        let states = read_step_states(&conn).expect("read step states");
        assert!(states.has_running_step);
        assert_eq!(
            states.by_gen_idx.get(&3).and_then(|s| s.timestamp),
            Some(300)
        );
    }

    #[test]
    fn test_sync_antigravity_db_does_not_advance_while_running() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| AppError::Database(format!("open fixture db: {e}")))?;
            conn.execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE steps (status INTEGER NOT NULL DEFAULT 0, metadata BLOB);",
            )
            .map_err(|e| AppError::Database(format!("create fixture schema: {e}")))?;
            conn.execute(
                "INSERT INTO steps VALUES (?1, ?2)",
                rusqlite::params![ANTIGRAVITY_RUNNING_STATUS, step_metadata(1, 100)],
            )
            .map_err(|e| AppError::Database(format!("insert fixture step: {e}")))?;
        }

        let db = Database::memory()?;
        let (imported, skipped) = sync_single_antigravity_db(&db, &db_path)?;
        assert_eq!((imported, skipped), (0, 0));

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert_eq!((last_modified, last_offset), (0, 0));
        Ok(())
    }

    #[test]
    fn test_sync_antigravity_db_does_not_checkpoint_when_step_read_fails() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| AppError::Database(format!("open fixture db: {e}")))?;
            // Deliberately omit `steps` to model an unavailable/incomplete
            // Antigravity schema. A read failure must not be acknowledged.
            conn.execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);",
            )
            .map_err(|e| AppError::Database(format!("create fixture schema: {e}")))?;
        }

        let db = Database::memory()?;
        assert!(sync_single_antigravity_db(&db, &db_path).is_err());

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert_eq!((last_modified, last_offset), (0, 0));
        Ok(())
    }

    #[test]
    fn test_sync_antigravity_db_advances_for_canceled_or_failed_steps() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| AppError::Database(format!("open fixture db: {e}")))?;
            conn.execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE steps (status INTEGER NOT NULL DEFAULT 0, metadata BLOB);",
            )
            .map_err(|e| AppError::Database(format!("create fixture schema: {e}")))?;
            conn.execute(
                "INSERT INTO steps VALUES (?1, ?2), (?3, ?4)",
                rusqlite::params![6, step_metadata(1, 100), 7, step_metadata(2, 200)],
            )
            .map_err(|e| AppError::Database(format!("insert fixture steps: {e}")))?;
        }

        let db = Database::memory()?;
        let (imported, skipped) = sync_single_antigravity_db(&db, &db_path)?;
        assert_eq!((imported, skipped), (0, 0));

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert!(last_modified > 0);
        assert_eq!(last_offset, 0);
        Ok(())
    }

    #[test]
    fn test_insert_antigravity_session_entry_upserts_existing_request() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini-antigravity-session-upsert";
        let first = AntigravityTokenData {
            input_tokens: 10,
            output_tokens: 2,
            cached_tokens: 1,
            model: "gemini-3-pro-b".to_string(),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &first,
            Some("agy-session"),
            1000,
        )?);

        let second = AntigravityTokenData {
            input_tokens: 20,
            output_tokens: 7,
            cached_tokens: 2,
            model: "gemini-3-flash-a-thinking".to_string(),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &second,
            Some("agy-session"),
            2000,
        )?);

        let conn = lock_conn!(db.conn);
        let row: (i64, i64, i64, String, String, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, model, pricing_model, created_at
             FROM proxy_request_logs WHERE request_id = ?1",
            rusqlite::params![request_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        assert_eq!(
            row,
            (
                22,
                7,
                2,
                "gemini-3-flash-a-thinking".to_string(),
                "gemini-3.5-flash".to_string(),
                2000
            )
        );

        Ok(())
    }

    #[test]
    fn test_normalize_antigravity_pricing_model_aliases() {
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-3-flash-a-thinking"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-3-pro-b"),
            "gemini-3-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-pro-default"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-pro-default-thinking"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M187"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M20"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M132"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M36"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M16"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M35"),
            "claude-sonnet-4-6-20260217"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M26"),
            "claude-opus-4-6-20260206"
        );
        assert_eq!(normalize_antigravity_pricing_model("unknown"), "unknown");
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M999"),
            "unknown"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("model_placeholder_custom"),
            "unknown"
        );
    }
}
