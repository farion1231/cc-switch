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

use crate::database::{lock_conn, AgentSessionCanonicalCoverageMarker, Database};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{
    normalize_session_relations, write_agent_session_node_on_conn,
    write_agent_session_usage_rollup_on_conn, NormalizedUsageRollup, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use chrono::{DateTime, Local, TimeZone};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const GEMINI_APP_TYPE: &str = "gemini";
const GEMINI_SESSION_PROVIDER_ID: &str = "_gemini_session";
const GEMINI_SESSION_DATA_SOURCE: &str = "gemini_session";

/// A complete durable-bucket key.  The DAO upsert is replacement semantics,
/// so all real source dimensions (including precision/time/count semantics)
/// must be present before a bucket is written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GeminiRollupKey {
    date: String,
    app_type: String,
    session_id: String,
    provider_id: String,
    model: String,
    request_model: String,
    pricing_model: String,
    data_source: String,
    precision: String,
    time_semantics: String,
    request_count_semantics: String,
}

#[derive(Debug, Clone)]
struct GeminiRollupAccumulator {
    request_count: i64,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    input_complete: bool,
    output_complete: bool,
    cache_read_complete: bool,
    first_event_at: Option<i64>,
    last_event_at: Option<i64>,
    request_ids: HashSet<String>,
}

impl Default for GeminiRollupAccumulator {
    fn default() -> Self {
        Self {
            request_count: 0,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            input_complete: true,
            output_complete: true,
            cache_read_complete: true,
            first_event_at: None,
            last_event_at: None,
            request_ids: HashSet::new(),
        }
    }
}

impl GeminiRollupAccumulator {
    fn absorb(&mut self, tokens: &GeminiTokens) {
        self.request_count += 1;
        merge_component(
            &mut self.input_tokens,
            &mut self.input_complete,
            tokens.input_known.then_some(tokens.input),
        );
        merge_component(
            &mut self.output_tokens,
            &mut self.output_complete,
            (tokens.output_known && tokens.thoughts_known)
                .then_some(tokens.output.saturating_add(tokens.thoughts)),
        );
        merge_component(
            &mut self.cache_read_tokens,
            &mut self.cache_read_complete,
            tokens.cached_known.then_some(tokens.cached),
        );
    }
}

fn merge_component(slot: &mut Option<i64>, complete: &mut bool, value: Option<u32>) {
    let Some(value) = value else {
        *complete = false;
        *slot = None;
        return;
    };
    if *complete {
        *slot = Some(slot.unwrap_or_default().saturating_add(i64::from(value)));
    }
}

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
    let mut rollups: HashMap<GeminiRollupKey, GeminiRollupAccumulator> = HashMap::new();

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
        let Some(date) = gemini_rollup_date(event_at) else {
            continue;
        };

        gemini_msg_count += 1;

        // 生成唯一 request_id
        let request_id = format!("gemini_session:{session_id}:{message_id}");

        match insert_gemini_session_entry(
            db,
            &request_id,
            &tokens,
            model,
            Some(session_id),
            timestamp,
        ) {
            Ok(GeminiInsertOutcome::Inserted) => imported += 1,
            Ok(GeminiInsertOutcome::Existing) => skipped += 1,
            Ok(GeminiInsertOutcome::ProxyDuplicate) => {
                // A complete event matching a successful proxy row belongs to
                // the proxy source only.  Do not create a second direct fact
                // or coverage marker for the same usage.
                skipped += 1;
                continue;
            }
            Ok(GeminiInsertOutcome::Rejected) => {
                skipped += 1;
                continue;
            }
            Err(e) => {
                log::warn!("[GEMINI-SYNC] 插入失败 ({}): {e}", request_id);
                skipped += 1;
            }
        }

        let key = GeminiRollupKey {
            date,
            app_type: GEMINI_APP_TYPE.to_string(),
            session_id: session_id.to_string(),
            provider_id: GEMINI_SESSION_PROVIDER_ID.to_string(),
            model: model.to_string(),
            request_model: model.to_string(),
            // Preserve the model as the pricing identity. Gemini has no
            // cache-creation component, so canonical cost remains NULL even
            // when a local price row exists.
            pricing_model: model.to_string(),
            data_source: GEMINI_SESSION_DATA_SOURCE.to_string(),
            precision: UsagePrecision::RequestExact.as_str().to_string(),
            time_semantics: TimeSemantics::EventTime.as_str().to_string(),
            request_count_semantics: RequestCountSemantics::AssistantMessage.as_str().to_string(),
        };
        let accumulator = rollups.entry(key).or_default();
        accumulator.request_ids.insert(request_id);
        accumulator.absorb(&tokens);
        accumulator.first_event_at = Some(
            accumulator
                .first_event_at
                .map_or(event_at, |first| first.min(event_at)),
        );
        accumulator.last_event_at = Some(
            accumulator
                .last_event_at
                .map_or(event_at, |last| last.max(event_at)),
        );
    }

    persist_gemini_session_usage(
        db,
        file_path,
        value.get("startTime").and_then(|v| v.as_str()),
        value.get("lastUpdated").and_then(|v| v.as_str()),
        session_id.as_deref(),
        rollups,
    )?;

    // 更新同步状态
    update_sync_state(db, &file_path_str, file_modified, gemini_msg_count)?;

    Ok((imported, skipped))
}

