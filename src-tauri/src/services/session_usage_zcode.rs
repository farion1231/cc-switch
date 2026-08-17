//! ZCode 会话日志使用追踪
//!
//! 从 ~/.zcode/cli/{rollout,debug}/ 下的 JSONL 会话文件中提取 token 使用数据，
//! 写入 proxy_request_logs 表（app_type='zcode'）。
//!
//! ## 数据流
//! ```text
//! ~/.zcode/cli/{rollout,debug}/model-io-*.jsonl → 增量解析 → proxy_request_logs 表
//! ```
//!
//! ## 解析的事件类型
//! - `type == "model_io"` → 提取 requestId / modelId / usage / completedAt / durationMs / sessionId
//!
//! ZCode 的 jsonl 每行即是一次 LLM API 调用，字段与 CC Switch 的 proxy_request_logs
//! 结构一一对应，无需 delta 计算，比 Codex 的父子 replay 简单得多。

use crate::config::get_zcode_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

const ZCODE_REQUEST_ID_PREFIX: &str = "zcode_session";
const ZCODE_APP_TYPE: &str = "zcode";
const ZCODE_PROVIDER_ID: &str = "_zcode_session";

static ZCODE_SCAN_PATHS: OnceLock<Vec<(PathBuf, PathBuf)>> = OnceLock::new();

/// 返回 ZCode session log 的扫描路径列表 — (glob_pattern, display_name)。
/// rollout 与 debug 两组目录结构相同，同一个 glob pattern 可合并。
fn zcode_scan_paths() -> &'static Vec<(PathBuf, PathBuf)> {
    ZCODE_SCAN_PATHS.get_or_init(|| {
        let base = get_zcode_config_dir();
        vec![
            (base.join("cli").join("rollout").join("model-io-*.jsonl"), "rollout".into()),
            (base.join("cli").join("debug").join("model-io-*.jsonl"), "debug".into()),
        ]
    })
}

/// ZCode jsonl 中提取的单条 model_io 事件
#[derive(Debug)]
struct ParsedZCodeEntry {
    request_id: String,
    session_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    created_at: i64,
    duration_ms: i64,
}

