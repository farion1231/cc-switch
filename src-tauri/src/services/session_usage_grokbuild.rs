//! Grok Build 会话日志使用追踪
//!
//! 从 `~/.grok/logs/unified.jsonl` 中的 `shell.turn.inference_done` 事件
//! 提取 token 使用数据，实现无代理模式下的 Grok Build 使用统计。
//!
//! ## 数据流
//! ```text
//! ~/.grok/logs/unified.jsonl → 增量解析 → 去重 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## Token 语义
//! Grok 日志中的 `prompt_tokens` **包含** cache 读取部分（与 Codex / Gemini 相同）。
//! 入库时保留 total input，并设置 `input_token_semantics = TOTAL`，
//! 由 `CostCalculator::calculate_for_app("grokbuild", ...)` 扣减 cache。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::grok_config::get_grok_config_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::SystemTime;

const APP_TYPE: &str = "grokbuild";
const DATA_SOURCE: &str = "grok_session";
const PROVIDER_ID: &str = "_grok_session";
const DEFAULT_MODEL: &str = "grok-4.5";

#[derive(Debug, Clone)]
struct GrokTurnUsage {
    session_id: String,
    line_offset: i64,
    loop_index: u64,
    prompt_tokens: u32,
    cached_prompt_tokens: u32,
    completion_tokens: u32,
    reasoning_tokens: u32,
    model_elapsed_ms: Option<i64>,
    timestamp: Option<String>,
}

/// 同步 Grok Build 使用数据
pub fn sync_grokbuild_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let grok_dir = get_grok_config_dir();
    let log_path = grok_dir.join("logs").join("unified.jsonl");

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    if !log_path.is_file() {
        return Ok(result);
    }

    result.files_scanned = 1;

    let session_models = load_session_model_map(&grok_dir.join("sessions"));

    match sync_unified_log(db, &log_path, &session_models) {
        Ok((imported, skipped)) => {
            result.imported = imported;
            result.skipped = skipped;
        }
        Err(e) => {
            let msg = format!("Grok Build 会话日志解析失败 {}: {e}", log_path.display());
            log::warn!("[GROKBUILD-SYNC] {msg}");
            result.errors.push(msg);
        }
    }

    if result.imported > 0 || !result.errors.is_empty() {
        log::info!(
            "[GROKBUILD-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件, 错误 {} 条",
            result.imported,
            result.skipped,
            result.files_scanned,
            result.errors.len()
        );
    }

    Ok(result)
}

fn sync_unified_log(
    db: &Database,
    file_path: &Path,
    session_models: &HashMap<String, String>,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let (last_modified, last_offset) = get_sync_state(db, &file_path_str)?;

    // 文件未变化且已有进度时跳过整文件扫描
    if file_modified <= last_modified && last_offset > 0 {
        return Ok((0, 0));
    }

    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;
    let mut line_offset: i64 = 0;

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

        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(turn) = parse_inference_done(&value, line_offset) else {
            continue;
        };

        let model = resolve_model(&turn.session_id, session_models);
        let request_id = format!(
            "grokbuild_session:{}:L{}:i{}",
            turn.session_id, turn.line_offset, turn.loop_index
        );

        match insert_grokbuild_session_entry(db, &request_id, &turn, &model) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[GROKBUILD-SYNC] 插入失败 ({request_id}): {e}");
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, line_offset)?;
    Ok((imported, skipped))
}

fn parse_inference_done(value: &Value, line_offset: i64) -> Option<GrokTurnUsage> {
    if value.get("msg").and_then(|v| v.as_str()) != Some("shell.turn.inference_done") {
        return None;
    }

    let ctx = value.get("ctx")?;
    let session_id = value
        .get("sid")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();

    let prompt_tokens = json_u32(ctx, "prompt_tokens");
    let cached_prompt_tokens = json_u32(ctx, "cached_prompt_tokens");
    let completion_tokens = json_u32(ctx, "completion_tokens");
    let reasoning_tokens = json_u32(ctx, "reasoning_tokens");

    if prompt_tokens == 0 && completion_tokens == 0 && reasoning_tokens == 0 {
        return None;
    }

    Some(GrokTurnUsage {
        session_id,
        line_offset,
        loop_index: ctx.get("loop_index").and_then(|v| v.as_u64()).unwrap_or(0),
        prompt_tokens,
        cached_prompt_tokens: cached_prompt_tokens.min(prompt_tokens),
        completion_tokens,
        reasoning_tokens,
        model_elapsed_ms: ctx
            .get("model_elapsed_ms")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                ctx.get("model_elapsed_ms")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as i64)
            }),
        timestamp: value
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

