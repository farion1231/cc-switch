//! DeepSeek Harness 会话用量追踪
//!
//! 从 DeepSeek Harness 的 JSONL session persistence 中提取 provider-reported
//! token 用量。Harness 的 `TokenUsage` 语义是 disjoint：`inputTokens` 已排除
//! cache 命中，`cacheReadTokens` / `cacheWriteTokens` 单独计数。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const DATA_SOURCE: &str = "deepseek_harness_session";
const PROVIDER_ID: &str = "_deepseek_harness_session";
const APP_TYPE: &str = "deepseek-harness";
const MAX_SESSION_LOG_BYTES: u64 = 50 * 1024 * 1024;
const MAX_COLLECT_DEPTH: usize = 16;

#[derive(Debug, Clone, Default)]
struct HarnessUsageEvent {
    seq: u64,
    session_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    created_at: i64,
}

impl HarnessUsageEvent {
    fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_creation_tokens == 0
    }
}

/// 同步 DeepSeek Harness 使用数据。
pub fn sync_deepseek_harness_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let roots = collect_session_roots();
    let mut files = Vec::new();
    for root in &roots {
        collect_session_files(root, &mut files, 0);
    }
    files.sort();
    files.dedup();

    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..Default::default()
    };

    for file_path in &files {
        match sync_single_harness_file(db, file_path) {
            Ok(file_result) => result.merge(file_result),
            Err(error) => {
                let msg = format!(
                    "DeepSeek Harness 会话文件解析失败 {}: {error}",
                    file_path.display()
                );
                log::warn!("[DSH-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[DSH-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

fn collect_session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if !path.is_dir() {
            return;
        }
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            roots.push(path);
        }
    };

    if let Some(root) = crate::settings::get_deepseek_harness_session_override_dir() {
        push(root);
    }

    if let Some(root) = std::env::var_os("DSH_SESSION_ROOT").filter(|v| !v.is_empty()) {
        push(PathBuf::from(root));
    }
    if let Some(home) = std::env::var_os("DSH_HOME").filter(|v| !v.is_empty()) {
        push(PathBuf::from(home).join("sessions"));
    }
    if let Some(home) = dirs::home_dir() {
        push(home.join(".dsh").join("sessions"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        push(cwd.join(".sessions"));
    }

    roots
}

fn collect_session_files(root: &Path, files: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_COLLECT_DEPTH {
        log::warn!(
            "DeepSeek Harness session directory traversal exceeded max depth {} at {}",
            MAX_COLLECT_DEPTH,
            root.display()
        );
        return;
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = entry.metadata();
        if metadata.as_ref().map(|m| m.is_symlink()).unwrap_or(false) {
            log::info!("[DSH-SYNC] 跳过符号链接（不跟随）: {}", path.display());
            continue;
        }
        if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            collect_session_files(&path, files, depth + 1);
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl")
            || path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl.zstd")
        {
            files.push(path);
        }
    }
}

fn sync_single_harness_file(
    db: &Database,
    file_path: &Path,
) -> Result<SessionSyncResult, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let metadata = fs::metadata(file_path)
        .map_err(|error| AppError::Config(format!("无法读取文件元数据: {error}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    if metadata.len() > MAX_SESSION_LOG_BYTES {
        log::warn!(
            "DeepSeek Harness session log too large ({} bytes), skipping: {}",
            metadata.len(),
            file_path.display()
        );
        return Ok(SessionSyncResult::default());
    }

    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok(SessionSyncResult::default());
    }

    let content = read_session_log_text(file_path)?;
    let events = parse_harness_usage_events(&content);
    let mut result = SessionSyncResult::default();

    for event in &events {
        if event.is_zero() {
            continue;
        }
        let request_id = format!("{DATA_SOURCE}:{}:{}", event.session_id, event.seq);
        match insert_harness_session_entry(db, &request_id, event) {
            Ok(true) => result.imported += 1,
            Ok(false) => result.skipped += 1,
            Err(error) => {
                log::warn!("[DSH-SYNC] 插入失败 ({request_id}): {error}");
                result.skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, events.len() as i64)?;
    Ok(result)
}

fn read_session_log_text(file_path: &Path) -> Result<String, AppError> {
    let bytes =
        fs::read(file_path).map_err(|error| AppError::Config(format!("无法读取文件: {error}")))?;
    let content = if file_path.extension().and_then(|ext| ext.to_str()) == Some("zstd") {
        zstd::decode_all(bytes.as_slice())
            .map_err(|error| AppError::Config(format!("无法解压 zstd 会话日志: {error}")))?
    } else {
        bytes
    };
    String::from_utf8(content)
        .map_err(|error| AppError::Config(format!("会话日志不是 UTF-8: {error}")))
}

fn parse_harness_usage_events(content: &str) -> Vec<HarnessUsageEvent> {
    let mut session_id: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut latest_model = "unknown".to_string();
    let mut events = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                if session_id.is_none() {
                    session_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
                }
                if created_at.is_none() {
                    created_at = value
                        .get("createdAt")
                        .and_then(Value::as_i64)
                        .map(|millis| millis / 1000);
                }
            }
            Some("request/context") => {
                if let Some(model) = value
                    .get("data")
                    .and_then(|data| data.get("model"))
                    .and_then(Value::as_str)
                {
                    latest_model = model.to_string();
                }
            }
            Some("request/header") => {
                if let Some(model) = value
                    .get("data")
                    .and_then(|data| data.get("header"))
                    .and_then(|header| header.get("config"))
                    .and_then(|config| config.get("model"))
                    .and_then(Value::as_str)
                {
                    latest_model = model.to_string();
                }
            }
            Some("assistant/message") => {
                let Some(data) = value.get("data") else {
                    continue;
                };
                let Some(usage) = data.get("usage") else {
                    continue;
                };
                let Some(session_id) = session_id.clone() else {
                    continue;
                };
                let model = data
                    .get("message")
                    .and_then(|message| message.get("source"))
                    .and_then(|source| source.get("model"))
                    .and_then(Value::as_str)
                    .unwrap_or(&latest_model)
                    .to_string();
                events.push(HarnessUsageEvent {
                    seq: value
                        .get("seq")
                        .and_then(Value::as_u64)
                        .unwrap_or(events.len() as u64),
                    session_id,
                    model,
                    input_tokens: value_u32(usage, "inputTokens"),
                    output_tokens: value_u32(usage, "outputTokens"),
                    cache_read_tokens: value_u32(usage, "cacheReadTokens"),
                    cache_creation_tokens: value_u32(usage, "cacheWriteTokens"),
                    created_at: value
                        .get("time")
                        .and_then(Value::as_i64)
                        .map(|millis| millis / 1000)
                        .or(created_at)
                        .unwrap_or_else(now_epoch_seconds),
                });
            }
            _ => {}
        }
    }

    events
}

fn value_u32(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn insert_harness_session_entry(
    db: &Database,
    request_id: &str,
    event: &HarnessUsageEvent,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model: &event.model,
        input_tokens: event.input_tokens,
        output_tokens: event.output_tokens,
        cache_read_tokens: event.cache_read_tokens,
        cache_creation_tokens: event.cache_creation_tokens,
        created_at: event.created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: event.input_tokens,
        output_tokens: event.output_tokens,
        cache_read_tokens: event.cache_read_tokens,
        cache_creation_tokens: event.cache_creation_tokens,
        model: Some(event.model.clone()),
        message_id: None,
    };
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match find_model_pricing(&conn, &event.model) {
            Some(pricing) => {
                let cost =
                    CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::from(1));
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

    let inserted = conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            pricing_model, input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
        rusqlite::params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            event.model,
            event.model,
            event.input_tokens,
            event.output_tokens,
            event.cache_read_tokens,
            event.cache_creation_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            Some(event.session_id.clone()),
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            event.created_at,
            DATA_SOURCE,
            event.model,
            INPUT_TOKEN_SEMANTICS_FRESH,
        ],
    )
    .map_err(|error| AppError::Database(format!("插入 DeepSeek Harness 会话日志失败: {error}")))?;

    Ok(inserted > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        [
            r#"{"type":"session","version":0,"id":"sess-1","createdAt":1700000000000,"cwd":"/work","delegationDepth":0}"#,
            r#"{"type":"request/context","seq":0,"time":1700000000100,"data":{"provider":"deepseek-official","model":"deepseek-v4-flash"}}"#,
            r#"{"type":"assistant/message","seq":1,"time":1700000002000,"data":{"turn":1,"step":1,"message":{"source":{"provider":"deepseek-official","model":"deepseek-v4-flash"},"content":[]},"usage":{"inputTokens":100,"outputTokens":20,"cacheReadTokens":30,"cacheWriteTokens":4,"reasoningTokens":8}}}"#,
        ]
        .join("\n")
    }

    #[test]
    fn parses_assistant_message_usage_without_adding_reasoning_twice() {
        let events = parse_harness_usage_events(&fixture());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id, "sess-1");
        assert_eq!(events[0].model, "deepseek-v4-flash");
        assert_eq!(events[0].input_tokens, 100);
        assert_eq!(events[0].output_tokens, 20);
        assert_eq!(events[0].cache_read_tokens, 30);
        assert_eq!(events[0].cache_creation_tokens, 4);
        assert_eq!(events[0].created_at, 1_700_000_002);
    }

    #[test]
    fn insert_is_idempotent_by_request_id() -> Result<(), AppError> {
        let db = Database::memory()?;
        let event = parse_harness_usage_events(&fixture()).remove(0);
        assert!(insert_harness_session_entry(
            &db,
            "deepseek_harness_session:sess-1:1",
            &event
        )?);
        assert!(!insert_harness_session_entry(
            &db,
            "deepseek_harness_session:sess-1:1",
            &event
        )?);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'deepseek_harness_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);

        let (pricing_model, input_semantics): (String, i64) = conn.query_row(
            "SELECT pricing_model, input_token_semantics FROM proxy_request_logs WHERE data_source = 'deepseek_harness_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(pricing_model, "deepseek-v4-flash");
        assert_eq!(input_semantics, INPUT_TOKEN_SEMANTICS_FRESH);
        Ok(())
    }

    #[test]
    fn reads_zstd_session_log() -> Result<(), AppError> {
        let encoded = zstd::encode_all(fixture().as_bytes(), 0)
            .map_err(|error| AppError::Config(error.to_string()))?;
        let tmp = tempfile::tempdir().map_err(|error| AppError::Config(error.to_string()))?;
        let path = tmp.path().join("session.jsonl.zstd");
        std::fs::write(&path, encoded).map_err(|error| AppError::Config(error.to_string()))?;

        let content = read_session_log_text(&path)?;
        let events = parse_harness_usage_events(&content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input_tokens, 100);
        Ok(())
    }
}