/// 同步 ZCode session logs 到 proxy_request_logs（app_type='zcode'）。
///
/// 增量同步：借 session_log_sync 表按 (file_path → last_line_offset) 记游标。
/// 幂等：request_id 是主键，INSERT OR IGNORE 可重复跑。
pub fn sync_zcode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();

    for (pattern, _name) in zcode_scan_paths() {
        if let Some(parent) = pattern.parent() {
            if !parent.exists() {
                continue;
            }
        }

        let dir = pattern.parent().unwrap();
        let prefix = pattern.file_name().unwrap().to_str().unwrap().trim_end_matches("*");
        let prefix_len = prefix.len();

        let entries: Vec<_> = match fs::read_dir(dir) {
            Ok(e) => e.flatten().collect(),
            Err(_) => continue,
        };

        for entry in entries {
            let path = entry.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if !file_name.starts_with(prefix) || !file_name.ends_with(".jsonl") {
                continue;
            }

            result.files_scanned += 1;

            match sync_single_file(db, &path) {
                Ok((imported, skipped)) => {
                    result.imported += imported;
                    result.skipped += skipped;
                }
                Err(e) => {
                    let msg = format!("{}: {e}", path.display());
                    log::warn!("[ZCODE-SYNC] 文件解析失败: {msg}");
                    result.errors.push(msg);
                }
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[ZCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 同步单个 ZCode JSONL 文件，返回 (imported, skipped)
fn sync_single_file(db: &Database, file_path: &PathBuf) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    let (last_modified, last_offset) = get_sync_state(db, &file_path_str)?;

    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let file = File::open(file_path)
        .map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);

    let mut line_offset: i64 = 0;
    let mut entries: Vec<ParsedZCodeEntry> = Vec::new();

    for line_result in reader.lines() {
        line_offset += 1;
        if line_offset <= last_offset {
            continue;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 只处理 model_io 类型
        if value.get("type").and_then(|t| t.as_str()) != Some("model_io") {
            continue;
        }

        let Some(entry) = parse_zcode_entry(&value) else {
            continue;
        };

        entries.push(entry);
    }

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    for entry in entries {
        match insert_zcode_entry(db, &entry) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[ZCODE-SYNC] 插入失败 ({}): {e}", entry.request_id);
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, line_offset)?;

    Ok((imported, skipped))
}

/// 从 jsonl 行解析出 ZCode entry
fn parse_zcode_entry(value: &serde_json::Value) -> Option<ParsedZCodeEntry> {
    let model = value.get("model")?.get("modelId")?.as_str()?.to_string();
    let usage = value.get("response")?.get("usage")?;

    let input_tokens = usage.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output_tokens = usage.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let cache_read_tokens = usage.get("cacheReadTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let cache_creation_tokens = usage.get("cacheCreationTokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // 必须有任一计费 token 才导入
    if input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 && cache_creation_tokens == 0 {
        return None;
    }

    let request_id = value.get("requestId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if request_id.is_empty() {
        return None;
    }

    let session_id = value.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let created_at = value
        .get("completedAt")
        .and_then(|v| v.as_str())
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

    let duration_ms = value.get("durationMs").and_then(|v| v.as_i64()).unwrap_or(0);

    Some(ParsedZCodeEntry {
        request_id,
        session_id,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        created_at,
        duration_ms,
    })
}

/// 插入单条 ZCode 会话记录到 proxy_request_logs，返回是否成功插入 (true=新插入, false=已存在)
fn insert_zcode_entry(db: &Database, entry: &ParsedZCodeEntry) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: ZCODE_APP_TYPE,
        model: &entry.model,
        input_tokens: entry.input_tokens,
        output_tokens: entry.output_tokens,
        cache_read_tokens: entry.cache_read_tokens,
        cache_creation_tokens: entry.cache_creation_tokens,
        created_at: entry.created_at,
    };
    if should_skip_session_insert(&conn, &entry.request_id, &dedup_key)? {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: entry.input_tokens,
        output_tokens: entry.output_tokens,
        cache_read_tokens: entry.cache_read_tokens,
        cache_creation_tokens: entry.cache_creation_tokens,
        model: Some(entry.model.clone()),
        message_id: None,
    };

    let pricing = find_model_pricing(&conn, &entry.model);
    let multiplier = rust_decimal::Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        match pricing {
            Some(p) => {
                let cost = CostCalculator::calculate(&usage, &p, multiplier);
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
                format!("{ZCODE_REQUEST_ID_PREFIX}:{}", entry.request_id),
                ZCODE_PROVIDER_ID,
                ZCODE_APP_TYPE,
                entry.model,
                entry.model,
                entry.input_tokens,
                entry.output_tokens,
                entry.cache_read_tokens,
                entry.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                entry.duration_ms,
                Option::<i64>::None,
                200i64,
                Option::<String>::None,
                entry.session_id,
                Some("zcode_session"),
                1i64,
                "1.0",
                entry.created_at,
                "zcode_session",
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 ZCode 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_jsonl(path: &PathBuf, lines: &[&str]) {
        let mut file = File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
    }

    #[test]
    fn test_parse_model_io_entry() {
        let line = r#"{"type":"model_io","requestId":"req-abc123","sessionId":"sess-xyz","model":{"modelId":"glm-5.2"},"response":{"usage":{"inputTokens":1000,"outputTokens":500,"cacheReadTokens":200,"cacheCreationTokens":50}},"completedAt":"2026-08-01T12:00:00Z","durationMs":3200}"#;
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let entry = parse_zcode_entry(&value).unwrap();
        assert_eq!(entry.request_id, "req-abc123");
        assert_eq!(entry.session_id, "sess-xyz");
        assert_eq!(entry.model, "glm-5.2");
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 500);
        assert_eq!(entry.cache_read_tokens, 200);
        assert_eq!(entry.cache_creation_tokens, 50);
        assert_eq!(entry.duration_ms, 3200);
    }

    #[test]
    fn test_parse_skips_non_model_io() {
        let line = r#"{"type":"user_message","text":"hello"}"#;
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parse_zcode_entry(&value).is_none());
    }

    #[test]
    fn test_parse_skips_zero_tokens() {
        let line = r#"{"type":"model_io","requestId":"req-zero","model":{"modelId":"glm-5.2"},"response":{"usage":{"inputTokens":0,"outputTokens":0,"cacheReadTokens":0,"cacheCreationTokens":0}}}"#;
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parse_zcode_entry(&value).is_none());
    }

    #[test]
    fn test_sync_imports_entries() {
        let db = Database::memory().unwrap();
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("model-io-test.jsonl");

        let line = r#"{"type":"model_io","requestId":"req-sync-test","sessionId":"sess-1","model":{"modelId":"glm-5.2"},"response":{"usage":{"inputTokens":1000,"outputTokens":500,"cacheReadTokens":200,"cacheCreationTokens":0}},"completedAt":"2026-08-01T12:00:00Z","durationMs":1000}"#;
        write_jsonl(&file, &[line]);

        let (imported, skipped) = sync_single_file(&db, &file).unwrap();
        assert_eq!(imported, 1);
        assert_eq!(skipped, 0);

        // 再次同步应跳过（幂等）
        let (imported2, skipped2) = sync_single_file(&db, &file).unwrap();
        assert_eq!(imported2, 0);
        assert_eq!(skipped2, 1);

        // 验证数据正确
        let conn = lock_conn!(db.conn);
        let row: (i64, i64, i64) = conn
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens FROM proxy_request_logs WHERE request_id LIKE '%req-sync-test%'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, (1000, 500, 200));

        drop(conn);
        drop(tmp);
    }
}
