//! Gemini CLI 会话日志使用追踪
//!
//! 从 ~/.gemini/tmp/<project_hash>/chats/session-*.json 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.gemini/tmp/*/chats/session-*.json → 全量解析 → 费用计算 → proxy_request_logs 表
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
use crate::services::agent_session_usage::{
    local_usage_date, RequestCountSemantics, SessionNodeMetadata, SessionRelationClaim,
    TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::session_usage_pipeline::{
    publish_canonical_batch_on_conn, CanonicalReplaceScope, CanonicalUsageBatch,
    UsagePublishTarget, UsageSourceSpec,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use chrono::DateTime;
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const GEMINI_APP_TYPE: &str = "gemini";
const GEMINI_SESSION_PROVIDER_ID: &str = "_gemini_session";
const GEMINI_SESSION_DATA_SOURCE: &str = "gemini_session";

/// 从 Gemini message 中提取的 token 数据
#[derive(Debug)]
struct GeminiTokens {
    input: u32,
    output: u32,
    cached: u32,
    thoughts: u32,
    input_known: bool,
    output_known: bool,
    cached_known: bool,
    thoughts_known: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiInsertOutcome {
    Inserted,
    Existing,
    ProxyDuplicate,
    Rejected,
}

struct GeminiSessionUsagePersistence<'a> {
    file_path: &'a Path,
    started_at: Option<&'a str>,
    last_updated_at: Option<&'a str>,
    session_id: Option<&'a str>,
    canonical_batch: CanonicalUsageBatch,
    file_path_string: &'a str,
    file_modified: i64,
    gemini_msg_count: i64,
}

impl GeminiTokens {
    fn has_any_known_component(&self) -> bool {
        self.input_known || self.output_known || self.cached_known || self.thoughts_known
    }

    fn has_complete_components(&self) -> bool {
        self.input_known && self.output_known && self.cached_known && self.thoughts_known
    }
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

    // 提取顶层 sessionId。Gemini 没有已证明的 parent 字段，因此每个
    // 会话只会声明为 standalone/self-only；project/title/时间不会推断关系。
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // 遍历 messages 数组
    let messages = match value.get("messages").and_then(|v| v.as_array()) {
        Some(msgs) => msgs,
        None => return Ok((0, 0)),
    };

    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;
    let mut gemini_msg_count: i64 = 0;
    let mut canonical_batch = CanonicalUsageBatch::default();

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

        // 提取消息 ID 和模型
        let message_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let timestamp = msg.get("timestamp").and_then(|v| v.as_str());

        // A source-presence-safe event needs a stable session identity, a
        // real RFC3339 event time, and at least one proven token field. Do
        // this admission before the legacy raw insert so invalid events never
        // receive a fabricated `now`/epoch timestamp or zero-only row.
        let Some(session_id) = session_id.as_deref() else {
            continue;
        };
        let Some(event_at) = parse_gemini_event_timestamp(timestamp) else {
            continue;
        };
        if !tokens.has_any_known_component() {
            continue;
        }
        let Some(date) = local_usage_date(event_at) else {
            continue;
        };

        gemini_msg_count += 1;

        // 生成唯一 request_id
        let request_id = format!("gemini_session:{session_id}:{message_id}");

        let admitted = match insert_gemini_session_entry(
            db,
            &request_id,
            &tokens,
            model,
            Some(session_id),
            timestamp,
        ) {
            Ok(GeminiInsertOutcome::Inserted) => {
                imported += 1;
                true
            }
            Ok(GeminiInsertOutcome::Existing) => {
                skipped += 1;
                true
            }
            Ok(GeminiInsertOutcome::ProxyDuplicate) => {
                // A complete event matching a successful proxy row belongs to
                // the proxy source only.  Do not create a second direct fact
                // or coverage marker for the same usage.
                skipped += 1;
                false
            }
            Ok(GeminiInsertOutcome::Rejected) => {
                skipped += 1;
                false
            }
            Err(e) => {
                log::warn!("[GEMINI-SYNC] 插入失败 ({}): {e}", request_id);
                skipped += 1;
                false
            }
        };
        if !admitted {
            continue;
        }
        let source = UsageSourceSpec::new(
            GEMINI_APP_TYPE,
            GEMINI_SESSION_PROVIDER_ID,
            GEMINI_SESSION_DATA_SOURCE,
            UsagePrecision::RequestExact,
            TimeSemantics::EventTime,
            RequestCountSemantics::AssistantMessage,
        );
        // Gemini exposes no cache-creation/write component. Preserve that
        // unknown dimension as NULL; compatibility raw rows may still use
        // their legacy zero columns independently.
        let mut fact = source.fact(date, session_id, model, model, model);
        fact.request_count = Some(1);
        fact.input_tokens = tokens.input_known.then_some(i64::from(tokens.input));
        fact.output_tokens = (tokens.output_known && tokens.thoughts_known)
            .then_some(i64::from(tokens.output.saturating_add(tokens.thoughts)));
        fact.cache_read_tokens = tokens.cached_known.then_some(i64::from(tokens.cached));
        fact.first_event_at = Some(event_at);
        fact.last_event_at = Some(event_at);
        canonical_batch.replace_observe(request_id, fact);
    }

    if let Some(session_id) = session_id.as_deref() {
        canonical_batch.replace_scopes.push(CanonicalReplaceScope {
            app_type: GEMINI_APP_TYPE.to_string(),
            session_id: session_id.to_string(),
            data_source: GEMINI_SESSION_DATA_SOURCE.to_string(),
        });
    }

    persist_gemini_session_usage(
        db,
        GeminiSessionUsagePersistence {
            file_path,
            started_at: value.get("startTime").and_then(|v| v.as_str()),
            last_updated_at: value.get("lastUpdated").and_then(|v| v.as_str()),
            session_id: session_id.as_deref(),
            canonical_batch,
            file_path_string: &file_path_str,
            file_modified,
            gemini_msg_count,
        },
    )?;

    Ok((imported, skipped))
}

fn parse_gemini_event_timestamp(timestamp: Option<&str>) -> Option<i64> {
    timestamp.and_then(|ts| {
        DateTime::parse_from_rfc3339(ts)
            .ok()
            .map(|datetime| datetime.timestamp())
    })
}

fn persist_gemini_session_usage(
    db: &Database,
    input: GeminiSessionUsagePersistence<'_>,
) -> Result<(), AppError> {
    let GeminiSessionUsagePersistence {
        file_path,
        started_at,
        last_updated_at,
        session_id,
        canonical_batch,
        file_path_string,
        file_modified,
        gemini_msg_count,
    } = input;
    let sync_timestamp = unix_now_seconds();
    let mut canonical_batch = canonical_batch;
    if let Some(session_id) = session_id.filter(|id| !id.trim().is_empty()) {
        let mut claim = SessionRelationClaim::standalone(GEMINI_APP_TYPE, session_id);
        claim.metadata = SessionNodeMetadata {
            source_path: Some(file_path.to_string_lossy().to_string()),
            created_at: parse_gemini_event_timestamp(started_at),
            last_active_at: parse_gemini_event_timestamp(last_updated_at),
            last_synced_at: sync_timestamp,
            ..SessionNodeMetadata::default()
        };
        canonical_batch.relation_claims.push(claim);
    }

    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 Gemini canonical 覆盖事务失败: {error}"))
    })?;
    publish_canonical_batch_on_conn(&tx, UsagePublishTarget::Published, canonical_batch)?;
    update_sync_state_on_conn(&tx, file_path_string, file_modified, gemini_msg_count)?;

    tx.commit().map_err(|error| {
        AppError::Database(format!("提交 Gemini canonical 覆盖事务失败: {error}"))
    })?;

    Ok(())
}

fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// 从 tokens JSON 对象中提取 token 数据
fn parse_gemini_tokens(tokens: &serde_json::Value) -> GeminiTokens {
    GeminiTokens {
        input: tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cached: tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        thoughts: tokens.get("thoughts").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        input_known: tokens.get("input").and_then(|v| v.as_u64()).is_some(),
        output_known: tokens.get("output").and_then(|v| v.as_u64()).is_some(),
        cached_known: tokens.get("cached").and_then(|v| v.as_u64()).is_some(),
        thoughts_known: tokens.get("thoughts").and_then(|v| v.as_u64()).is_some(),
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
) -> Result<GeminiInsertOutcome, AppError> {
    if session_id.is_none_or(|id| id.trim().is_empty())
        || parse_gemini_event_timestamp(timestamp).is_none()
        || !tokens.has_any_known_component()
    {
        // Legacy raw rows require non-null numeric columns, but no caller may
        // use those zero placeholders for an event lacking source identity,
        // event time, or any proven token component.
        return Ok(GeminiInsertOutcome::Rejected);
    }
    let conn = lock_conn!(db.conn);

    let created_at = parse_gemini_event_timestamp(timestamp)
        .expect("validated Gemini event timestamp before raw insertion");

    let same_source_exists: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM proxy_request_logs
             WHERE request_id = ?1 AND app_type = ?2 AND data_source = ?3
         )",
        rusqlite::params![request_id, GEMINI_APP_TYPE, GEMINI_SESSION_DATA_SOURCE],
        |row| row.get(0),
    )?;
    if same_source_exists {
        return Ok(GeminiInsertOutcome::Existing);
    }

    // 合并 thoughts 到 output（思考 token 按输出计费）
    let output_tokens = tokens.output.saturating_add(tokens.thoughts);

    let dedup_key = DedupKey {
        app_type: GEMINI_APP_TYPE,
        model,
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        created_at,
    };
    if tokens.has_complete_components()
        && should_skip_session_insert(&conn, request_id, &dedup_key)?
    {
        return Ok(GeminiInsertOutcome::ProxyDuplicate);
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
    // The legacy raw-log schema has non-null cost columns and historically
    // used "0" when no local price was found. The normalized bucket above
    // deliberately keeps this state as `None` instead of copying it.
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app(GEMINI_APP_TYPE, &usage, &p, multiplier);
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
            GEMINI_SESSION_PROVIDER_ID,
            GEMINI_APP_TYPE,
            model,
            model,
            tokens.input,
            output_tokens,
            tokens.cached,
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
            session_id.map(|s| s.to_string()),
            Some(GEMINI_SESSION_DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            GEMINI_SESSION_DATA_SOURCE,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Gemini 会话日志失败: {e}")))?;

    let changed = conn.changes() > 0;
    Ok(if changed {
        GeminiInsertOutcome::Inserted
    } else {
        GeminiInsertOutcome::Existing
    })
}

/// 查找 Gemini 模型定价
fn find_gemini_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
        // Explicit zero fields are source-proven values; the sync adapter may
        // admit this message when its session ID and event time are valid.
        assert!(
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0
        );
    }

    #[test]
    fn missing_session_id_does_not_create_raw_fact_or_marker() -> Result<(), AppError> {
        let db = Database::memory()?;
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session-missing-id.json");
        let value = serde_json::json!({
            "messages": [{
                "id": "message-no-session",
                "type": "gemini",
                "model": "gemini-fixture-known",
                "timestamp": "2026-08-13T01:10:00Z",
                "tokens": {"input": 1}
            }]
        });
        fs::write(
            &path,
            serde_json::to_vec(&value).expect("serialize missing-session fixture"),
        )
        .expect("write missing-session fixture");

        assert_eq!(sync_single_gemini_file(&db, &path)?, (0, 0));
        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'gemini_session'),
                (SELECT COUNT(*) FROM agent_session_usage_rollups WHERE data_source = 'gemini_session'),
                (SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'gemini' AND data_source = 'gemini_session'),
                (SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'gemini')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(counts, (0, 0, 0, 0));
        Ok(())
    }
}
