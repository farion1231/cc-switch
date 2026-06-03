//! Antigravity 会话日志使用追踪
//!
//! 从 ~/.gemini/antigravity/conversations/*.db 中提取 token 使用数据。
//! Antigravity 使用 protobuf 格式存储在 SQLite 数据库中，而不是 JSON/JSONL。
//!
//! ## 数据库结构
//! - gen_metadata: 每次 LLM 响应后的累计状态快照（protobuf blob）
//! - steps: 每步操作记录，含精确时间戳（metadata.f1）和 gen_idx 映射（metadata.f20.f3）
//! - trajectory_metadata_blob: 会话元数据（项目路径、时间戳等）
//!
//! ## Protobuf token 字段路径
//! - gen_metadata → f1 → f4.{f2,f3,f9,f10}: 当前步 token 明细
//! - gen_metadata → f1 → f17.f2.{f2,f3,f9,f10}: 累计 token 明细（部分会话）
//! - gen_metadata → f1 → f19: 模型名称
//! - gen_metadata → f1 → f9.f10.f1: 累计总 token 数
//! - trajectory_metadata_blob → f2.{f1,f2}: 会话创建时间戳
//! - trajectory_metadata_blob → f3: 会话 ID
//! - steps → f1.{f1,f2}: 每步操作精确时间戳
//! - steps → f20.f3: 对应 gen_metadata idx
//!
//! ## 时间戳策略
//! 1. 优先使用 steps 表的 per-step 精确时间戳
//! 2. 没有对应 steps 的增量条目 fallback 到文件修改时间

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Decoded protobuf field value
#[derive(Debug)]
enum ProtoValue {
    Varint(u64),
    String(String),
    Nested(Vec<u8>),
}

/// Simple protobuf message parser
struct ProtoParser<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ProtoParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        ProtoParser { data, offset: 0 }
    }

    fn decode_varint(&mut self) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
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
        if self.offset >= self.data.len() {
            return None;
        }

        let tag = self.decode_varint()?;
        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x7) as u32;

        match wire_type {
            0 => {
                // Varint
                let value = self.decode_varint()?;
                Some((field_num, ProtoValue::Varint(value)))
            }
            2 => {
                // Length-delimited
                let length = self.decode_varint()? as usize;
                if self.offset + length > self.data.len() {
                    return None;
                }
                let blob = &self.data[self.offset..self.offset + length];
                self.offset += length;

                // Try to decode as UTF-8 string
                if let Ok(s) = std::str::from_utf8(blob) {
                    // Only treat as string if it looks printable
                    if s.chars()
                        .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace() || c == '\0')
                    {
                        Some((field_num, ProtoValue::String(s.to_string())))
                    } else {
                        Some((field_num, ProtoValue::Nested(blob.to_vec())))
                    }
                } else {
                    Some((field_num, ProtoValue::Nested(blob.to_vec())))
                }
            }
            5 => {
                // Fixed32 - skip 4 bytes
                if self.offset + 4 > self.data.len() {
                    return None;
                }
                self.offset += 4;
                self.next_field()
            }
            _ => None,
        }
    }

    /// Find a specific field number in the message and extract its value
    fn get_varint(&mut self, target_field: u32) -> Option<u64> {
        while let Some((num, value)) = self.next_field() {
            match (num, value) {
                (n, ProtoValue::Varint(v)) if n == target_field => return Some(v),
                _ => continue,
            }
        }
        None
    }

    /// Find a specific field number and return its nested blob
    fn get_nested(&mut self, target_field: u32) -> Option<Vec<u8>> {
        while let Some((num, value)) = self.next_field() {
            match (num, value) {
                (n, ProtoValue::Nested(v)) if n == target_field => return Some(v),
                _ => continue,
            }
        }
        None
    }

    /// Find a specific field number and return its string value
    fn get_string(&mut self, target_field: u32) -> Option<String> {
        while let Some((num, value)) = self.next_field() {
            match (num, value) {
                (n, ProtoValue::String(s)) if n == target_field => return Some(s),
                _ => continue,
            }
        }
        None
    }
}

/// 从 protobuf blob 中提取 Antigravity token 使用数据
#[derive(Debug)]
struct AntigravityTokenData {
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    thoughts_tokens: u32,
    #[allow(dead_code)]
    total_tokens: u32,
    model: String,
}

