//! Oh My Pi session usage importer.
//!
//! Oh My Pi records usage/cost in its session JSONL files under
//! `~/.omp/agent/sessions/*.jsonl`. This importer keeps direct (non-proxy) Oh
//! My Pi usage visible in the shared dashboard, mirroring the Gemini importer
//! flow: mtime-based incremental skip, dedup via `session_usage_dedup`, and an
//! UPSERT into `proxy_request_logs` with `data_source = 'ohmypi_session'`.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const APP_TYPE: &str = "ohmypi";
const DATA_SOURCE: &str = "ohmypi_session";
const PROVIDER_PLACEHOLDER: &str = "_ohmypi_session";
const UNKNOWN_MODEL: &str = "unknown";
const MAX_USAGE_LABEL_BYTES: usize = 512;

/// 同步 Oh My Pi 使用数据（从 JSONL 会话日志）
pub fn sync_ohmypi_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = collect_ohmypi_session_files()?;

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

    let cursors = load_sync_cursors(db)?;

    for file_path in &files {
        let last_modified = cursors
            .get(file_path.to_string_lossy().as_ref())
            .map_or(0, |c| c.last_modified);
        match sync_single_ohmypi_file(db, file_path, last_modified) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(error) => {
                let msg = format!("Oh My Pi 会话文件解析失败 {}: {error}", file_path.display());
                log::warn!("[OHMYPI-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[OHMYPI-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集 `~/.omp/agent/sessions` 下的 JSONL 会话文件
fn collect_ohmypi_session_files() -> Result<Vec<PathBuf>, AppError> {
    let sessions_dir = crate::ohmypi_config::get_ohmypi_agent_dir()?.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_jsonl_files(&sessions_dir, &mut files);
    Ok(files)
}

/// 递归收集 `sessions_dir` 下的所有 JSONL 会话文件（支持两层 `<branch>/<timestamp>_<uuid>.jsonl` 布局）
fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

/// 同步单个 Oh My Pi JSONL 会话文件，返回 (imported, skipped)
///
/// `last_modified` 来自调用方批量预取的游标（见 [`crate::services::session_usage::load_sync_cursors`]）。
fn sync_single_ohmypi_file(
    db: &Database,
    file_path: &Path,
    last_modified: i64,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    if file_modified <= last_modified {
        return Ok((0, 0));
    }
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;
    let mut ohmypi_msg_count: i64 = 0;

    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let Some(message) = value.get("message").and_then(|v| v.as_object()) else {
            continue;
        };
        if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let usage = match message.get("usage") {
            Some(u) if u.is_object() => u,
            _ => continue,
        };
        let input = usage
            .get("inputTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("outputTokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if input == 0 && output == 0 {
            continue; // 跳过无用量记录的消息
        }

        ohmypi_msg_count += 1;

        let model = bounded_label(message.get("model"), UNKNOWN_MODEL);
        let session_id = value.get("sessionId").and_then(|v| v.as_str());
        let message_id = message
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let request_id = format!(
            "ohmypi_session:{}:{}",
            session_id.unwrap_or("unknown"),
            message_id
        );
        let created_at = value
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .map(|ms| ms / 1000)
            .unwrap_or_else(now_ts);

        match insert_ohmypi_session_entry(
            db,
            &request_id,
            input as u32,
            output as u32,
            &model,
            session_id,
            created_at,
        ) {
            Ok(true) => imported += 1,
            _ => skipped += 1,
        }
    }

    update_sync_state(db, &file_path_str, file_modified, ohmypi_msg_count)?;
    Ok((imported, skipped))
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn bounded_label(value: Option<&Value>, fallback: &str) -> String {
    let raw = value
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback);
    let mut label = raw.to_string();
    if label.len() > MAX_USAGE_LABEL_BYTES {
        label.truncate(MAX_USAGE_LABEL_BYTES);
    }
    label
}

/// 插入单条 Oh My Pi 会话记录到 proxy_request_logs
fn insert_ohmypi_session_entry(
    db: &Database,
    request_id: &str,
    input_tokens: u32,
    output_tokens: u32,
    model: &str,
    session_id: Option<&str>,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    // Check the session_usage_dedup ledger so re-scans of a modified file
    // don't re-import messages already written to proxy_request_logs.
    let already_seen: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM session_usage_dedup
                WHERE data_source = ?1 AND request_id = ?2
            )",
            rusqlite::params![DATA_SOURCE, request_id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(format!("查询 Oh My Pi 用量去重账本失败: {e}")))?;
    if already_seen {
        return Ok(false);
    }

    // Register in the ledger before inserting the usage row.
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![DATA_SOURCE, request_id, request_id, 0i64,],
    )
    .map_err(|e| AppError::Database(format!("写入 Oh My Pi 用量去重账本失败: {e}")))?;

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_model_pricing(&conn, model);
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, &p, multiplier);
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
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        rusqlite::params![
            request_id,
            PROVIDER_PLACEHOLDER,
            APP_TYPE,
            model,
            model,
            input_tokens,
            output_tokens,
            0i64,
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
            session_id,
            "session",
            false,
            "1",
            created_at,
            DATA_SOURCE,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Oh My Pi 会话日志失败: {e}")))?;

    Ok(true)
}