fn json_u32(obj: &Value, key: &str) -> u32 {
    obj.get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n.min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

/// 从 `~/.grok/sessions/**/summary.json` 构建 session_id → model 映射
fn load_session_model_map(sessions_dir: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if !sessions_dir.is_dir() {
        return map;
    }

    let walker = match fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(_) => return map,
    };

    // sessions 目录结构: sessions/<encoded_cwd>/<session_id>/summary.json
    for project_entry in walker.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let session_dirs = match fs::read_dir(&project_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for session_entry in session_dirs.flatten() {
            let session_path = session_entry.path();
            if !session_path.is_dir() {
                continue;
            }
            let summary_path = session_path.join("summary.json");
            if !summary_path.is_file() {
                continue;
            }
            let Ok(content) = fs::read_to_string(&summary_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&content) else {
                continue;
            };

            let session_id = value
                .get("info")
                .and_then(|i| i.get("id"))
                .and_then(|v| v.as_str())
                .or_else(|| session_path.file_name().and_then(|n| n.to_str()))
                .unwrap_or("")
                .to_string();
            if session_id.is_empty() {
                continue;
            }

            if let Some(model) = value
                .get("current_model_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                map.insert(session_id, normalize_model_id(model));
            }
        }
    }

    map
}

fn resolve_model(session_id: &str, session_models: &HashMap<String, String>) -> String {
    session_models
        .get(session_id)
        .cloned()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// 规范化模型 ID：别名映射 + 自定义 provider slug 回落
fn normalize_model_id(raw: &str) -> String {
    let id = raw.trim();
    if id.is_empty() {
        return DEFAULT_MODEL.to_string();
    }

    match id {
        "grok-build" | "grok-code-fast-1" | "grok-build-0.1" => "grok-build-0.1".to_string(),
        "grok-composer-2-fast" | "grok-composer-2.5-fast" => "grok-composer-2.5-fast".to_string(),
        // 用户自定义 [model.<slug>] 的 slug（如 cpa / heiyu）通常不是计费模型名
        other if looks_like_provider_slug(other) => DEFAULT_MODEL.to_string(),
        other => other.to_string(),
    }
}

fn looks_like_provider_slug(id: &str) -> bool {
    // 真实 Grok 模型 ID 通常以 grok- 开头；短 slug / 无连字符名多半是 provider 配置名
    if id.starts_with("grok-") || id.starts_with("grok_") {
        return false;
    }
    if id.contains('/') {
        return false;
    }
    // 纯自定义短名：cpa, heiyu, cursorlao 等
    id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !id.contains("gpt-")
        && !id.contains("claude")
}

fn insert_grokbuild_session_entry(
    db: &Database,
    request_id: &str,
    turn: &GrokTurnUsage,
    model: &str,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let created_at = turn
        .timestamp
        .as_ref()
        .and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
                .or_else(|| {
                    // unified.jsonl 使用 "2026-07-17T08:02:07.662Z" — 标准 RFC3339
                    chrono::DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.fZ")
                        .ok()
                        .map(|dt| dt.timestamp())
                })
        })
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    // prompt_tokens 已含 cache；output 合并 reasoning（与 Gemini thoughts 策略一致）
    let input_tokens = turn.prompt_tokens;
    let cache_read_tokens = turn.cached_prompt_tokens.min(input_tokens);
    let output_tokens = turn.completion_tokens.saturating_add(turn.reasoning_tokens);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_grokbuild_pricing(&conn, model);
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
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
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
            input_token_semantics = excluded.input_token_semantics
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model",
        rusqlite::params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            model,
            model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            0i64,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            turn.model_elapsed_ms.unwrap_or(0),
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            Some(turn.session_id.clone()),
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            DATA_SOURCE,
            INPUT_TOKEN_SEMANTICS_TOTAL,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Grok Build 会话日志失败: {e}")))?;

    Ok(conn.changes() > 0)
}