/// 同步 Antigravity 使用数据（从 SQLite + protobuf 会话日志）
pub fn sync_antigravity_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let gemini_dir = get_gemini_dir();
    let antigravity_dir = gemini_dir.join("antigravity").join("conversations");

    let files = collect_antigravity_db_files(&antigravity_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        errors: vec![],
    };

    if files.is_empty() {
        return Ok(result);
    }

    for file_path in &files {
        match sync_single_antigravity_db(db, file_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Antigravity 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[ANTIGRAVITY-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[ANTIGRAVITY-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Antigravity 会话 DB 文件
fn collect_antigravity_db_files(antigravity_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if !antigravity_dir.is_dir() {
        return files;
    }

    let entries = match fs::read_dir(antigravity_dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "db" {
                // Skip WAL/SHM files
                if path.to_string_lossy().ends_with("-wal")
                    || path.to_string_lossy().ends_with("-shm")
                {
                    continue;
                }
                files.push(path);
            }
        }
    }

    files
}

/// 同步单个 Antigravity DB 文件，返回 (imported, skipped)
fn sync_single_antigravity_db(db: &Database, db_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = db_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata =
        fs::metadata(db_path).map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_modified_secs = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 检查同步状态
    let (last_modified, last_gen_idx) = get_sync_state(db, &file_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 使用 rusqlite 打开 Antigravity 的数据库
    let antigravity_conn = rusqlite::Connection::open(db_path)
        .map_err(|e| AppError::Config(format!("无法打开 Antigravity DB: {e}")))?;

    // 读取 trajectory_metadata_blob 获取会话元数据
    let trajectory_meta = read_trajectory_metadata(&antigravity_conn);

    // 读取 gen_metadata 条目
    let gen_entries = read_gen_metadata_entries(&antigravity_conn);

    if gen_entries.is_empty() {
        update_sync_state(db, &file_path_str, file_modified, 0)?;
        return Ok((0, 0));
    }

    // 读取 steps 表获取 per-entry 精确时间戳
    let step_timestamps = read_step_timestamps(&antigravity_conn);

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;
    let max_idx = gen_entries.last().map(|e| e.idx + 1).unwrap_or(0);

    // 获取会话 ID
    let session_id = trajectory_meta.as_ref().and_then(|m| m.session_id.clone());

    // 处理每个 gen_metadata 条目，计算 delta
    for entry in &gen_entries {
        let idx = entry.idx;
        if idx < last_gen_idx {
            continue; // 已处理过
        }

        if let Some(token_data) = &entry.token_data {
            let dedup_request_id = format!(
                "antigravity_session:{}:{}",
                session_id.as_deref().unwrap_or("unknown"),
                idx
            );

            // 优先用 steps 表的精确时间戳，没有 steps 的条目用文件修改时间
            let created_at = step_timestamps
                .get(&idx)
                .copied()
                .unwrap_or(file_modified_secs);

            match insert_antigravity_session_entry(
                db,
                &dedup_request_id,
                token_data,
                &session_id,
                created_at,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[ANTIGRAVITY-SYNC] 插入失败 ({}): {e}", dedup_request_id);
                    skipped += 1;
                }
            }
        }
    }

    // 更新同步状态（使用 max_idx 作为 offset 避免全量重读）
    update_sync_state(db, &file_path_str, file_modified, max_idx)?;

    Ok((imported, skipped))
}

/// 轨迹元数据
#[derive(Debug, Default)]
struct TrajectoryMetadata {
    session_id: Option<String>,
    project_path: Option<String>,
    created_at_seconds: Option<i64>,
}

/// 读取 trajectory_metadata_blob
fn read_trajectory_metadata(conn: &rusqlite::Connection) -> Option<TrajectoryMetadata> {
    let mut stmt = conn
        .prepare("SELECT data FROM trajectory_metadata_blob WHERE id = 'main'")
        .ok()?;
    let data: Vec<u8> = stmt.query_row([], |row| row.get(0)).ok()?;

    let mut parser = ProtoParser::new(&data);
    let mut meta = TrajectoryMetadata::default();

    while let Some((num, value)) = parser.next_field() {
        match (num, value) {
            // f1: workspace info
            (1, ProtoValue::Nested(nested)) => {
                let mut wp = ProtoParser::new(&nested);
                // Extract project info if needed
                while let Some((wn, wv)) = wp.next_field() {
                    match (wn, wv) {
                        (1, ProtoValue::String(s)) => {
                            meta.project_path = Some(s);
                        }
                        _ => continue,
                    }
                }
            }
            // f2: timestamp (seconds + nanos)
            (2, ProtoValue::Nested(nested)) => {
                let mut tp = ProtoParser::new(&nested);
                meta.created_at_seconds = tp.get_varint(1).map(|v| v as i64);
            }
            // f3: session ID
            (3, ProtoValue::String(s)) => {
                meta.session_id = Some(s);
            }
            _ => continue,
        }
    }

    Some(meta)
}

/// 单个 gen_metadata 条目
#[derive(Debug)]
struct GenMetadataEntry {
    idx: i64,
    token_data: Option<AntigravityTokenData>,
}

/// 读取所有 gen_metadata 条目
fn read_gen_metadata_entries(conn: &rusqlite::Connection) -> Vec<GenMetadataEntry> {
    let mut entries = Vec::new();

    let mut stmt = match conn.prepare("SELECT idx, data FROM gen_metadata ORDER BY idx") {
        Ok(s) => s,
        Err(_) => return entries,
    };

    let rows = stmt
        .query_map([], |row| {
            let idx: i64 = row.get(0)?;
            let data: Vec<u8> = row.get(1)?;
            Ok((idx, data))
        })
        .ok();

    if let Some(rows) = rows {
        for row in rows.flatten() {
            let (idx, data) = row;
            let token_data = parse_gen_metadata_blob(&data);
            entries.push(GenMetadataEntry { idx, token_data });
        }
    }

    entries
}

/// 从 steps 表读取 per-step 时间戳，构建 gen_idx → 最早时间戳的映射。
/// steps.metadata.f1.{f1,f2} = 创建时间 (seconds, nanos)
/// steps.metadata.f20.f3 = 对应的 gen_metadata idx
fn read_step_timestamps(conn: &rusqlite::Connection) -> std::collections::HashMap<i64, i64> {
    let mut map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();

    let mut stmt = match conn.prepare("SELECT metadata FROM steps WHERE metadata IS NOT NULL") {
        Ok(s) => s,
        Err(_) => return map,
    };

    let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0)).ok();

    if let Some(rows) = rows {
        for row in rows.flatten() {
            if let Some((gen_idx, ts_secs)) = parse_step_timestamp(&row) {
                // 每个 gen_idx 取最早的时间戳
                map.entry(gen_idx)
                    .and_modify(|existing| {
                        if ts_secs < *existing {
                            *existing = ts_secs;
                        }
                    })
                    .or_insert(ts_secs);
            }
        }
    }

    map
}

/// 解析单个 step metadata protobuf，提取 (gen_idx, timestamp_seconds)
fn parse_step_timestamp(data: &[u8]) -> Option<(i64, i64)> {
    let mut parser = ProtoParser::new(data);
    let mut ts_secs: Option<i64> = None;
    let mut gen_idx: Option<i64> = None;

    while let Some((num, value)) = parser.next_field() {
        match num {
            1 => {
                // f1: google.protobuf.Timestamp
                if let ProtoValue::Nested(nested) = value {
                    let mut ts = ProtoParser::new(&nested);
                    ts_secs = ts.get_varint(1).map(|v| v as i64);
                }
            }
            20 => {
                // f20: nested with f3 = gen_metadata idx
                if let ProtoValue::Nested(nested) = value {
                    let mut f20 = ProtoParser::new(&nested);
                    gen_idx = f20.get_varint(3).map(|v| v as i64);
                }
            }
            _ => continue,
        }
    }

    match (gen_idx, ts_secs) {
        (Some(g), Some(t)) => Some((g, t)),
        _ => None,
    }
}

/// 将 Google 内部模型代号映射到公开名称，以匹配定价表
fn normalize_antigravity_model(raw: &str) -> String {
    // -b 后缀是 Google preview 模型的内部代号
    if let Some(base) = raw.strip_suffix("-b") {
        return format!("{base}-preview");
    }
    raw.to_string()
}

/// 解析 gen_metadata protobuf blob 提取 token 使用数据
fn parse_gen_metadata_blob(data: &[u8]) -> Option<AntigravityTokenData> {
    let mut parser = ProtoParser::new(data);

    // 获取 f1（主元数据嵌套消息）
    let f1_blob = parser.get_nested(1)?;
    let mut f1 = ProtoParser::new(&f1_blob);

    let mut model = String::new();
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;
    let mut cached_tokens: u32 = 0;
    let mut thoughts_tokens: u32 = 0;
    let mut total_tokens: u32 = 0;

    while let Some((num, value)) = f1.next_field() {
        match num {
            // f4: token usage for this step
            4 => {
                if let ProtoValue::Nested(nested) = value {
                    extract_token_fields(
                        &nested,
                        &mut input_tokens,
                        &mut output_tokens,
                        &mut cached_tokens,
                        &mut thoughts_tokens,
                    );
                }
            }
            // f9.f10.f1: total accumulated tokens
            9 => {
                if let ProtoValue::Nested(nested) = value {
                    let mut f9 = ProtoParser::new(&nested);
                    if let Some(f10_blob) = f9.get_nested(10) {
                        let mut f10 = ProtoParser::new(&f10_blob);
                        total_tokens = f10.get_varint(1).unwrap_or(0) as u32;
                    }
                }
            }
            // f17.f2: cumulative token usage (some conversations)
            17 => {
                if let ProtoValue::Nested(nested) = value {
                    let mut f17 = ProtoParser::new(&nested);
                    if let Some(f2_blob) = f17.get_nested(2) {
                        extract_token_fields(
                            &f2_blob,
                            &mut input_tokens,
                            &mut output_tokens,
                            &mut cached_tokens,
                            &mut thoughts_tokens,
                        );
                    }
                }
            }
            // f19: model name string（优先使用）
            19 => {
                if let ProtoValue::String(s) = value {
                    model = s;
                }
            }
            // f20: metadata tags — model_enum 只在 f19 为空时做 fallback
            20 => {
                if model.is_empty() {
                    if let ProtoValue::Nested(nested) = value {
                        let mut tag_parser = ProtoParser::new(&nested);
                        let tag_key = tag_parser.get_string(1);
                        let tag_val = tag_parser.get_string(2);
                        if tag_key.as_deref() == Some("model_enum") {
                            if let Some(v) = tag_val {
                                model = v;
                            }
                        }
                    }
                }
            }
            _ => continue,
        }
    }

    // 需要至少有一些 token 数据
    if input_tokens == 0 && output_tokens == 0 && cached_tokens == 0 && thoughts_tokens == 0 {
        return None;
    }

    if model.is_empty() {
        model = "gemini".to_string();
    }

    // Google 内部代号映射到公开模型名，以匹配定价表
    model = normalize_antigravity_model(&model);

    Some(AntigravityTokenData {
        input_tokens,
        output_tokens,
        cached_tokens,
        thoughts_tokens,
        total_tokens,
        model,
    })
}

/// 从 token usage protobuf blob 中提取字段
fn extract_token_fields(
    data: &[u8],
    input: &mut u32,
    output: &mut u32,
    cached: &mut u32,
    thoughts: &mut u32,
) {
    let mut parser = ProtoParser::new(data);
    while let Some((num, value)) = parser.next_field() {
        if let ProtoValue::Varint(v) = value {
            match num {
                2 => *input = v as u32,     // input/prompt tokens
                3 => *output = v as u32,    // output/completion tokens
                9 => *cached = v as u32,    // cached content tokens
                10 => *thoughts = v as u32, // thoughts/reasoning tokens
                _ => {}
            }
        }
    }
}

/// 插入单条 Antigravity 会话记录到 proxy_request_logs
fn insert_antigravity_session_entry(
    db: &Database,
    request_id: &str,
    token_data: &AntigravityTokenData,
    session_id: &Option<String>,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    // 合并 thoughts 到 output（思考 token 按输出计费）
    let output_tokens = token_data.output_tokens + token_data.thoughts_tokens;

    let dedup_key = DedupKey {
        app_type: "gemini",
        model: &token_data.model,
        input_tokens: token_data.input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: token_data.input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        model: Some(token_data.model.clone()),
        message_id: None,
    };

    let pricing = find_model_pricing(&conn, &token_data.model);
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

    // 使用 UPSERT
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
            total_cost_usd = excluded.total_cost_usd,
            created_at = excluded.created_at
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR created_at != excluded.created_at",
        rusqlite::params![
            request_id,
            "_antigravity_session",
            "gemini",
            token_data.model,
            token_data.model,
            token_data.input_tokens,
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
            session_id.clone(),
            Some("antigravity_session"),
            1i64,
            "1.0",
            created_at,
            "antigravity_session",
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Antigravity 会话日志失败: {e}")))?;

    Ok(conn.changes() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_varint_decode() {
        // 1 in varint
        let data = [0x01u8];
        let mut p = ProtoParser::new(&data);
        assert_eq!(p.decode_varint(), Some(1));

        // 300 in varint: 0xAC 0x02
        let data = [0xACu8, 0x02];
        let mut p = ProtoParser::new(&data);
        assert_eq!(p.decode_varint(), Some(300));

        // 0 in varint
        let data = [0x00u8];
        let mut p = ProtoParser::new(&data);
        assert_eq!(p.decode_varint(), Some(0));
    }

    #[test]
    fn test_proto_field_decode() {
        // Tag 1 (varint) = 42: 0x08 0x2A
        let data = [0x08u8, 0x2A];
        let mut p = ProtoParser::new(&data);
        let (num, val) = p.next_field().unwrap();
        assert_eq!(num, 1);
        assert!(matches!(val, ProtoValue::Varint(42)));

        // Tag 2 (length-delimited, "hi"): 0x12 0x02 0x68 0x69
        let data = [0x12u8, 0x02, 0x68, 0x69];
        let mut p = ProtoParser::new(&data);
        let (num, val) = p.next_field().unwrap();
        assert_eq!(num, 2);
        assert!(matches!(val, ProtoValue::String(ref s) if s == "hi"));
    }

    #[test]
    fn test_collect_antigravity_db_files_nonexistent() {
        let files = collect_antigravity_db_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }
}
