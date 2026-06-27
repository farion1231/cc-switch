//! Grok Build 会话日志使用追踪
//!
//! 从 ~/.grok/logs/unified.jsonl 中提取精确 token 使用数据，写入 proxy_request_logs，
//! 让 Usage 看板能统计 Grok Build（含 grok-composer-2.5-fast / grok-build-0.1）用量。
//!
//! 镜像 `session_usage_codex.rs` 的结构与约定，复用其 sync state / 去重 / 计价管线。
//!
//! ## 数据流
//! ```text
//! ~/.grok/logs/unified.jsonl → 增量解析（按 session 绑定 model）→ 费用计算 → proxy_request_logs
//! ```
//!
//! ## 解析的事件类型（`msg` 字段）
//! - `model catalog: notifying clients` → 绑定 session_id ↔ current_model_id
//! - `shell.turn.inference_done` → 提取 prompt / cached_prompt / completion tokens + latency
//!
//! ## Token 语义
//! Grok 日志的 `prompt_tokens` 含 cached 部分；写入 DB 前已拆成 fresh input + cache_read
//! （与 Claude session 一致），因此 **不** 加入 `CACHE_INCLUSIVE_APP_TYPES`。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const APP_TYPE: &str = "grok";
const PROVIDER_ID: &str = "_grok_session";
const PROVIDER_TYPE: &str = "grok_session";
const DATA_SOURCE: &str = "grok_session";
const DEFAULT_MODEL: &str = "grok-composer-2.5-fast";

/// Grok 配置目录：优先 `GROK_HOME` 环境变量，回退 `~/.grok`。
fn get_grok_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("GROK_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok")
}

/// 归一化 Grok 模型名：小写 + 别名归一。
fn normalize_grok_model(raw: &str) -> String {
    let name = raw.trim().to_lowercase();
    match name.as_str() {
        "grok-build" | "grok-code-fast" | "grok-code-fast-1" | "grok-code-fast-1-0825" => {
            "grok-build-0.1".to_string()
        }
        "grok-composer-2-fast" => "grok-composer-2.5-fast".to_string(),
        _ => name,
    }
}

/// 同步 Grok Build 使用数据（从 ~/.grok/logs/unified.jsonl）
pub fn sync_grok_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let unified = get_grok_config_dir().join("logs").join("unified.jsonl");

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: vec![],
    };

    if !unified.is_file() {
        return Ok(result);
    }
    result.files_scanned = 1;

    match sync_grok_unified_file(db, &unified) {
        Ok((imported, skipped)) => {
            result.imported += imported;
            result.skipped += skipped;
        }
        Err(e) => {
            let msg = format!("Grok 日志解析失败 {}: {e}", unified.display());
            log::warn!("[GROK-SYNC] {msg}");
            result.errors.push(msg);
        }
    }

    if result.imported > 0 {
        log::info!(
            "[GROK-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条",
            result.imported,
            result.skipped
        );
    }

    Ok(result)
}