fn parse_gemini_event_timestamp(timestamp: Option<&str>) -> Option<i64> {
    timestamp.and_then(|ts| {
        DateTime::parse_from_rfc3339(ts)
            .ok()
            .map(|datetime| datetime.timestamp())
    })
}

fn gemini_rollup_date(event_at: i64) -> Option<String> {
    Local
        .timestamp_opt(event_at, 0)
        .single()
        .map(|datetime| datetime.date_naive().format("%Y-%m-%d").to_string())
}

fn persist_gemini_session_usage(
    db: &Database,
    file_path: &Path,
    started_at: Option<&str>,
    last_updated_at: Option<&str>,
    session_id: Option<&str>,
    rollups: HashMap<GeminiRollupKey, GeminiRollupAccumulator>,
) -> Result<(), AppError> {
    let Some(session_id) = session_id.filter(|id| !id.trim().is_empty()) else {
        // A malformed/legacy file without a top-level sessionId has no safe
        // durable session identity to persist.
        return Ok(());
    };
    let sync_timestamp = unix_now_seconds();

    let mut claim = SessionRelationClaim::standalone(GEMINI_APP_TYPE, session_id);
    claim.metadata = SessionNodeMetadata {
        source_path: Some(file_path.to_string_lossy().to_string()),
        created_at: parse_gemini_event_timestamp(started_at),
        last_active_at: parse_gemini_event_timestamp(last_updated_at),
        last_synced_at: sync_timestamp,
        ..SessionNodeMetadata::default()
    };
    let node = normalize_session_relations(&[claim])?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::InvalidInput("Gemini 会话节点规范化失败".into()))?;

    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 Gemini canonical 覆盖事务失败: {error}"))
    })?;
    write_agent_session_node_on_conn(&tx, &node)?;

    for (key, aggregate) in rollups {
        let rollup = NormalizedUsageRollup {
            date: key.date,
            app_type: key.app_type,
            session_id: key.session_id,
            provider_id: key.provider_id,
            model: key.model,
            request_model: key.request_model,
            pricing_model: key.pricing_model,
            data_source: key.data_source,
            precision: UsagePrecision::RequestExact,
            time_semantics: TimeSemantics::EventTime,
            request_count_semantics: RequestCountSemantics::AssistantMessage,
            request_count: Some(aggregate.request_count),
            input_tokens: aggregate.input_tokens,
            output_tokens: aggregate.output_tokens,
            cache_read_tokens: aggregate.cache_read_tokens,
            // Gemini exposes no cache-creation/write component. Preserve
            // that unknown dimension as NULL; compatibility raw rows may
            // still use their legacy zero columns independently.
            cache_creation_tokens: None,
            // A complete local cost cannot be proven without that component.
            total_cost_usd: None,
            first_event_at: aggregate.first_event_at,
            last_event_at: aggregate.last_event_at,
        };
        // The bucket must be durable before its raw-request coverage markers;
        // both writes commit atomically below.
        write_agent_session_usage_rollup_on_conn(&tx, &rollup)?;
        for request_id in aggregate.request_ids {
            let marker = AgentSessionCanonicalCoverageMarker {
                app_type: rollup.app_type.clone(),
                data_source: rollup.data_source.clone(),
                request_id,
                canonical_session_id: Some(rollup.session_id.clone()),
                marked_at: sync_timestamp,
            };
            Database::upsert_agent_session_canonical_coverage_on_conn(&tx, &marker)?;
        }
    }

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
        // The legacy raw-log schema has non-null cost columns and historically
        // used "0" when no local price was found. The normalized bucket above
        // deliberately keeps this state as `None` instead of copying it.
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
            GEMINI_SESSION_PROVIDER_ID, // provider_id
            GEMINI_APP_TYPE,      // app_type
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
            Some(GEMINI_SESSION_DATA_SOURCE), // provider_type
            1i64,                // is_streaming
            "1.0",               // cost_multiplier
            created_at,
            GEMINI_SESSION_DATA_SOURCE, // data_source
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Gemini 会话日志失败: {e}")))?;

    // changes() > 0 表示新插入或已更新，== 0 表示值完全相同（无实际变更）
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

    fn seed_pricing(db: &Database, model: &str, input: &str, output: &str, cache: &str) {
        let conn = db
            .conn
            .lock()
            .expect("lock anonymous Gemini pricing database");
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million,
                output_cost_per_million, cache_read_cost_per_million,
                cache_creation_cost_per_million
             ) VALUES (?1, ?1, ?2, ?3, ?4, '0')",
            rusqlite::params![model, input, output, cache],
        )
        .expect("seed anonymous Gemini pricing");
    }

    fn write_fixture(path: &Path, session_id: &str, messages: serde_json::Value) {
        let value = serde_json::json!({
            "sessionId": session_id,
            "startTime": "2026-08-13T01:00:00Z",
            "lastUpdated": "2026-08-13T02:00:00Z",
            "messages": messages,
        });
        fs::write(
            path,
            serde_json::to_vec(&value).expect("serialize anonymous fixture"),
        )
        .expect("write anonymous Gemini fixture");
    }

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
            input_known: true,
            output_known: true,
            cached_known: true,
            thoughts_known: true,
        };
        let inserted = insert_gemini_session_entry(
            &db,
            "gemini-session-dup",
            &tokens,
            "gemini-2.5-pro",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
        )?;
        assert_eq!(inserted, GeminiInsertOutcome::ProxyDuplicate);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn proxy_match_is_not_canonicalized_but_cache_mismatch_is() -> Result<(), AppError> {
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
                    "proxy-exact-gemini",
                    "google",
                    "gemini",
                    "gemini-proxy-match",
                    "gemini-proxy-match",
                    10,
                    7,
                    1,
                    0,
                    "0.01",
                    100,
                    200,
                    1005,
                    "proxy"
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "proxy-cache-mismatch",
                    "google",
                    "gemini",
                    "gemini-cache-mismatch",
                    "gemini-cache-mismatch",
                    10,
                    7,
                    9,
                    0,
                    "0.01",
                    100,
                    200,
                    1200,
                    "proxy"
                ],
            )?;
        }

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("proxy-arbitration.json");
        write_fixture(
            &path,
            "session-proxy-arbitration",
            serde_json::json!([
                {
                    "id": "message-exact",
                    "type": "gemini",
                    "model": "gemini-proxy-match",
                    "timestamp": "1970-01-01T00:16:45Z",
                    "tokens": {"input": 10, "output": 2, "cached": 1, "thoughts": 5}
                },
                {
                    "id": "message-cache-mismatch",
                    "type": "gemini",
                    "model": "gemini-cache-mismatch",
                    "timestamp": "1970-01-01T00:20:00Z",
                    "tokens": {"input": 10, "output": 2, "cached": 1, "thoughts": 5}
                }
            ]),
        );

        assert_eq!(
            sync_single_gemini_file(&db, &path)?,
            (1, 1),
            "only the cache-mismatched event should be imported locally"
        );
        let conn = lock_conn!(db.conn);
        let local_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE app_type = 'gemini' AND data_source = 'gemini_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(local_rows, 1);
        let canonical_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'gemini' AND data_source = 'gemini_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(canonical_rows, 1);
        let exact_bucket: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'gemini' AND model = 'gemini-proxy-match'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(exact_bucket, 0);
        let marker_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT request_id FROM agent_session_canonical_coverage
                 WHERE app_type = 'gemini' AND data_source = 'gemini_session'
                 ORDER BY request_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        assert_eq!(
            marker_ids,
            vec!["gemini_session:session-proxy-arbitration:message-cache-mismatch".to_string()]
        );
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
        // Compatibility values remain zero, but presence flags preserve the
        // distinction needed by canonical nullable facts.
        let json: serde_json::Value = serde_json::json!({
            "input": 100,
            "output": 50
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 100);
        assert_eq!(tokens.output, 50);
        assert_eq!(tokens.cached, 0);
        assert_eq!(tokens.thoughts, 0);
        assert!(!tokens.has_complete_components());
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
        // The cached field is explicitly present, so this event has source
        // evidence even though the other components are zero.
        let should_skip =
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0;
        assert!(!should_skip, "纯缓存命中记录不应被跳过");
    }

    #[test]
    fn sync_persists_aggregated_session_buckets_and_preserves_cost_states() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        seed_pricing(&db, "gemini-fixture-known", "1", "2", "3");
        seed_pricing(&db, "gemini-fixture-zero", "0", "0", "0");
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session-fixture-a.json");
        write_fixture(
            &path,
            "session-fixture-a",
            serde_json::json!([
                {
                    "id": "message-1",
                    "type": "gemini",
                    "model": "gemini-fixture-known",
                    "timestamp": "2026-08-13T01:10:00Z",
                    "tokens": {"input": 100, "output": 20, "cached": 30, "thoughts": 5}
                },
                {
                    "id": "message-2",
                    "type": "gemini",
                    "model": "gemini-fixture-known",
                    "timestamp": "2026-08-13T01:20:00Z",
                    "tokens": {"input": 50, "output": 10, "cached": 0, "thoughts": 2}
                },
                {
                    "id": "message-unknown-cost",
                    "type": "gemini",
                    "model": "gemini-fixture-unknown",
                    "timestamp": "2026-08-13T01:30:00Z",
                    "tokens": {"input": 7, "output": 3, "cached": 1, "thoughts": 0}
                },
                {
                    "id": "message-zero-cost",
                    "type": "gemini",
                    "model": "gemini-fixture-zero",
                    "timestamp": "2026-08-13T01:40:00Z",
                    "tokens": {"input": 5, "output": 1, "cached": 0, "thoughts": 0}
                },
                {
                    "id": "message-no-timestamp",
                    "type": "gemini",
                    "model": "gemini-fixture-known",
                    "tokens": {"input": 11, "output": 1, "cached": 0, "thoughts": 0}
                },
                {
                    "id": "message-unknown-token-component",
                    "type": "gemini",
                    "model": "gemini-fixture-known",
                    "timestamp": "2026-08-13T01:45:00Z",
                    "tokens": {"input": 12, "output": 1}
                },
                {
                    "id": "message-zero-usage",
                    "type": "gemini",
                    "model": "gemini-fixture-zero",
                    "timestamp": "2026-08-13T01:50:00Z",
                    "tokens": {"input": 0, "output": 0, "cached": 0, "thoughts": 0}
                },
                {
                    "id": "message-allunknown",
                    "type": "gemini",
                    "model": "gemini-fixture-known",
                    "timestamp": "2026-08-13T01:55:00Z",
                    "tokens": {}
                }
            ]),
        );

        let first = sync_single_gemini_file(&db, &path)?;
        // Six messages have a valid session/time and at least one source-known
        // token component.  The missing-time and all-unknown messages are
        // rejected before legacy raw insertion.
        assert_eq!(first, (6, 0));

        let conn = lock_conn!(db.conn);
        let request_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'gemini_session' ORDER BY request_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        assert_eq!(request_ids.len(), 6);
        assert!(request_ids
            .iter()
            .all(|id| id.starts_with("gemini_session:session-fixture-a:")));
        assert!(request_ids.iter().any(|id| id.ends_with(":message-1")));
        assert!(!request_ids
            .iter()
            .any(|id| id.ends_with(":message-no-timestamp")));
        assert!(!request_ids
            .iter()
            .any(|id| id.ends_with(":message-allunknown")));

        let known: (
            i64,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            i64,
            i64,
        ) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd, first_event_at, last_event_at
             FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a' AND model = 'gemini-fixture-known'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )?;
        assert_eq!(known.0, 3);
        assert_eq!(known.1, Some(162));
        // The partial message lacks thoughts, so output is unknown for the
        // whole bucket instead of treating thoughts as zero.
        assert_eq!(known.2, None);
        // The partial message lacks cached, so cache-read is unknown for the
        // whole bucket instead of summing a fabricated zero.
        assert_eq!(known.3, None);
        // Gemini has no cache-creation/write field; canonical storage must
        // preserve that unknown component and remain partial.
        assert_eq!(known.4, None);
        assert_eq!(known.5, None);
        assert!(known.6 < known.7);

        let semantics: (String, String, String, String) = conn.query_row(
            "SELECT data_source, precision, time_semantics, request_count_semantics
             FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a' AND model = 'gemini-fixture-known'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            semantics,
            (
                GEMINI_SESSION_DATA_SOURCE.into(),
                "request_exact".into(),
                "event_time".into(),
                "assistant_message".into()
            )
        );

        let unknown_cost: Option<String> = conn.query_row(
            "SELECT total_cost_usd FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a' AND model = 'gemini-fixture-unknown'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(unknown_cost, None);

        let zero_cost: (Option<i64>, Option<String>, String) = conn.query_row(
            "SELECT cache_read_tokens, total_cost_usd,
                    (SELECT total_cost_usd FROM proxy_request_logs
                     WHERE request_id = 'gemini_session:session-fixture-a:message-zero-cost')
             FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a' AND model = 'gemini-fixture-zero'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        // Explicit cached=0 remains known in the canonical bucket, while the
        // absent cache-creation field keeps canonical cost NULL. Raw
        // compatibility columns may still retain their historical "0" cost.
        assert_eq!(zero_cost.0, Some(0));
        assert_eq!(zero_cost.1, None);
        assert_eq!(zero_cost.2, "0");

        let bucket_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a'",
            [],
            |row| row.get(0),
        )?;
        // The complete and partial known-model messages share one bucket;
        // unknown and known-zero each get their own source/model bucket. The
        // timestamp-less and all-unknown messages are absent from durable
        // EventTime storage, while the partial component message is retained.
        assert_eq!(bucket_count, 3);

        let marker_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT request_id FROM agent_session_canonical_coverage
                 WHERE app_type = 'gemini' AND data_source = 'gemini_session'
                 ORDER BY request_id",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        assert_eq!(marker_ids.len(), 6);
        assert!(marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-1" }));
        assert!(marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-2" }));
        assert!(marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-unknown-cost" }));
        assert!(marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-zero-cost" }));
        assert!(marker_ids.iter().any(|id| {
            id == "gemini_session:session-fixture-a:message-unknown-token-component"
        }));
        assert!(marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-zero-usage" }));
        assert!(!marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-no-timestamp" }));
        assert!(!marker_ids
            .iter()
            .any(|id| { id == "gemini_session:session-fixture-a:message-allunknown" }));
        let marker_session: String = conn.query_row(
            "SELECT canonical_session_id FROM agent_session_canonical_coverage
             WHERE app_type = 'gemini' AND data_source = 'gemini_session'
               AND request_id = 'gemini_session:session-fixture-a:message-1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_session, "session-fixture-a");

        // The 01:45 message has valid input/output evidence but no cached or
        // thoughts fields. It is represented by the known-model bucket and
        // covered exactly once rather than dropped as an all-zero record.
        let partial_marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'gemini' AND data_source = 'gemini_session'
               AND request_id = 'gemini_session:session-fixture-a:message-unknown-token-component'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(partial_marker_count, 1);

        let node: (Option<String>, String, String, String) = conn.query_row(
            "SELECT parent_session_id, root_session_id, node_kind, relation_confidence
             FROM agent_session_nodes
             WHERE app_type = 'gemini' AND session_id = 'session-fixture-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            node,
            (
                None,
                "session-fixture-a".into(),
                "standalone".into(),
                "unavailable".into()
            )
        );

        // Incremental cursor keeps a repeated sync from drifting or replacing
        // the already aggregated bucket with a single last message.
        drop(conn);
        let second = sync_single_gemini_file(&db, &path)?;
        assert_eq!(second, (0, 0));
        let conn = lock_conn!(db.conn);
        let stable: (i64, Option<i64>, Option<i64>) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens
             FROM agent_session_usage_rollups
             WHERE session_id = 'session-fixture-a' AND model = 'gemini-fixture-known'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(stable, (3, Some(162), None));
        let stable_markers: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'gemini' AND data_source = 'gemini_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stable_markers, 6);
        Ok(())
    }

    #[test]
    fn nearby_gemini_sessions_are_independent_standalone_nodes() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_pricing(&db, "gemini-fixture-known", "1", "2", "3");
        let dir = tempdir().expect("tempdir");
        let first_path = dir.path().join("session-first.json");
        let second_path = dir.path().join("session-second.json");
        let messages = serde_json::json!([{
            "id": "message-1",
            "type": "gemini",
            "model": "gemini-fixture-known",
            "timestamp": "2026-08-13T01:10:00Z",
            "tokens": {"input": 1, "output": 1, "cached": 0, "thoughts": 0}
        }]);
        write_fixture(&first_path, "session-first", messages.clone());
        write_fixture(&second_path, "session-second", messages);

        sync_single_gemini_file(&db, &first_path)?;
        sync_single_gemini_file(&db, &second_path)?;

        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT session_id, parent_session_id, root_session_id, node_kind
             FROM agent_session_nodes WHERE app_type = 'gemini' ORDER BY session_id",
        )?;
        let nodes = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].1, None);
        assert_eq!(nodes[0].0, nodes[0].2);
        assert_eq!(nodes[0].3, "standalone");
        assert_eq!(nodes[1].1, None);
        assert_eq!(nodes[1].0, nodes[1].2);
        assert_eq!(nodes[1].3, "standalone");
        Ok(())
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
