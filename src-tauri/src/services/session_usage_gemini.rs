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
use crate::gemini_config::{get_gemini_dir, ANTIGRAVITY_ROOTS};
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{
    find_model_pricing, has_matching_proxy_usage_log, is_placeholder_pricing_model,
    resolve_antigravity_pricing, should_skip_session_insert, DedupKey,
};
use crate::session_manager::providers::gemini::{is_session_file, parse_session_document};
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

/// 同步 Gemini 使用数据（从 JSON 会话日志和 Antigravity DB）
pub fn sync_gemini_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let gemini_dir = get_gemini_dir();

    let files = collect_gemini_session_files(&gemini_dir);
    let antigravity_files = collect_antigravity_db_files(&gemini_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: (files.len() + antigravity_files.len()) as u32,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    if files.is_empty() && antigravity_files.is_empty() {
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
        cache_creation_1h_tokens: 0,
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
    Ok(changed)
}

/// 查找 Gemini 模型定价
fn find_gemini_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

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
        for index in 0..10 {
            let byte = *self.data.get(self.offset)?;
            self.offset += 1;
            if index == 9 && byte > 1 {
                return None;
            }
            result |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Some(result);
            }
        }
        None
    }

    fn next_field(&mut self) -> Option<(u32, ProtoValue)> {
        while self.offset < self.data.len() {
            let tag = u32::try_from(self.decode_varint()?).ok()?;
            let field_num = tag >> 3;
            let wire_type = tag & 0x7;
            if field_num == 0 {
                return None;
            }

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
                    let length = usize::try_from(self.decode_varint()?).ok()?;
                    let end = match self.offset.checked_add(length) {
                        Some(end) if end <= self.data.len() => end,
                        _ => return None,
                    };
                    let blob = &self.data[self.offset..end];
                    self.offset = end;
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

#[derive(Debug, Default, Clone)]
pub(crate) struct AntigravityTokenData {
    // Raw `gen_metadata` field f2: fresh input only. The importer combines it
    // with `cached_tokens` before persistence to match upstream Gemini rows.
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
    // Raw `gen_metadata` field f5: cache-read input.
    pub(crate) cached_tokens: u32,
    pub(crate) model: String,
    pub(crate) duration_ms: Option<i64>,
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
    let db_path_str = db_path.to_string_lossy();
    let wal_path = PathBuf::from(format!("{}-wal", db_path_str));
    let shm_path = PathBuf::from(format!("{}-shm", db_path_str));
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }
    if let Ok(shm_meta) = fs::metadata(&shm_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&shm_meta));
    }
    let file_modified_secs = (file_modified / 1_000_000_000) as i64;

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

pub(crate) fn parse_step_gen_idx(data: &[u8]) -> Option<i64> {
    let mut parser = ProtoParser::new(data);
    while let Some((field, value)) = parser.next_field() {
        if field == 20 {
            if let ProtoValue::LengthDelimited(nested) = value {
                let mut f20 = ProtoParser::new(&nested);
                return Some(f20.get_varint(3).unwrap_or(0) as i64);
            }
        }
    }
    None
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
                gen_idx = Some(f20.get_varint(3).unwrap_or(0) as i64);
            }
            _ => {}
        }
    }

    match (gen_idx, timestamp) {
        (Some(gen_idx), Some(timestamp)) => Some((gen_idx, timestamp)),
        _ => None,
    }
}