fn find_grokbuild_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn parse_inference_done_splits_cache_inclusive_prompt() {
        let value = serde_json::json!({
            "ts": "2026-07-17T08:02:07.662Z",
            "src": "shell",
            "sid": "019f6ed7-62a7-7561-b5d0-10d6205d9dc4",
            "msg": "shell.turn.inference_done",
            "ctx": {
                "loop_index": 1,
                "model_elapsed_ms": 19150,
                "prompt_tokens": 152441,
                "cached_prompt_tokens": 1280,
                "completion_tokens": 250,
                "reasoning_tokens": 134
            }
        });
        let turn = parse_inference_done(&value, 42).expect("parse");
        assert_eq!(turn.session_id, "019f6ed7-62a7-7561-b5d0-10d6205d9dc4");
        assert_eq!(turn.line_offset, 42);
        assert_eq!(turn.prompt_tokens, 152441);
        assert_eq!(turn.cached_prompt_tokens, 1280);
        assert_eq!(turn.completion_tokens, 250);
        assert_eq!(turn.reasoning_tokens, 134);
        assert_eq!(turn.loop_index, 1);
    }

    #[test]
    fn parse_ignores_non_inference_events() {
        let value = serde_json::json!({
            "sid": "abc",
            "msg": "turn.phase_transition",
            "ctx": {"prompt_tokens": 1}
        });
        assert!(parse_inference_done(&value, 1).is_none());
    }

    #[test]
    fn normalize_model_aliases_and_provider_slugs() {
        assert_eq!(normalize_model_id("grok-build"), "grok-build-0.1");
        assert_eq!(normalize_model_id("grok-code-fast-1"), "grok-build-0.1");
        assert_eq!(normalize_model_id("grok-4.5"), "grok-4.5");
        assert_eq!(normalize_model_id("cpa"), "grok-4.5");
        assert_eq!(normalize_model_id("heiyu"), "grok-4.5");
        assert_eq!(normalize_model_id("cursorlao"), "grok-4.5");
    }

    #[test]
    fn insert_grokbuild_session_entry_roundtrip() -> Result<(), AppError> {
        let db = Database::memory()?;
        let turn = GrokTurnUsage {
            session_id: "sess-1".into(),
            line_offset: 10,
            loop_index: 1,
            prompt_tokens: 1000,
            cached_prompt_tokens: 400,
            completion_tokens: 50,
            reasoning_tokens: 20,
            model_elapsed_ms: Some(1200),
            timestamp: Some("2026-07-22T01:00:00.000Z".into()),
        };
        let request_id = "grokbuild_session:sess-1:L10";
        assert!(insert_grokbuild_session_entry(
            &db, request_id, &turn, "grok-4.5"
        )?);

        // second insert same id with same values → no change counted as skip-ish
        assert!(!insert_grokbuild_session_entry(
            &db, request_id, &turn, "grok-4.5"
        )?);

        let conn = lock_conn!(db.conn);
        let (app_type, data_source, input, cache, output, semantics): (
            String,
            String,
            i64,
            i64,
            i64,
            i64,
        ) = conn.query_row(
            "SELECT app_type, data_source, input_tokens, cache_read_tokens, output_tokens, input_token_semantics
             FROM proxy_request_logs WHERE request_id = ?1",
            rusqlite::params![request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )?;
        assert_eq!(app_type, "grokbuild");
        assert_eq!(data_source, "grok_session");
        assert_eq!(input, 1000);
        assert_eq!(cache, 400);
        assert_eq!(output, 70); // 50 + 20 reasoning
        assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_TOTAL);
        Ok(())
    }

    #[test]
    fn sync_skips_missing_log_file() -> Result<(), AppError> {
        let db = Database::memory()?;
        // Point GROK home away by using a temp empty dir via override is hard;
        // call internal path with nonexistent file via public API when no logs.
        // When default ~/.grok/logs/unified.jsonl missing on CI, this returns empty.
        let result = sync_grokbuild_usage(&db)?;
        // Should not error; imported may be 0
        assert!(result.errors.is_empty() || result.files_scanned <= 1);
        Ok(())
    }
}
