//! Kimi Code 会话日志使用追踪
//!
//! 从 ~/.kimi-code/sessions/*/wire.jsonl 与
//! ~/.kimi-code/sessions/*/agents/*/wire.jsonl 中提取 `usage.record`。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::kimi_config::get_kimi_dir;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug)]
struct KimiUsageRecord {
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    created_at: i64,
}

pub fn sync_kimi_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let kimi_dir = get_kimi_dir();
    let files = collect_kimi_session_files(&kimi_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        errors: vec![],
    };

    for file_path in &files {
        match sync_single_kimi_file(db, file_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Kimi Code 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[KIMI-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[KIMI-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

fn collect_kimi_session_files(kimi_dir: &Path) -> Vec<PathBuf> {
    let sessions_dir = kimi_dir.join("sessions");
    if !sessions_dir.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    let sessions = match fs::read_dir(&sessions_dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in sessions.flatten() {
        let session_dir = entry.path();
        if !session_dir.is_dir() {
            continue;
        }

        let root_wire = session_dir.join("wire.jsonl");
        if root_wire.is_file() {
            files.push(root_wire);
        }

        let agents_dir = session_dir.join("agents");
        if !agents_dir.is_dir() {
            continue;
        }
        let agents = match fs::read_dir(&agents_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for agent in agents.flatten() {
            let wire = agent.path().join("wire.jsonl");
            if wire.is_file() {
                files.push(wire);
            }
        }
    }

    files
}

fn sync_single_kimi_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let (last_modified, last_offset) = get_sync_state(db, &file_path_str)?;

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);
    let session_id = kimi_session_id_from_path(file_path);
    let agent_id = kimi_agent_id_from_path(file_path).unwrap_or_else(|| "main".to_string());

    let mut line_offset: i64 = 0;
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    for line_result in reader.lines() {
        line_offset += 1;

        let line = match line_result {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() || !line.contains("\"usage.record\"") {
            continue;
        }

        if line_offset <= last_offset {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let record = match parse_kimi_usage_record(&value) {
            Some(record) => record,
            None => continue,
        };

        let session_id_str = session_id.as_deref().unwrap_or("unknown");
        let request_id = format!("kimi_session:{session_id_str}:{agent_id}:{line_offset}");

        match insert_kimi_usage_record(db, &request_id, &record, session_id.as_deref()) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[KIMI-SYNC] 插入失败 ({}): {e}", request_id);
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, line_offset)?;
    Ok((imported, skipped))
}

fn parse_kimi_usage_record(value: &serde_json::Value) -> Option<KimiUsageRecord> {
    if value.get("type").and_then(|v| v.as_str()) != Some("usage.record") {
        return None;
    }

    let usage = value.get("usage")?;
    let record = KimiUsageRecord {
        model: normalize_kimi_model(
            value
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
        ),
        input_tokens: usage
            .get("inputOther")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_tokens: usage.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cache_read_tokens: usage
            .get("inputCacheRead")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        cache_creation_tokens: usage
            .get("inputCacheCreation")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        created_at: value
            .get("time")
            .and_then(|v| v.as_i64())
            .map(|ms| ms / 1000)
            .unwrap_or_else(current_unix_seconds),
    };

    let has_tokens = record.input_tokens > 0
        || record.output_tokens > 0
        || record.cache_read_tokens > 0
        || record.cache_creation_tokens > 0;
    has_tokens.then_some(record)
}

fn insert_kimi_usage_record(
    db: &Database,
    request_id: &str,
    record: &KimiUsageRecord,
    session_id: Option<&str>,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: "kimi",
        model: &record.model,
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        created_at: record.created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        model: Some(record.model.clone()),
        message_id: None,
    };

    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match find_model_pricing(&conn, &record.model) {
            Some(pricing) => {
                let cost =
                    CostCalculator::calculate_for_app("kimi", &usage, &pricing, Decimal::from(1));
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

    let inserted_rows = conn
        .execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                request_id,
                "_kimi_session",
                "kimi",
                record.model,
                record.model,
                record.input_tokens,
                record.output_tokens,
                record.cache_read_tokens,
                record.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,
                Option::<i64>::None,
                200i64,
                Option::<String>::None,
                session_id.map(|s| s.to_string()),
                Some("kimi_session"),
                1i64,
                "1.0",
                record.created_at,
                "kimi_session",
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Kimi Code 会话日志失败: {e}")))?;

    if inserted_rows > 0 {
        crate::usage_events::notify_log_recorded();
    }

    Ok(inserted_rows > 0)
}

fn normalize_kimi_model(raw: &str) -> String {
    let mut model = raw.trim().to_lowercase();
    if let Some(pos) = model.rfind('/') {
        model = model[pos + 1..].to_string();
    }
    if model.is_empty() {
        "unknown".to_string()
    } else {
        model
    }
}

fn kimi_session_id_from_path(path: &Path) -> Option<String> {
    let components: Vec<_> = path.components().collect();
    components.windows(2).find_map(|window| {
        let name = window[0].as_os_str().to_string_lossy();
        (name == "sessions").then(|| window[1].as_os_str().to_string_lossy().to_string())
    })
}

fn kimi_agent_id_from_path(path: &Path) -> Option<String> {
    let components: Vec<_> = path.components().collect();
    components.windows(2).find_map(|window| {
        let name = window[0].as_os_str().to_string_lossy();
        (name == "agents").then(|| window[1].as_os_str().to_string_lossy().to_string())
    })
}

fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_kimi_session_files_nonexistent() {
        let files = collect_kimi_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_kimi_usage_record() {
        let json: serde_json::Value = serde_json::json!({
            "type": "usage.record",
            "model": "kimi-code/kimi-k2",
            "usage": {
                "inputOther": 10,
                "output": 5,
                "inputCacheRead": 3,
                "inputCacheCreation": 2
            },
            "usageScope": "turn",
            "time": 1779256800302i64
        });

        let record = parse_kimi_usage_record(&json).unwrap();
        assert_eq!(record.model, "kimi-k2");
        assert_eq!(record.input_tokens, 10);
        assert_eq!(record.output_tokens, 5);
        assert_eq!(record.cache_read_tokens, 3);
        assert_eq!(record.cache_creation_tokens, 2);
        assert_eq!(record.created_at, 1779256800);
    }

    #[test]
    fn test_insert_kimi_usage_record_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "kimi-proxy",
                    "moonshot",
                    "kimi",
                    "kimi-k2",
                    "kimi-k2",
                    10,
                    5,
                    3,
                    2,
                    "0.01",
                    100,
                    200,
                    1779256800,
                    "proxy"
                ],
            )?;
        }

        let record = KimiUsageRecord {
            model: "kimi-k2".to_string(),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 3,
            cache_creation_tokens: 2,
            created_at: 1779256800,
        };
        let inserted =
            insert_kimi_usage_record(&db, "kimi-session-dup", &record, Some("session-1"))?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }
}