pub(crate) fn parse_gen_metadata_blob(data: &[u8]) -> Option<AntigravityTokenData> {
    let mut parser = ProtoParser::new(data);
    let f1_blob = parser.get_nested(1)?;
    let mut f1 = ProtoParser::new(&f1_blob);
    let mut step_tokens = AntigravityTokenData::default();
    let mut cumulative_tokens = AntigravityTokenData::default();
    let mut model = String::new();
    let mut duration_ms: Option<i64> = None;

    while let Some((field, value)) = f1.next_field() {
        match (field, value) {
            (4, ProtoValue::LengthDelimited(nested)) => {
                extract_token_fields(&nested, &mut step_tokens);
            }
            (11, ProtoValue::LengthDelimited(nested)) => {
                let mut dur_parser = ProtoParser::new(&nested);
                let mut seconds: i64 = 0;
                let mut nanos: i64 = 0;
                while let Some((field, value)) = dur_parser.next_field() {
                    if let ProtoValue::Varint(val) = value {
                        match field {
                            1 => seconds = val as i64,
                            2 => nanos = val as i64,
                            _ => {}
                        }
                    }
                }
                let ms = seconds
                    .saturating_mul(1000)
                    .saturating_add(nanos / 1_000_000);
                if (100..=3_600_000).contains(&ms) {
                    duration_ms = Some(ms);
                }
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
    token_data.duration_ms = duration_ms;
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

#[cfg(test)]
fn normalize_antigravity_pricing_model(conn: &rusqlite::Connection, raw_model: &str) -> String {
    resolve_antigravity_pricing(conn, raw_model).1
}

fn insert_antigravity_session_entry(
    db: &Database,
    request_id: &str,
    token_data: &AntigravityTokenData,
    session_id: Option<&str>,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| AppError::Database(format!("启动 Antigravity 会话导入事务失败: {e}")))?;

    let ledger_exists: bool = tx
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM session_usage_dedup
                WHERE data_source = 'antigravity_session' AND request_id = ?1
            )",
            rusqlite::params![request_id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(format!("查询 Antigravity 去重账本失败: {e}")))?;

    let detail_exists: bool = tx
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM proxy_request_logs
                WHERE request_id = ?1
            )",
            rusqlite::params![request_id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(format!("查询 Antigravity 会话明细失败: {e}")))?;

    // 明细已被聚合清理但持久账本存在：阻止重复导入与双算
    if ledger_exists && !detail_exists {
        return Ok(false);
    }

    let output_tokens = token_data.output_tokens;
    let input_tokens = token_data
        .input_tokens
        .saturating_add(token_data.cached_tokens);
    let raw_model = token_data.model.trim();
    let model = if raw_model.is_empty() {
        "unknown"
    } else {
        raw_model
    };

    let (pricing, pricing_model) = resolve_antigravity_pricing(&tx, model);

    let dedup_key = DedupKey {
        app_type: "gemini",
        model: &pricing_model,
        input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        created_at,
    };

    if !detail_exists && has_matching_proxy_usage_log(&tx, &dedup_key)? {
        tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup
             (data_source, request_id, semantic_id, has_entry_id)
             VALUES ('antigravity_session', ?1, ?1, 1)",
            rusqlite::params![request_id],
        )
        .map_err(|e| AppError::Database(format!("写入 Antigravity 去重账本失败: {e}")))?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Antigravity 代理跳过事务失败: {e}")))?;
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        model: Some(pricing_model.clone()),
        message_id: None,
    };

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

    // A running generation may finish its timing after its tokens are present.
    // Persist both in this same UPSERT, including timing-only changes. There is
    // no separate latency read/update or historical backfill pass; the existing
    // running-step checkpoint controls which source generations are replayed.
    let upserted = tx.execute(
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
            latency_ms = CASE WHEN excluded.latency_ms > 0 THEN excluded.latency_ms ELSE proxy_request_logs.latency_ms END,
            created_at = excluded.created_at
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR pricing_model IS NOT excluded.pricing_model
           OR created_at != excluded.created_at
           OR (excluded.latency_ms > 0 AND excluded.latency_ms != proxy_request_logs.latency_ms)",
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
            token_data.duration_ms.unwrap_or(0),
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

    tx.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES ('antigravity_session', ?1, ?1, 1)",
        rusqlite::params![request_id],
    )
    .map_err(|e| AppError::Database(format!("写入 Antigravity 去重账本失败: {e}")))?;

    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Antigravity 会话导入事务失败: {e}")))?;

    let changed = upserted > 0;
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
        let mut out = proto_varint((field as u64) << 3);
        out.extend(proto_varint(value));
        out
    }

    fn proto_len_field(field: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut out = proto_varint(((field as u64) << 3) | 2);
        out.extend(proto_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    #[test]
    fn antigravity_usage_parser_rejects_overflowing_varints_and_tags() {
        for value in [0, 127, 128, u64::MAX] {
            let bytes = proto_varint(value);
            assert_eq!(ProtoParser::new(&bytes).decode_varint(), Some(value));
        }
        for bytes in [
            vec![0xff; 9],
            [vec![0xff; 9], vec![2]].concat(),
            [vec![0x80; 10], vec![0]].concat(),
        ] {
            assert!(ProtoParser::new(&bytes).decode_varint().is_none());
        }
        // An oversized tag must not wrap into field 2 (the input token field).
        let mut oversized_tag = proto_varint((u64::from(u32::MAX) + 3) << 3);
        oversized_tag.extend(proto_varint(10));
        assert!(ProtoParser::new(&oversized_tag).next_field().is_none());
        assert!(ProtoParser::new(&[0, 10]).next_field().is_none());

        let mut oversized_length = vec![0x0a];
        oversized_length.extend(proto_varint(u64::MAX));
        assert!(ProtoParser::new(&oversized_length).next_field().is_none());
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
        assert_eq!(usage.duration_ms, None);
    }

    #[test]
    fn test_parse_antigravity_usage_extracts_duration_from_field_11() {
        let mut usage = proto_varint_field(1, 1016);
        usage.extend(proto_varint_field(2, 100));
        usage.extend(proto_varint_field(3, 200));

        let mut dur = proto_varint_field(1, 5); // 5s
        dur.extend(proto_varint_field(2, 244_000_000)); // 244ms

        let mut metadata = proto_len_field(4, usage.clone());
        metadata.extend(proto_len_field(11, dur));
        metadata.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        let data = proto_len_field(1, metadata);

        let parsed = parse_gen_metadata_blob(&data).expect("Agy usage should parse");
        assert_eq!(parsed.duration_ms, Some(5244));

        // 仅包含 nanos（seconds = 0 省略），应正确解析为 500ms
        let dur_nanos_only = proto_varint_field(2, 500_000_000);
        let mut meta_nanos = proto_len_field(4, usage.clone());
        meta_nanos.extend(proto_len_field(11, dur_nanos_only));
        meta_nanos.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        let data_nanos = proto_len_field(1, meta_nanos);
        let parsed_nanos = parse_gen_metadata_blob(&data_nanos).expect("Agy usage should parse");
        assert_eq!(parsed_nanos.duration_ms, Some(500));

        // 字段顺序为 nanos 在前、seconds 在后，应正确累加为 5244ms
        let mut dur_rev = proto_varint_field(2, 244_000_000);
        dur_rev.extend(proto_varint_field(1, 5));
        let mut meta_rev = proto_len_field(4, usage);
        meta_rev.extend(proto_len_field(11, dur_rev));
        meta_rev.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        let data_rev = proto_len_field(1, meta_rev);
        let parsed_rev = parse_gen_metadata_blob(&data_rev).expect("Agy usage should parse");
        assert_eq!(parsed_rev.duration_ms, Some(5244));
    }

    #[test]
    fn test_parse_antigravity_usage_rejects_out_of_range_duration() {
        let mut usage = proto_varint_field(1, 1016);
        usage.extend(proto_varint_field(2, 100));
        usage.extend(proto_varint_field(3, 200));

        // 50ms is too short (< 100ms)
        let dur_short = proto_varint_field(2, 50_000_000);

        let mut metadata_short = proto_len_field(4, usage.clone());
        metadata_short.extend(proto_len_field(11, dur_short));
        metadata_short.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        let data_short = proto_len_field(1, metadata_short);

        let parsed_short = parse_gen_metadata_blob(&data_short).expect("Agy usage should parse");
        assert_eq!(parsed_short.duration_ms, None);

        // 2 hours is too long (> 3600s)
        let dur_long = proto_varint_field(1, 7200);

        let mut metadata_long = proto_len_field(4, usage);
        metadata_long.extend(proto_len_field(11, dur_long));
        metadata_long.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        let data_long = proto_len_field(1, metadata_long);

        let parsed_long = parse_gen_metadata_blob(&data_long).expect("Agy usage should parse");
        assert_eq!(parsed_long.duration_ms, None);
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
        assert_eq!(states.by_gen_idx.len(), 2);
    }

    #[test]
    fn test_sync_antigravity_db_skips_unmodified() -> Result<(), AppError> {
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
        }

        let db = Database::memory()?;
        let key = db_path.to_string_lossy().to_string();
        update_sync_state(&db, &key, 1_000_000_000_000, 0)?;

        let (imported, skipped) = sync_single_antigravity_db(&db, &db_path)?;
        assert_eq!((imported, skipped), (0, 0));
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
            duration_ms: None,
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
            duration_ms: Some(1500),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &second,
            Some("agy-session"),
            2000,
        )?);

        let conn = lock_conn!(db.conn);
        let row: (i64, i64, i64, String, String, i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, model, pricing_model, created_at, latency_ms
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
                    row.get(6)?,
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
                2000,
                1500
            )
        );

        Ok(())
    }

    #[test]
    fn test_normalize_antigravity_pricing_model_aliases() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3-flash-a-thinking"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-pro-default"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-pro-default-thinking"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M187"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M20"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M132"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M36"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M16"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M35"),
            "claude-sonnet-4-6-20260217"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M26"),
            "claude-opus-4-6-20260206"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.8flash"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.8-flash"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.8"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.8"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.8-flash-a"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.8-flash-tiered"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.8-flash-n"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.8-flash-thinking"),
            "gemini-3.8-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.7flash"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.7-flash"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.7"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.7"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.7-flash-exp-b"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.7-flash-thinking"),
            "gemini-3.7-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.6flash"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.6-flash"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "3.6"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.6"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.6-flash-exp-a"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "gemini-3.6-flash-thinking"),
            "gemini-3.6-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "unknown"),
            "unknown"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "MODEL_PLACEHOLDER_M999"),
            "model_placeholder_m999"
        );
        assert_eq!(
            normalize_antigravity_pricing_model(&conn, "model_placeholder_custom"),
            "model_placeholder_custom"
        );
        Ok(())
    }

    #[test]
    fn test_antigravity_dedup_ledger_blocks_reinsertion_after_detail_pruning(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini_antigravity_session:test-session:0";
        let token_data = AntigravityTokenData {
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 20,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: None,
        };

        // 1. 首次导入：写入明细与去重账本
        let imported = insert_antigravity_session_entry(
            &db,
            request_id,
            &token_data,
            Some("test-session"),
            10_000,
        )?;
        assert!(imported);

        {
            let conn = lock_conn!(db.conn);
            let detail_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| r.get(0),
            )?;
            assert_eq!(detail_count, 1);
            let ledger_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'antigravity_session' AND request_id = ?1",
                [request_id],
                |r| r.get(0),
            )?;
            assert_eq!(ledger_count, 1);
        }

        // 2. 模拟聚合清理 (rollup_and_prune)：删除明细行，但保留 session_usage_dedup 账本
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "DELETE FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
            )?;
        }

        // 3. 再次同步（例如由于其他步骤仍为 status 2 导致未推进检查点）：
        // 持久账本阻止已聚合事件重新插入，避免双算
        let re_imported = insert_antigravity_session_entry(
            &db,
            request_id,
            &token_data,
            Some("test-session"),
            10_000,
        )?;
        assert!(!re_imported, "明细被清理但账本存在时，必须阻止重新插入");

        {
            let conn = lock_conn!(db.conn);
            let detail_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| r.get(0),
            )?;
            assert_eq!(detail_count, 0, "明细表不得被重新插入");
        }

        Ok(())
    }

    #[test]
    fn test_antigravity_status_two_step_allows_final_token_upsert() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini_antigravity_session:running-session:1";

        // 步骤尚在运行（status 2）时，先记录初始中间 token
        let initial_tokens = AntigravityTokenData {
            input_tokens: 10,
            output_tokens: 2,
            cached_tokens: 0,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: None,
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &initial_tokens,
            Some("running-session"),
            1_000,
        )?);

        // 步骤完成或推进时更新为最终 token
        let final_tokens = AntigravityTokenData {
            input_tokens: 80,
            output_tokens: 40,
            cached_tokens: 20,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: None,
        };
        let updated = insert_antigravity_session_entry(
            &db,
            request_id,
            &final_tokens,
            Some("running-session"),
            1_000,
        )?;
        assert!(updated, "最终 token 变更时应成功更新");

        {
            let conn = lock_conn!(db.conn);
            let (input, output, cache_read): (i64, i64, i64) = conn.query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            // 80 + 20 cached = 100 cache-inclusive input
            assert_eq!(input, 100);
            assert_eq!(output, 40);
            assert_eq!(cache_read, 20);
        }

        // 再次相同同步，无变化返回 false
        let unchanged = insert_antigravity_session_entry(
            &db,
            request_id,
            &final_tokens,
            Some("running-session"),
            1_000,
        )?;
        assert!(!unchanged);

        Ok(())
    }

    #[test]
    fn test_antigravity_session_entry_updates_existing_positive_latency() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "latency-update-test";
        let initial_tokens = AntigravityTokenData {
            input_tokens: 100,
            output_tokens: 400,
            cached_tokens: 0,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: Some(5000),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &initial_tokens,
            Some("session-lat"),
            1_000,
        )?);

        {
            let conn = lock_conn!(db.conn);
            let lat: i64 = conn.query_row(
                "SELECT latency_ms FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| r.get(0),
            )?;
            assert_eq!(lat, 5000);
        }

        // 仅耗时修正：从 5000ms 修正为 8000ms，其他字段保持不变
        let corrected_tokens = AntigravityTokenData {
            input_tokens: 100,
            output_tokens: 400,
            cached_tokens: 0,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: Some(8000),
        };
        let updated = insert_antigravity_session_entry(
            &db,
            request_id,
            &corrected_tokens,
            Some("session-lat"),
            1_000,
        )?;
        assert!(updated, "已有正数耗时发生修正时应成功更新");

        {
            let conn = lock_conn!(db.conn);
            let lat: i64 = conn.query_row(
                "SELECT latency_ms FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| r.get(0),
            )?;
            assert_eq!(lat, 8000, "数据库应更新为修正后的 8000ms");
        }

        Ok(())
    }

    #[test]
    fn test_antigravity_proxy_first_records_ledger_and_skips_detail() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 1. 代理日志先到达
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source, pricing_model
                ) VALUES ('proxy-early', 'google', 'gemini', 'gemini-3.5-flash', 'gemini-3.5-flash', 100, 50, 20, 0, '0.05', 200, 200, 10000, 'proxy', 'gemini-3.5-flash')",
                [],
            )?;
        }

        // 2. 会话同步后到达（完全相同的指纹：80 fresh + 20 cached = 100 cache-inclusive input）
        let token_data = AntigravityTokenData {
            input_tokens: 80,
            output_tokens: 50,
            cached_tokens: 20,
            model: "gemini-3.5-flash".to_string(),
            duration_ms: None,
        };
        let imported = insert_antigravity_session_entry(
            &db,
            "session-req-distinct",
            &token_data,
            Some("proxy-first"),
            10_000,
        )?;
        assert!(!imported, "代理日志命中时跳过会话明细插入");

        // 3. 验证去重账本已记录，即使代理日志后续被清理也不会重新插入
        {
            let conn = lock_conn!(db.conn);
            let ledger_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'antigravity_session' AND request_id = 'session-req-distinct'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(ledger_count, 1);

            // 清理代理日志
            conn.execute(
                "DELETE FROM proxy_request_logs WHERE data_source = 'proxy'",
                [],
            )?;
        }

        let re_imported = insert_antigravity_session_entry(
            &db,
            "session-req-distinct",
            &token_data,
            Some("proxy-first"),
            10_000,
        )?;
        assert!(!re_imported, "代理日志清理后，持久账本阻止重新插入");

        Ok(())
    }

    #[test]
    fn test_antigravity_pricing_custom_pricing_precedence_and_claude_5_5() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        // 1. 用户对 claude-sonnet-4-6 设置了自定义价格 $1.00
        conn.execute(
            "UPDATE model_pricing SET input_cost_per_million = '1.0' WHERE model_id = 'claude-sonnet-4-6'",
            [],
        )?;

        let (pricing, pricing_model) = resolve_antigravity_pricing(&conn, "claude-sonnet-4-6");
        assert_eq!(pricing_model, "claude-sonnet-4-6");
        assert_eq!(
            pricing.expect("pricing").input_cost_per_million,
            Decimal::from(1)
        );

        // 2. claude-opus-5-5 命中已种子价格 ($4.00)
        let (opus_pricing, opus_model) = resolve_antigravity_pricing(&conn, "claude-opus-5-5");
        assert_eq!(opus_model, "claude-opus-5-5");
        assert_eq!(
            opus_pricing.expect("opus pricing").input_cost_per_million,
            Decimal::from(4)
        );

        // 3. claude-opus-5.5-thinking 剥离 -thinking 并命中 claude-opus-5-5
        let (opus_think_pricing, opus_think_model) =
            resolve_antigravity_pricing(&conn, "claude-opus-5.5-thinking");
        assert_eq!(opus_think_model, "claude-opus-5-5");
        assert_eq!(
            opus_think_pricing
                .expect("opus think")
                .input_cost_per_million,
            Decimal::from(4)
        );

        // 4. claude-sonnet-5.5 命中已种子官方价格 ($2.00)
        let (sonnet_55_pricing, sonnet_55_model) =
            resolve_antigravity_pricing(&conn, "claude-sonnet-5.5");
        assert_eq!(sonnet_55_model, "claude-sonnet-5-5");
        assert_eq!(
            sonnet_55_pricing
                .expect("sonnet 5.5 pricing")
                .input_cost_per_million,
            Decimal::from(2)
        );

        // 未知版本如 claude-sonnet-5.6 保留完整版本号与家族，不降级为 5.5 或 5，且由于无内置价格返回未定价 (None)
        let (sonnet_56_pricing, sonnet_56_model) =
            resolve_antigravity_pricing(&conn, "claude-sonnet-5.6");
        assert_eq!(sonnet_56_model, "claude-sonnet-5-6");
        assert!(sonnet_56_pricing.is_none(), "无官方价格条目时不得猜测价格");

        // 5. 未知占位符保留原始 ID，不产生价格
        let (m999_pricing, m999_model) =
            resolve_antigravity_pricing(&conn, "model_placeholder_m999");
        assert_eq!(m999_model, "model_placeholder_m999");
        assert!(m999_pricing.is_none());

        Ok(())
    }
    #[test]
    fn antigravity_future_flash_versions_require_only_price_entries() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
            output_cost_per_million, cache_read_cost_per_million, cache_creation_cost_per_million)
            VALUES ('gemini-9.12-flash', 'Future fixture', '7', '8', '1', '0')",
            [],
        )?;
        for model in [
            "9.12flash",
            "9.12-flash",
            "flash-9.12",
            "gemini-9.12-flash-high",
            "gemini-9.12-flash-tiered",
            "gemini-9.12-flash-thinking",
        ] {
            let (price, resolved) = resolve_antigravity_pricing(&conn, model);
            assert_eq!(resolved, "gemini-9.12-flash", "{model}");
            assert_eq!(price.unwrap().input_cost_per_million, Decimal::from(7));
        }
        for model in [
            "9.12",
            "gemini-9.12",
            "gemini-9.12-flash-preview",
            "gemini-9.12-flash-a",
            "gemini-9.12-flash-pro",
            "gemini-9.12-flash-fast",
            "gemini-9.12-flash-n",
            "gemini-9.13-flash-high",
            "gemini-9-flash-high",
        ] {
            assert!(
                resolve_antigravity_pricing(&conn, model).0.is_none(),
                "{model}"
            );
        }
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
            output_cost_per_million, cache_read_cost_per_million, cache_creation_cost_per_million)
            VALUES ('gemini-9.12-flash-high', 'Custom variant', '11', '8', '1', '0')",
            [],
        )?;
        let (price, resolved) = resolve_antigravity_pricing(&conn, "gemini-9.12-flash-high");
        assert_eq!(resolved, "gemini-9.12-flash-high");
        assert_eq!(price.unwrap().input_cost_per_million, Decimal::from(11));
        Ok(())
    }
    #[test]
    fn antigravity_unpriced_aliases_can_be_backfilled_after_prices_are_added(
    ) -> Result<(), AppError> {
        for (raw_model, canonical_model) in [
            ("9.12flash", "gemini-9.12-flash"),
            ("model_placeholder_m187", "gemini-3.5-flash"),
            ("gemini-3.8-flash-n", "gemini-3.8-flash"),
        ] {
            let db = Database::memory()?;
            {
                let conn = lock_conn!(db.conn);
                conn.execute(
                    "DELETE FROM model_pricing WHERE model_id = ?1",
                    [canonical_model],
                )?;
            }
            let tokens = AntigravityTokenData {
                input_tokens: 80,
                output_tokens: 50,
                cached_tokens: 20,
                model: raw_model.to_string(),
                duration_ms: None,
            };
            assert!(insert_antigravity_session_entry(
                &db,
                "unpriced-request",
                &tokens,
                Some("unpriced-session"),
                10_000,
            )?);
            {
                let conn = lock_conn!(db.conn);
                conn.execute(
                    "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million,
                     output_cost_per_million, cache_read_cost_per_million, cache_creation_cost_per_million)
                     VALUES (?1, 'Added later', '7', '8', '1', '0')",
                    [canonical_model],
                )?;
            }
            assert_eq!(
                db.backfill_missing_usage_costs_for_model(canonical_model)?,
                1,
                "{raw_model}"
            );
            let conn = lock_conn!(db.conn);
            let (pricing_model, cost): (String, String) = conn.query_row(
                "SELECT pricing_model, total_cost_usd FROM proxy_request_logs WHERE request_id = 'unpriced-request'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(pricing_model, canonical_model);
            assert_eq!(
                Decimal::from_str_exact(&cost).unwrap(),
                Decimal::from_str_exact("0.00098").unwrap()
            );
        }
        Ok(())
    }

    fn antigravity_sync_fixture(
        path: &Path,
        status: i64,
    ) -> Result<rusqlite::Connection, AppError> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE steps (status INTEGER, metadata BLOB);",
        )?;
        conn.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (0, ?1)",
            [antigravity_gen_metadata(80, 50, 20, 50, 0)],
        )?;
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2)",
            rusqlite::params![status, step_metadata(0, 10_000)],
        )?;
        Ok(conn)
    }

    fn antigravity_rollup_totals(db: &Database) -> Result<(i64, f64), AppError> {
        let conn = lock_conn!(db.conn);
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0),
            COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) FROM usage_daily_rollups",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    #[test]
    fn antigravity_sync_updates_timing_without_token_changes() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("timing.db");
        let source = antigravity_sync_fixture(&path, 2)?;
        let blob = |duration_ms: Option<u64>| {
            let data = antigravity_gen_metadata(80, 400, 20, 400, 0);
            let mut metadata = ProtoParser::new(&data).get_nested(1).unwrap();
            if let Some(ms) = duration_ms {
                let mut duration = proto_varint_field(1, ms / 1000);
                duration.extend(proto_varint_field(2, (ms % 1000) * 1_000_000));
                metadata.extend(proto_len_field(11, duration));
            }
            proto_len_field(1, metadata)
        };
        source.execute("UPDATE gen_metadata SET data = ?1", [blob(None)])?;
        let db = Database::memory()?;
        let key = path.to_string_lossy();
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
        assert_eq!(get_sync_state(&db, &key)?, (0, 0));
        let initial_cost: String = {
            let conn = lock_conn!(db.conn);
            conn.query_row("SELECT total_cost_usd FROM proxy_request_logs", [], |r| {
                r.get(0)
            })?
        };

        // Tokens stay identical: missing timing and then corrected positive
        // timing must each update the existing row through the real sync path.
        for duration in [5000, 8000] {
            source.execute("UPDATE gen_metadata SET data = ?1", [blob(Some(duration))])?;
            assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
            assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
            assert_eq!(get_sync_state(&db, &key)?, (0, 0));
            let stats = db.get_provider_stats(None, None, None, None, None)?;
            assert_eq!(stats.len(), 1);
            assert_eq!(stats[0].request_count, 1);
            assert_eq!(stats[0].est_speed_output_tokens, 400);
            assert_eq!(stats[0].est_speed_duration_ms, duration);
            assert_eq!(stats[0].speed_output_tokens, 0);
            let conn = lock_conn!(db.conn);
            let row: (i64, i64, String, i64) = conn.query_row(
                "SELECT input_tokens, output_tokens, total_cost_usd, latency_ms FROM proxy_request_logs",
                [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            assert_eq!(row, (100, 400, initial_cost.clone(), duration as i64));
            let ledger_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM session_usage_dedup", [], |r| r.get(0))?;
            assert_eq!(ledger_count, 1);
        }

        source.execute("UPDATE steps SET status = 3", [])?;
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        let (modified, offset) = get_sync_state(&db, &key)?;
        assert!(modified > 0);
        assert_eq!(offset, 1);
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 0));
        Ok(())
    }

    #[test]
    fn antigravity_running_sync_final_tokens_and_real_rollup_are_idempotent() -> Result<(), AppError>
    {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("running.db");
        let source = antigravity_sync_fixture(&path, 2)?;
        let db = Database::memory()?;
        let key = path.to_string_lossy();
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
        assert_eq!(get_sync_state(&db, &key)?, (0, 0));
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        source.execute(
            "UPDATE gen_metadata SET data = ?1 WHERE idx = 0",
            [antigravity_gen_metadata(120, 70, 30, 70, 0)],
        )?;
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
        {
            let conn = lock_conn!(db.conn);
            let tokens: (i64, i64, i64) = conn.query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens FROM proxy_request_logs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(tokens, (150, 70, 30));
        }
        assert_eq!(db.rollup_and_prune(30)?, 1);
        let totals = antigravity_rollup_totals(&db)?;
        assert_eq!(totals.0, 1);
        assert!(totals.1 > 0.0);
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        assert_eq!(db.rollup_and_prune(30)?, 0);
        assert_eq!(antigravity_rollup_totals(&db)?, totals);
        source.execute("UPDATE steps SET status = 3", [])?;
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        let (modified, offset) = get_sync_state(&db, &key)?;
        assert!(modified > 0);
        assert_eq!(offset, 1);
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 0));
        assert_eq!(antigravity_rollup_totals(&db)?, totals);
        Ok(())
    }

    #[test]
    fn antigravity_failed_ledger_write_rolls_back_detail_and_retries_sync() -> Result<(), AppError>
    {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("retry.db");
        let _source = antigravity_sync_fixture(&path, 3)?;
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER fail_agy_ledger BEFORE INSERT ON session_usage_dedup
                BEGIN SELECT RAISE(FAIL, 'injected ledger failure'); END;",
            )?;
        }
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        assert_eq!(get_sync_state(&db, &path.to_string_lossy())?, (0, 0));
        {
            let conn = lock_conn!(db.conn);
            for table in ["proxy_request_logs", "session_usage_dedup"] {
                let count: i64 =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
                assert_eq!(count, 0, "transaction must roll back {table}");
            }
            conn.execute_batch("DROP TRIGGER fail_agy_ledger")?;
        }
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 0));
        Ok(())
    }

    #[test]
    fn antigravity_late_proxy_real_rollup_does_not_double_count() -> Result<(), AppError> {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("late-proxy.db");
        let _source = antigravity_sync_fixture(&path, 2)?;
        let db = Database::memory()?;
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (1, 0));
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (request_id, provider_id, app_type,
                model, request_model, pricing_model, input_tokens, output_tokens, cache_read_tokens,
                total_cost_usd, latency_ms, status_code, created_at, data_source)
                VALUES ('late-proxy', 'google', 'gemini', 'gemini-3.5-flash', 'gemini-3.5-flash',
                'gemini-3.5-flash', 100, 50, 20, '0.05', 200, 200, 10000, 'proxy')",
                [],
            )?;
        }
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        assert_eq!(db.rollup_and_prune(30)?, 2);
        assert_eq!(antigravity_rollup_totals(&db)?, (1, 0.05));
        assert_eq!(sync_single_antigravity_db(&db, &path)?, (0, 1));
        assert_eq!(db.rollup_and_prune(30)?, 0);
        assert_eq!(antigravity_rollup_totals(&db)?, (1, 0.05));
        Ok(())
    }

    #[test]
    fn antigravity_first_import_after_v4_0_4_upgrade_preserves_other_sources(
    ) -> Result<(), AppError> {
        // v4.0.4 and this release both use schema 20; their schema.rs is identical.
        // Reopen an on-disk fixture with existing usage and checkpoints, then run
        // the same create/migrate/seed sequence without touching user state.
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("cc-switch.db");
        {
            let conn = rusqlite::Connection::open(&db_path)?;
            Database::create_tables_on_conn(&conn)?;
            conn.execute_batch("PRAGMA user_version = 20;")?;
            conn.execute(
                "INSERT INTO proxy_request_logs (request_id, provider_id, app_type,
                model, input_tokens, output_tokens, total_cost_usd, latency_ms, status_code,
                created_at, data_source) VALUES ('existing-codex', 'codex', 'codex', 'gpt-5',
                123, 45, '1.25', 100, 200, 10000, 'codex_session')",
                [],
            )?;
        }
        let db = Database {
            conn: std::sync::Mutex::new(rusqlite::Connection::open(&db_path)?),
            log_count_cache: std::sync::Mutex::new(None),
        };
        update_sync_state(&db, "existing-codex-file", 123456, 789)?;
        db.create_tables()?;
        db.apply_schema_migrations()?;
        db.ensure_model_pricing_seeded()?;
        let source_path = temp.path().join("first-import.db");
        let _source = antigravity_sync_fixture(&source_path, 3)?;
        assert_eq!(sync_single_antigravity_db(&db, &source_path)?, (1, 0));
        assert_eq!(sync_single_antigravity_db(&db, &source_path)?, (0, 0));
        assert_eq!(get_sync_state(&db, "existing-codex-file")?, (123456, 789));
        let conn = lock_conn!(db.conn);
        assert_eq!(Database::get_user_version(&conn)?, 20);
        let old: (i64, String, String) = conn.query_row(
            "SELECT input_tokens, total_cost_usd,
            data_source FROM proxy_request_logs WHERE request_id = 'existing-codex'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(old, (123, "1.25".into(), "codex_session".into()));
        let counts: (i64, i64) = conn.query_row(
            "SELECT
            (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'antigravity_session'),
            (SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'antigravity_session')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(counts, (1, 1));
        Ok(())
    }
    #[test]
    #[ignore = "requires copied AGY databases under CC_SWITCH_TEST_HOME"]
    fn validate_platform_usage_database_copy() -> Result<(), AppError> {
        assert!(std::env::var_os("CC_SWITCH_TEST_HOME").is_some());
        let discovered = collect_antigravity_db_files(&get_gemini_dir());
        assert!(
            !discovered.is_empty(),
            "fixture must contain conversation databases"
        );
        let mut files = Vec::new();
        let mut empty = 0;
        for path in discovered {
            let conn = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let tables: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            if tables == 0 {
                empty += 1;
            } else {
                files.push(path);
            }
        }
        let db = Database::memory()?;
        let mut imported = 0;
        for path in &files {
            imported += sync_single_antigravity_db(&db, path)?.0;
        }
        assert!(imported > 0, "fixture must contain usage");
        let totals = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
                SUM(CAST(total_cost_usd AS REAL)) FROM proxy_request_logs",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, f64>(3)?,
                    ))
                },
            )?
        };
        for path in &files {
            assert_eq!(sync_single_antigravity_db(&db, path)?.0, 0);
        }
        let conn = lock_conn!(db.conn);
        let again = conn.query_row(
            "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
            SUM(CAST(total_cost_usd AS REAL)) FROM proxy_request_logs",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            },
        )?;
        assert_eq!(totals, again);
        println!(
            "platform usage: databases={}, imported={imported}, rows={}, repeat_unchanged=true, empty_databases={empty}",
            files.len(),
            totals.0
        );
        Ok(())
    }
}