fn sync_grok_unified_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
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

    let sessions_dir = get_grok_config_dir().join("sessions");
    let mut session_models: HashMap<String, String> = HashMap::new();
    let mut lookup_cache: HashMap<String, Option<String>> = HashMap::new();

    let mut line_offset: i64 = 0;
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    for line_result in reader.lines() {
        line_offset += 1;

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let is_catalog = line.contains("model catalog: notifying clients");
        let is_inference = line.contains("shell.turn.inference_done");
        if !is_catalog && !is_inference {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let msg = match value.get("msg").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => continue,
        };
        let ctx = value.get("ctx");
        let sid = value
            .get("sid")
            .and_then(|v| v.as_str())
            .or_else(|| ctx.and_then(|c| c.get("session_id")).and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        if msg == "model catalog: notifying clients" {
            if let (Some(sid), Some(model)) = (
                sid.as_deref(),
                ctx.and_then(|c| c.get("current_model_id"))
                    .and_then(|v| v.as_str()),
            ) {
                session_models.insert(sid.to_string(), normalize_grok_model(model));
            }
            continue;
        }

        if msg != "shell.turn.inference_done" {
            continue;
        }

        let Some(sid) = sid.as_deref() else {
            continue;
        };

        let ctx = match ctx {
            Some(c) => c,
            None => continue,
        };

        let prompt = ctx.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cached = ctx
            .get("cached_prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let completion = ctx
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let loop_index = ctx.get("loop_index").and_then(|v| v.as_u64()).unwrap_or(0);
        let elapsed = ctx.get("model_elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0);

        if prompt == 0 && completion == 0 {
            continue;
        }

        let model = match session_models.get(sid).cloned() {
            Some(m) => m,
            None => {
                let looked = match lookup_cache.get(sid) {
                    Some(cached_hit) => cached_hit.clone(),
                    None => {
                        let found = session_model_lookup(&sessions_dir, sid);
                        lookup_cache.insert(sid.to_string(), found.clone());
                        found
                    }
                };
                match looked {
                    Some(m) => {
                        let m = normalize_grok_model(&m);
                        session_models.insert(sid.to_string(), m.clone());
                        m
                    }
                    None => DEFAULT_MODEL.to_string(),
                }
            }
        };

        if line_offset <= last_offset {
            continue;
        }

        // prompt_tokens 含 cached 部分；拆成 fresh input + cache_read 再入库。
        let input_tokens = prompt.saturating_sub(cached);
        let cache_read_tokens = cached.min(prompt);
        let output_tokens = completion;

        let request_id = format!("grok_session:{}:{}:{}", sid, loop_index, line_offset);

        let created_at = value
            .get("ts")
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

        match insert_grok_session_entry(
            db,
            &request_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            &model,
            Some(sid),
            elapsed,
            created_at,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[GROK-SYNC] 插入失败 ({}): {e}", request_id);
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, line_offset)?;
    Ok((imported, skipped))
}

fn session_model_lookup(sessions_dir: &Path, session_id: &str) -> Option<String> {
    if !sessions_dir.is_dir() {
        return None;
    }
    for summary in rglob_summary_json(sessions_dir) {
        if let Ok(text) = fs::read_to_string(&summary) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                let info = data.get("info").and_then(|v| v.as_object());
                let matches = info
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|id| id == session_id)
                    .unwrap_or(false);
                if matches {
                    return data
                        .get("current_model_id")
                        .or_else(|| data.get("primaryModelId"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        }
    }
    None
}

fn rglob_summary_json(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_summary_recursive(dir, &mut out, 0, 4);
    out
}

fn collect_summary_recursive(dir: &Path, out: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_summary_recursive(&path, out, depth + 1, max_depth);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("summary.json") {
            out.push(path);
        }
    }
}

fn insert_grok_session_entry(
    db: &Database,
    request_id: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    model: &str,
    session_id: Option<&str>,
    latency_ms: u64,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model,
        input_tokens: input_tokens as u32,
        output_tokens: output_tokens as u32,
        cache_read_tokens: cache_read_tokens as u32,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: input_tokens as u32,
        output_tokens: output_tokens as u32,
        cache_read_tokens: cache_read_tokens as u32,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_grok_pricing(&conn, model);
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
                PROVIDER_ID,
                APP_TYPE,
                model,
                model,
                input_tokens as i64,
                output_tokens as i64,
                cache_read_tokens as i64,
                0i64,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                latency_ms as i64,
                Option::<i64>::None,
                200i64,
                Option::<String>::None,
                session_id.map(|s| s.to_string()),
                Some(PROVIDER_TYPE),
                1i64,
                "1.0",
                created_at,
                DATA_SOURCE,
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Grok 会话日志失败: {e}")))?;

    if inserted_rows > 0 {
        crate::usage_events::notify_log_recorded();
    }

    Ok(true)
}

fn find_grok_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, &normalize_grok_model(model_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_grok_model_aliases() {
        assert_eq!(normalize_grok_model("grok-build"), "grok-build-0.1");
        assert_eq!(normalize_grok_model("grok-code-fast-1"), "grok-build-0.1");
        assert_eq!(
            normalize_grok_model("grok-composer-2-fast"),
            "grok-composer-2.5-fast"
        );
        assert_eq!(
            normalize_grok_model("Grok-Composer-2.5-Fast"),
            "grok-composer-2.5-fast"
        );
        assert_eq!(normalize_grok_model("grok-build-0.1"), "grok-build-0.1");
    }

    #[test]
    fn test_normalize_grok_model_trims_and_lowercases() {
        assert_eq!(normalize_grok_model("  GroK-Build  "), "grok-build-0.1");
    }

    #[test]
    fn test_insert_grok_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "grok-proxy",
                    "xai",
                    "grok",
                    "grok-composer-2.5-fast",
                    "grok-composer-2.5-fast",
                    10,
                    2,
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

        let inserted = insert_grok_session_entry(
            &db,
            "grok-session-dup",
            10,
            2,
            1,
            "grok-composer-2.5-fast",
            Some("session-1"),
            100,
            1000,
        )?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }
}
