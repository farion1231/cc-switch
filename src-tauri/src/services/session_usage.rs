//! Claude Code 会话日志使用追踪
//!
//! 从 ~/.claude/projects/ 下的 JSONL 会话文件中提取 token 使用数据，
//! 实现无代理模式下的使用统计。
//!
//! ## 数据流
//! ```text
//! ~/.claude/projects/*/*.jsonl → 增量解析 → 去重 → proxy_request_logs +
//! 规范化会话节点 / 日用量桶
//! ```

use crate::config::get_claude_config_dir;
use crate::database::{lock_conn, AgentSessionCanonicalCoverageMarker, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{
    normalize_session_relations, write_agent_session_node,
    write_agent_session_usage_rollup_fact_on_conn, write_agent_session_usage_rollup_on_conn,
    NormalizedSessionNode, NormalizedUsageRollup, NormalizedUsageRollupFact, RelationClaim,
    RelationConfidence, RequestCountSemantics, SessionNodeMetadata, SessionRelationClaim,
    TimeSemantics, UsagePrecision,
};
use crate::services::usage_stats::{
    effective_usage_log_filter, find_model_pricing, has_matching_proxy_usage_coverage_for_session,
    session_insert_outcome, session_insert_outcome_excluding_claimed, DedupKey,
    MatchingProxyUsageLog, SessionInsertOutcome,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::SystemTime;

/// 同步结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncResult {
    pub imported: u32,
    pub skipped: u32,
    pub files_scanned: u32,
    pub suspected_duplicates: u32,
    pub deferred_files: u32,
    pub errors: Vec<String>,
}

impl SessionSyncResult {
    pub fn merge(&mut self, other: SessionSyncResult) {
        self.imported = self.imported.saturating_add(other.imported);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.files_scanned = self.files_scanned.saturating_add(other.files_scanned);
        self.suspected_duplicates = self
            .suspected_duplicates
            .saturating_add(other.suspected_duplicates);
        self.deferred_files = self.deferred_files.saturating_add(other.deferred_files);
        self.errors.extend(other.errors);
    }
}

pub fn session_sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn merge_sync_step(
    aggregate: &mut SessionSyncResult,
    name: &str,
    step: Result<SessionSyncResult, AppError>,
) {
    match step {
        Ok(result) => aggregate.merge(result),
        Err(error) => aggregate.errors.push(format!("{name} 同步失败: {error}")),
    }
}

/// 调用方必须持有 [`session_sync_mutex`]。此函数是同步内核，供后台任务、
/// 手动同步和 Codex 重建共享，避免 tokio Mutex 重入。
pub fn sync_all_unlocked(db: &Database) -> SessionSyncResult {
    let mut result = SessionSyncResult::default();
    merge_sync_step(&mut result, "Claude", sync_claude_session_logs(db));
    merge_sync_step(
        &mut result,
        "Codex",
        crate::services::session_usage_codex::sync_codex_usage_with_replay(db),
    );
    merge_sync_step(
        &mut result,
        "Gemini",
        crate::services::session_usage_gemini::sync_gemini_usage(db),
    );
    merge_sync_step(
        &mut result,
        "OpenCode",
        crate::services::session_usage_opencode::sync_opencode_usage(db),
    );
    merge_sync_step(
        &mut result,
        "Grok Build",
        crate::services::session_usage_grokbuild::sync_grokbuild_usage(db),
    );
    merge_sync_step(
        &mut result,
        "Pi",
        crate::services::session_usage_pi::sync_pi_usage(db),
    );
    merge_sync_step(&mut result, "OpenClaw", sync_openclaw_session_nodes(db));
    merge_sync_step(&mut result, "Hermes", sync_hermes_session_nodes(db));
    merge_sync_step(
        &mut result,
        "Claude Desktop / Cowork",
        sync_cowork_session_usage(db),
    );
    notify_sync_result(&result);
    result
}

/// Register OpenClaw's node-only source in the shared sync pass.
///
/// The OpenClaw adapter intentionally reports unavailable usage.  We still
/// persist its namespaced standalone nodes so session discovery remains
/// truthful, but never create a zero-valued usage bucket.
fn sync_openclaw_session_nodes(db: &Database) -> Result<SessionSyncResult, AppError> {
    let scan = crate::services::session_usage_openclaw::sync_openclaw_session_usage(
        crate::openclaw_config::get_openclaw_dir(),
    );
    let claims = scan.claims();
    let normalized = normalize_session_relations(&claims)?;
    for node in &normalized {
        write_agent_session_node(db, node)?;
    }

    Ok(SessionSyncResult {
        skipped: scan.files_skipped,
        files_scanned: scan.files_scanned,
        errors: scan.errors,
        ..SessionSyncResult::default()
    })
}

/// Register Hermes' detailed read-only source in the shared sync pass.
///
/// The adapter owns the full-dimension fact/snapshot transaction.  Integration
/// persists only standalone session nodes and returns the adapter's own
/// imported/skipped/error accounting; Hermes facts are never flattened into
/// `proxy_request_logs` or written a second time here.
fn sync_hermes_session_nodes(db: &Database) -> Result<SessionSyncResult, AppError> {
    let detailed = crate::services::session_usage_hermes::sync_hermes_usage_detailed(db)?;
    let claims = crate::services::session_usage_hermes::hermes_standalone_session_claims(&detailed);
    let normalized = normalize_session_relations(&claims)?;
    for node in &normalized {
        write_agent_session_node(db, node)?;
    }

    Ok(detailed.as_session_sync_result())
}

/// Register Cowork's canonical `claude-desktop` source.  Its adapter emits
/// canonical buckets only; proxy/raw arbitration remains a central concern
/// and is exposed through the shared usage filters below. Cowork emits no
/// `cowork_session` compatibility rows, so coverage markers are intentionally
/// not fabricated for this source.
fn sync_cowork_session_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let cowork = crate::services::session_usage_cowork::sync_cowork_usage(db)?;
    let shadowed = arbitrate_cowork_proxy_rows(db, &cowork.source_records)?;
    Ok(SessionSyncResult {
        imported: cowork.imported.saturating_sub(shadowed),
        skipped: cowork.skipped.saturating_add(shadowed),
        files_scanned: cowork.files_scanned,
        errors: cowork.errors,
        ..SessionSyncResult::default()
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CoworkCanonicalBucketKey {
    date: String,
    session_id: String,
    model: String,
}

#[derive(Debug, Clone, Default)]
struct CoworkCanonicalBucket {
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    first_event_at: Option<i64>,
    last_event_at: Option<i64>,
}

/// Apply the central Desktop gateway arbitration after the Cowork adapter has
/// parsed its transcript.  A matching successful `claude-desktop` proxy row
/// wins for that exact event; cache-token differences keep the transcript
/// event visible.  Grok's face-value turn semantics never pass through this
/// helper or a generic fingerprint. The canonical rewrite stays on the
/// v20-compatible generic bridge and does not create raw-coverage markers:
/// Cowork has no same-source raw lifecycle to suppress.
fn arbitrate_cowork_proxy_rows(
    db: &Database,
    source_records: &[crate::services::session_usage_cowork::CoworkSourceIdentity],
) -> Result<u32, AppError> {
    if source_records.is_empty() {
        return Ok(0);
    }

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 Cowork 来源仲裁事务失败: {error}")))?;
    let mut buckets: BTreeMap<CoworkCanonicalBucketKey, CoworkCanonicalBucket> = BTreeMap::new();
    let mut touched = BTreeMap::new();
    let mut shadowed = 0u32;

    for record in source_records {
        let key = CoworkCanonicalBucketKey {
            date: chrono::DateTime::<chrono::Utc>::from_timestamp(record.event_at, 0)
                .map(|value| {
                    value
                        .with_timezone(&chrono::Local)
                        .date_naive()
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .ok_or_else(|| {
                    AppError::InvalidInput(format!(
                        "Cowork source event has invalid timestamp: {}",
                        record.event_at
                    ))
                })?,
            session_id: record.session_id.clone(),
            model: record.model.clone(),
        };
        touched.insert(key.clone(), ());

        if crate::services::usage_stats::has_matching_cowork_proxy_usage(
            &tx,
            &record.model,
            record.event_at,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_creation_tokens,
        )? {
            shadowed = shadowed.saturating_add(1);
            continue;
        }

        let bucket = buckets.entry(key).or_default();
        bucket.request_count = bucket.request_count.saturating_add(1);
        bucket.input_tokens = bucket.input_tokens.saturating_add(record.input_tokens);
        bucket.output_tokens = bucket.output_tokens.saturating_add(record.output_tokens);
        bucket.cache_read_tokens = bucket
            .cache_read_tokens
            .saturating_add(record.cache_read_tokens);
        bucket.cache_creation_tokens = bucket
            .cache_creation_tokens
            .saturating_add(record.cache_creation_tokens);
        bucket.first_event_at = Some(
            bucket
                .first_event_at
                .map_or(record.event_at, |value| value.min(record.event_at)),
        );
        bucket.last_event_at = Some(
            bucket
                .last_event_at
                .map_or(record.event_at, |value| value.max(record.event_at)),
        );
    }

    for (key, _) in touched {
        tx.execute(
            "DELETE FROM agent_session_usage_rollups
             WHERE date = ?1 AND app_type = 'claude-desktop'
               AND session_id = ?2 AND provider_id = 'cowork_session'
               AND model = ?3 AND request_model = ?3 AND pricing_model = ?3
               AND data_source = 'cowork_session'
               AND precision = 'request_exact'
               AND time_semantics = 'event_time'
               AND request_count_semantics = 'assistant_message'",
            rusqlite::params![&key.date, &key.session_id, &key.model],
        )
        .map_err(|error| AppError::Database(format!("清理 Cowork 仲裁桶失败: {error}")))?;

        if let Some(bucket) = buckets.remove(&key) {
            let rollup = NormalizedUsageRollup {
                date: key.date,
                app_type: "claude-desktop".to_string(),
                session_id: key.session_id,
                provider_id: "cowork_session".to_string(),
                model: key.model.clone(),
                request_model: key.model.clone(),
                pricing_model: key.model,
                data_source: "cowork_session".to_string(),
                precision: UsagePrecision::RequestExact,
                time_semantics: TimeSemantics::EventTime,
                request_count_semantics: RequestCountSemantics::AssistantMessage,
                request_count: Some(bucket.request_count),
                input_tokens: Some(bucket.input_tokens),
                output_tokens: Some(bucket.output_tokens),
                cache_read_tokens: Some(bucket.cache_read_tokens),
                cache_creation_tokens: Some(bucket.cache_creation_tokens),
                total_cost_usd: None,
                first_event_at: bucket.first_event_at,
                last_event_at: bucket.last_event_at,
            };
            write_agent_session_usage_rollup_on_conn(&tx, &rollup)?;
        }
    }

    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Cowork 来源仲裁事务失败: {error}")))?;
    Ok(shadowed)
}

pub(crate) fn notify_sync_result(result: &SessionSyncResult) {
    if result.imported > 0 {
        crate::usage_events::notify_log_recorded();
    }
}

/// 数据来源分布
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSourceSummary {
    pub data_source: String,
    pub request_count: u32,
    pub total_cost_usd: String,
}

/// 从 JSONL 中解析出的 assistant 消息使用数据
#[derive(Debug, Clone)]
struct ParsedAssistantUsage {
    message_id: String,
    model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    stop_reason: Option<String>,
    timestamp: Option<String>,
    session_id: Option<String>,
}

fn parse_token_component(usage: &serde_json::Value, field: &str) -> Option<i64> {
    usage
        .get(field)
        .and_then(|value| value.as_u64())
        .and_then(|value| i64::try_from(value).ok())
}

fn has_source_token_component(message: &ParsedAssistantUsage) -> bool {
    [
        message.input_tokens,
        message.output_tokens,
        message.cache_read_tokens,
        message.cache_creation_tokens,
    ]
    .iter()
    .any(Option::is_some)
}

fn all_source_token_components_known(message: &ParsedAssistantUsage) -> bool {
    [
        message.input_tokens,
        message.output_tokens,
        message.cache_read_tokens,
        message.cache_creation_tokens,
    ]
    .iter()
    .all(Option::is_some)
}

fn token_value_for_raw(value: Option<i64>) -> u32 {
    value.unwrap_or(0).clamp(0, u32::MAX as i64) as u32
}

/// One Claude transcript and the structural relation proven by its path.
///
/// The identity is discovered before incremental parsing so a cursor that
/// starts after the first line still retains the JSONL `sessionId` priority.
#[derive(Debug, Clone)]
struct ClaudeFileIdentity {
    session_id: String,
    claim: SessionRelationClaim,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClaudeRollupKey {
    date: String,
    session_id: String,
    provider_id: String,
    model: String,
    request_model: String,
    pricing_model: String,
    data_source: String,
}

#[derive(Debug, Clone)]
struct PreparedClaudeMessage {
    request_id: String,
    message: ParsedAssistantUsage,
    raw_exists: bool,
    matched_proxy: Option<MatchingProxyUsageLog>,
    skip_raw: bool,
}

/// 同步 Claude Code 会话日志到使用统计数据库
pub fn sync_claude_session_logs(db: &Database) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_claude_config_dir().join("projects");
    sync_claude_files(db, &projects_dir)
}

/// Sync a Claude projects tree after collecting every relation claim in the
/// batch.  Normalizing the complete claim set first is important: a child may
/// be visited before its root file and must not be permanently downgraded to
/// `unknown` merely because the parent had not been seen yet.
fn sync_claude_files(db: &Database, projects_dir: &Path) -> Result<SessionSyncResult, AppError> {
    if !projects_dir.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            suspected_duplicates: 0,
            deferred_files: 0,
            errors: vec![],
        });
    }

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    // 收集所有 .jsonl 文件
    let jsonl_files = collect_jsonl_files(projects_dir);

    let mut files = Vec::with_capacity(jsonl_files.len());
    let mut claims = Vec::with_capacity(jsonl_files.len());
    for path in jsonl_files {
        // Workflow journals are bookkeeping JSONL, not a transcript and do
        // not carry assistant usage.  Keep them out of both the node graph and
        // the session cursor while retaining the scanner's legacy collection
        // behavior for callers/tests that inspect raw paths.
        if path.file_name().and_then(|name| name.to_str()) == Some("journal.jsonl") {
            continue;
        }
        let identity = discover_claude_identity(&path, projects_dir)?;
        claims.push(identity.claim.clone());
        files.push((path, identity));
    }

    let normalized_nodes = normalize_session_relations(&claims)?;
    if normalized_nodes.len() != files.len() {
        return Err(AppError::InvalidInput(
            "Claude relation normalizer returned a mismatched batch".into(),
        ));
    }

    // Persist all nodes even when an unchanged cursor means there is no new
    // usage.  Historical node metadata must survive transcript deletion.
    for node in &normalized_nodes {
        write_agent_session_node(db, node)?;
    }

    for ((file_path, identity), node) in files.iter().zip(normalized_nodes.iter()) {
        result.files_scanned += 1;

        match sync_single_file_with_context(db, file_path, Some(identity), Some(node)) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[SESSION-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[SESSION-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// Discover a stable session ID from JSONL metadata, falling back to the file
/// stem only when no `sessionId` is present.  This is deliberately independent
/// of the incremental cursor: a later sync must not change identity because
/// the first metadata line was already consumed.
fn discover_claude_identity(
    path: &Path,
    projects_dir: &Path,
) -> Result<ClaudeFileIdentity, AppError> {
    let session_id = discover_session_id_from_jsonl(path)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .ok_or_else(|| AppError::InvalidInput("Claude transcript 缺少 session_id".into()))?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(AppError::InvalidInput(
            "Claude transcript 缺少 session_id".into(),
        ));
    }

    let relation = claude_relation_for_path(path, projects_dir);
    let (title, project_dir) = discover_claude_native_metadata(path);
    let metadata = SessionNodeMetadata {
        title,
        project_dir,
        source_path: Some(path.to_string_lossy().into_owned()),
        last_synced_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0),
        ..SessionNodeMetadata::default()
    };
    let claim = SessionRelationClaim {
        app_type: "claude".to_string(),
        session_id: session_id.clone(),
        relation,
        metadata,
    };
    Ok(ClaudeFileIdentity { session_id, claim })
}

fn discover_session_id_from_jsonl(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let Some(session_id) = value.get("sessionId").and_then(|value| value.as_str()) {
            let session_id = session_id.trim();
            if !session_id.is_empty() {
                return Some(session_id.to_string());
            }
        }
    }
    None
}

/// Read only Claude Code's native session metadata.  `custom-title` and `cwd`
/// are top-level transcript fields; message/prompt payloads are intentionally
/// never inspected, so a user's first message cannot become a task title.
fn discover_claude_native_metadata(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(file) = fs::File::open(path) else {
        return (None, None);
    };
    let mut title = None;
    let mut project_dir = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if project_dir.is_none() {
            project_dir = value
                .get("cwd")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
        }
        if value.get("type").and_then(serde_json::Value::as_str) == Some("custom-title") {
            if let Some(native_title) = value
                .get("customTitle")
                .or_else(|| value.get("custom_title"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                title = Some(native_title.to_owned());
            }
        }
    }
    (title, project_dir)
}

/// Return a structural claim only for the two documented Claude layouts.
/// Nearby files, title matches, and project/cwd similarity remain standalone.
fn claude_relation_for_path(path: &Path, projects_dir: &Path) -> RelationClaim {
    let relative = match path.strip_prefix(projects_dir) {
        Ok(relative) => relative,
        Err(_) => return RelationClaim::Standalone,
    };
    let components: Vec<String> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();

    match components.as_slice() {
        // projects/<project>/<session>.jsonl
        [_, _] => RelationClaim::Root,
        // projects/<project>/<SESSION_ID>/subagents/<child>.jsonl
        [_, parent, marker, _] if marker == "subagents" => RelationClaim::Parent {
            parent_session_id: parent.clone(),
            confidence: RelationConfidence::Structural,
        },
        // projects/<project>/<SESSION_ID>/subagents/workflows/wf_*/<child>.jsonl
        [_, parent, subagents, workflows, workflow_id, _]
            if subagents == "subagents"
                && workflows == "workflows"
                && workflow_id.starts_with("wf_") =>
        {
            RelationClaim::Parent {
                parent_session_id: parent.clone(),
                confidence: RelationConfidence::Structural,
            }
        }
        _ => RelationClaim::Standalone,
    }
}

/// 收集目录下所有 .jsonl 文件（含子 agent 文件）
///
/// 扫描固定深度，不使用递归，避免死循环：
///   projects_dir/项目目录/*.jsonl                                      (主会话)
///   projects_dir/项目目录/SESSION_ID/subagents/*.jsonl                  (Task/Agent 子 agent)
///   projects_dir/项目目录/SESSION_ID/subagents/workflows/wf_*/*.jsonl   (Workflow 子 agent)
///
/// 最后一层是 Claude Code Workflow 功能产生的子 agent transcript，比普通子
/// agent 多嵌套一层 `workflows/wf_<ID>/`。漏掉这一层会让 Workflow 的 token
/// 用量完全不计入统计；路径收集仍保留 `journal.jsonl` 供旧 scanner 测试，
/// 批次同步阶段会将其显式排除。
fn collect_jsonl_files(projects_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let entries = match fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 每个项目目录下的 .jsonl 文件
        if let Ok(sub_entries) = fs::read_dir(&path) {
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    // 主会话 JSONL 文件
                    files.push(sub_path);
                } else if sub_path.is_dir() {
                    // 扫描子 agent 目录: 项目/SESSION_ID/subagents/*.jsonl
                    let subagents_dir = sub_path.join("subagents");
                    if subagents_dir.is_dir() {
                        push_jsonl_children(&subagents_dir, &mut files);

                        // 额外下探 Workflow 子 agent:
                        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/*.jsonl
                        let workflows_dir = subagents_dir.join("workflows");
                        if workflows_dir.is_dir() {
                            if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
                                for wf_entry in wf_entries.flatten() {
                                    let wf_path = wf_entry.path();
                                    if wf_path.is_dir() {
                                        push_jsonl_children(&wf_path, &mut files);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    files
}

/// 将 `dir` 下直接子层的所有 `.jsonl` 文件追加到 `files`（不递归）。
fn push_jsonl_children(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
}

/// 同步单个 JSONL 文件，返回 (imported, skipped)
#[cfg(test)]
fn sync_single_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    // Keep the private legacy helper useful for focused tests and callers that
    // sync one file outside a projects batch: it still honors JSONL
    // `sessionId`, then filename fallback, while remaining self-only.
    let projects_dir = file_path.parent().unwrap_or_else(|| Path::new("."));
    let identity = discover_claude_identity(file_path, projects_dir).ok();
    let node = identity.as_ref().and_then(|value| {
        normalize_session_relations(std::slice::from_ref(&value.claim))
            .ok()
            .and_then(|nodes| nodes.into_iter().next())
    });
    if let Some(node) = node.as_ref() {
        write_agent_session_node(db, node)?;
    }
    sync_single_file_with_context(db, file_path, identity.as_ref(), node.as_ref())
}

/// Incrementally parse one file while retaining the normalized node identity
/// supplied by the batch pass.  A missing node is fail-closed: no raw row is
/// admitted because a canonical fact/coverage marker could not be proven.
fn sync_single_file_with_context(
    db: &Database,
    file_path: &Path,
    identity: Option<&ClaudeFileIdentity>,
    node: Option<&NormalizedSessionNode>,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态
    let (last_modified, last_offset) = get_sync_state(db, &file_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 从上次偏移位置开始增量解析
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);

    let mut line_offset: i64 = 0;
    let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
    // Keep the JSONL identity as a fallback when the incremental cursor starts
    // after the metadata line.  A sessionId found in the scanned rows still
    // wins over the fallback, matching the source's stable-ID contract.
    let fallback_session_id = identity.map(|value| value.session_id.clone());
    let mut current_session_id: Option<String> = None;

    for line_result in reader.lines() {
        line_offset += 1;

        // 跳过已处理的行
        if line_offset <= last_offset {
            continue;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // 容忍不完整的最后一行
        };

        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 提取 session ID (从 system 或首条消息)
        if current_session_id.is_none() {
            if let Some(sid) = value.get("sessionId").and_then(|v| v.as_str()) {
                current_session_id = Some(sid.to_string());
            }
        }

        // 只处理 assistant 类型的消息
        if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }

        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };

        let msg_id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let usage = match message.get("usage") {
            Some(u) => u,
            None => continue,
        };

        let parsed = ParsedAssistantUsage {
            message_id: msg_id.clone(),
            model: message
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            input_tokens: parse_token_component(usage, "input_tokens"),
            output_tokens: parse_token_component(usage, "output_tokens"),
            cache_read_tokens: parse_token_component(usage, "cache_read_input_tokens"),
            cache_creation_tokens: parse_token_component(usage, "cache_creation_input_tokens"),
            stop_reason: message
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            timestamp: value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            session_id: identity
                .map(|value| value.session_id.clone())
                .or_else(|| current_session_id.clone()),
        };

        // 按 message.id 去重：优先保留有 stop_reason 的条目，否则保留最新的
        let should_replace = match messages.get(&msg_id) {
            None => true,
            Some(existing) => {
                // 新条目有 stop_reason 而旧条目没有 → 替换
                if parsed.stop_reason.is_some() && existing.stop_reason.is_none() {
                    true
                }
                // 两个都有或都没有 stop_reason → 取 output_tokens 更大的
                else if parsed.stop_reason.is_some() == existing.stop_reason.is_some() {
                    parsed.output_tokens > existing.output_tokens
                } else {
                    false
                }
            }
        };

        if should_replace {
            messages.insert(msg_id, parsed);
        }
    }

    // 写入数据库
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    let mut rollups: HashMap<ClaudeRollupKey, NormalizedUsageRollupFact> = HashMap::new();
    let mut canonical_request_ids: HashMap<ClaudeRollupKey, Vec<String>> = HashMap::new();
    let mut proxy_request_ids: HashMap<ClaudeRollupKey, Vec<MatchingProxyUsageLog>> =
        HashMap::new();
    let mut claimed_proxy_request_ids = HashSet::new();
    let mut prepared_messages = Vec::new();
    for parsed in messages.values() {
        let mut msg = parsed.clone();
        if msg.session_id.is_none() {
            msg.session_id = current_session_id
                .clone()
                .or_else(|| fallback_session_id.clone());
        }
        // A canonical fact requires a stable session identity, a source event
        // timestamp, and at least one token component actually present in the
        // transcript.  Invalid/empty rows are intentionally consumed by the
        // cursor without producing a lossy raw zero or a sentinel timestamp.
        if msg
            .session_id
            .as_deref()
            .is_none_or(|session_id| session_id.trim().is_empty())
            || parsed_event_timestamp(&msg).is_none()
            || !has_source_token_component(&msg)
        {
            skipped += 1;
            continue;
        }

        let request_id = format!(
            "{}{}",
            crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
            msg.message_id
        );

        let (raw_exists, covered, matched_proxy, skip_raw) = {
            let conn = lock_conn!(db.conn);
            let raw_exists = claude_raw_request_exists(&conn, &request_id)?;
            let covered = Database::has_agent_session_canonical_coverage_on_conn(
                &conn,
                "claude",
                "session_log",
                &request_id,
            )?;
            let dedup_key = all_source_token_components_known(&msg).then(|| {
                let event_at = parsed_event_timestamp(&msg).expect("validated above");
                DedupKey {
                    app_type: "claude",
                    model: &msg.model,
                    input_tokens: token_value_for_raw(msg.input_tokens),
                    output_tokens: token_value_for_raw(msg.output_tokens),
                    cache_read_tokens: token_value_for_raw(msg.cache_read_tokens),
                    cache_creation_tokens: token_value_for_raw(msg.cache_creation_tokens),
                    created_at: event_at,
                }
            });
            // A takeover deliberately has no native `session_log` raw row.
            // Replay repair may suppress that row only when the *current*
            // message fingerprint/time maps to its proxy coverage marker;
            // another takeover in the same canonical session must not hide a
            // normal covered message's missing raw row.
            let skip_raw = if covered && !raw_exists {
                match dedup_key.as_ref() {
                    Some(key) => has_matching_proxy_usage_coverage_for_session(
                        &conn,
                        key,
                        msg.session_id.as_deref().unwrap_or_default(),
                    )?,
                    None => false,
                }
            } else {
                false
            };
            if !raw_exists && !covered {
                if let Some(dedup_key) = dedup_key {
                    let matched_proxy = match session_insert_outcome_excluding_claimed(
                        &conn,
                        &request_id,
                        &dedup_key,
                        &claimed_proxy_request_ids,
                    )? {
                        SessionInsertOutcome::Insert => None,
                        SessionInsertOutcome::ExistingRequest => {
                            skipped += 1;
                            continue;
                        }
                        SessionInsertOutcome::MatchedProxy(proxy) => {
                            // A single proxy request can be fingerprint-identical
                            // to multiple transcript rows in one batch.  The
                            // matcher already excludes earlier claims, so this
                            // insertion records the selected proxy row exactly once.
                            claimed_proxy_request_ids.insert(proxy.request_id.clone());
                            Some(proxy)
                        }
                    };
                    (raw_exists, covered, matched_proxy, skip_raw)
                } else {
                    (raw_exists, covered, None, skip_raw)
                }
            } else {
                (raw_exists, covered, None, skip_raw)
            }
        };

        // Existing raw rows are only repaired/covered when a durable fact is
        // missing.  A marker without a raw row can safely be repaired after a
        // prior interrupted insert, but no new raw row is admitted until its
        // fact is ready for the same request.
        let fact = if covered {
            None
        } else {
            node.and_then(|node| claude_rollup_for_message(db, node, &msg))
        };
        if !covered && fact.is_none() {
            skipped += 1;
            continue;
        }

        if let Some(fact) = fact.as_ref() {
            let rollup_key = claude_rollup_key(fact);
            merge_claude_rollup(&mut rollups, fact.clone());
            canonical_request_ids
                .entry(rollup_key)
                .or_default()
                .push(request_id.clone());
            if let Some(proxy) = matched_proxy.as_ref() {
                proxy_request_ids
                    .entry(claude_rollup_key(fact))
                    .or_default()
                    .push(proxy.clone());
            }
        }
        prepared_messages.push(PreparedClaudeMessage {
            request_id,
            message: msg,
            raw_exists,
            matched_proxy,
            skip_raw,
        });
    }

    if !prepared_messages.is_empty() {
        let (new_imported, new_skipped) = flush_claude_rollups(
            db,
            rollups,
            canonical_request_ids,
            proxy_request_ids,
            &prepared_messages,
        )?;
        imported = imported.saturating_add(new_imported);
        skipped = skipped.saturating_add(new_skipped);
    }

    // Advance the cursor only after the durable session buckets have been
    // written.  If the bucket write fails, the next sync can safely replay
    // the raw rows through their existing request-id deduplication boundary.
    update_sync_state(db, &file_path_str, file_modified, line_offset)?;

    Ok((imported, skipped))
}

fn claude_rollup_key(rollup: &NormalizedUsageRollupFact) -> ClaudeRollupKey {
    ClaudeRollupKey {
        date: rollup.date.clone(),
        session_id: rollup.session_id.clone(),
        provider_id: rollup.provider_id.clone(),
        model: rollup.model.clone(),
        request_model: rollup.request_model.clone(),
        pricing_model: rollup.pricing_model.clone(),
        data_source: rollup.data_source.clone(),
    }
}

fn parsed_event_timestamp(message: &ParsedAssistantUsage) -> Option<i64> {
    message.timestamp.as_deref().and_then(|timestamp| {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .ok()
            .map(|value| value.timestamp())
    })
}

fn local_date_for_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0).map(|value| {
        value
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// Build one precise assistant-message bucket.  Token components remain
/// nullable so a source omission is not confused with an explicit zero.
fn claude_rollup_for_message(
    db: &Database,
    node: &NormalizedSessionNode,
    message: &ParsedAssistantUsage,
) -> Option<NormalizedUsageRollupFact> {
    let event_at = parsed_event_timestamp(message)?;
    let session_id = node.session_id.trim();
    if session_id.is_empty()
        || message.session_id.as_deref().map(str::trim) != Some(session_id)
        || !has_source_token_component(message)
    {
        return None;
    }
    let date = local_date_for_timestamp(event_at)?;
    let conn = db.conn.lock().ok()?;
    let pricing = find_model_pricing_for_session(&conn, &message.model);
    let total_cost_usd = if all_source_token_components_known(message) {
        pricing.as_ref().map(|pricing| {
            let usage = TokenUsage {
                input_tokens: token_value_for_raw(message.input_tokens),
                output_tokens: token_value_for_raw(message.output_tokens),
                cache_read_tokens: token_value_for_raw(message.cache_read_tokens),
                cache_creation_tokens: token_value_for_raw(message.cache_creation_tokens),
                model: Some(message.model.clone()),
                message_id: None,
            };
            CostCalculator::calculate(&usage, pricing, Decimal::from(1))
                .total_cost
                .to_string()
        })
    } else {
        None
    };
    let pricing_model = pricing
        .as_ref()
        .map(|_| message.model.clone())
        .unwrap_or_default();

    Some(NormalizedUsageRollupFact {
        date,
        app_type: "claude".to_string(),
        session_id: session_id.to_string(),
        provider_id: "_session".to_string(),
        model: message.model.clone(),
        request_model: message.model.clone(),
        pricing_model,
        data_source: "session_log".to_string(),
        precision: UsagePrecision::RequestExact,
        time_semantics: TimeSemantics::EventTime,
        request_count_semantics: RequestCountSemantics::AssistantMessage,
        input_token_semantics: 0,
        source_identity: String::new(),
        profile_id: String::new(),
        database_identity: String::new(),
        base_url_digest: String::new(),
        billing_mode: String::new(),
        task: String::new(),
        source_version: String::new(),
        sync_window_start: 0,
        sync_window_end: 0,
        request_count: Some(1),
        api_call_count: None,
        input_tokens: message.input_tokens,
        output_tokens: message.output_tokens,
        cache_read_tokens: message.cache_read_tokens,
        cache_creation_tokens: message.cache_creation_tokens,
        cache_write_tokens: None,
        reasoning_tokens: None,
        total_cost_usd,
        cost_status: Some(if all_source_token_components_known(message) {
            if pricing.is_some() {
                "complete".to_string()
            } else {
                "unknown".to_string()
            }
        } else {
            "partial".to_string()
        }),
        cost_source: Some("session_log".to_string()),
        cost_delta_kind: None,
        correction_state: None,
        first_event_at: Some(event_at),
        last_event_at: Some(event_at),
    })
}

fn merge_token_component(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => left.checked_add(right),
        _ => None,
    }
}

fn add_cost_values(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => {
            Some((Decimal::from_str(left).ok()? + Decimal::from_str(right).ok()?).to_string())
        }
        _ => None,
    }
}

fn merge_claude_rollup(
    rollups: &mut HashMap<ClaudeRollupKey, NormalizedUsageRollupFact>,
    incoming: NormalizedUsageRollupFact,
) {
    let key = claude_rollup_key(&incoming);
    if let Some(existing) = rollups.get_mut(&key) {
        existing.request_count = match (existing.request_count, incoming.request_count) {
            (Some(left), Some(right)) => left.checked_add(right),
            _ => None,
        };
        existing.input_tokens = merge_token_component(existing.input_tokens, incoming.input_tokens);
        existing.output_tokens =
            merge_token_component(existing.output_tokens, incoming.output_tokens);
        existing.cache_read_tokens =
            merge_token_component(existing.cache_read_tokens, incoming.cache_read_tokens);
        existing.cache_creation_tokens = merge_token_component(
            existing.cache_creation_tokens,
            incoming.cache_creation_tokens,
        );
        existing.total_cost_usd = add_cost_values(
            existing.total_cost_usd.as_deref(),
            incoming.total_cost_usd.as_deref(),
        );
        existing.first_event_at = match (existing.first_event_at, incoming.first_event_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        existing.last_event_at = match (existing.last_event_at, incoming.last_event_at) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
    } else {
        rollups.insert(key, incoming);
    }
}

/// Merge a newly imported batch with an existing durable bucket before using
/// the canonical write bridge.  The DAO operation replaces a full key, so
/// writing one message at a time would otherwise erase earlier increments.
fn flush_claude_rollups(
    db: &Database,
    rollups: HashMap<ClaudeRollupKey, NormalizedUsageRollupFact>,
    canonical_request_ids: HashMap<ClaudeRollupKey, Vec<String>>,
    proxy_request_ids: HashMap<ClaudeRollupKey, Vec<MatchingProxyUsageLog>>,
    prepared_messages: &[PreparedClaudeMessage],
) -> Result<(u32, u32), AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 Claude canonical 覆盖事务失败: {error}"))
    })?;
    let marked_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    for (key, rollup) in rollups {
        let existing = match tx.query_row(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd, first_event_at, last_event_at
             FROM agent_session_usage_rollups
             WHERE date = ?1 AND app_type = ?2 AND session_id = ?3
               AND provider_id = ?4 AND model = ?5 AND request_model = ?6
               AND pricing_model = ?7 AND data_source = ?8
               AND precision = ?9 AND time_semantics = ?10
               AND request_count_semantics = ?11
               AND input_token_semantics = ?12
               AND source_identity = ?13 AND profile_id = ?14
               AND database_identity = ?15 AND base_url_digest = ?16
               AND billing_mode = ?17 AND task = ?18 AND source_version = ?19
               AND sync_window_start = ?20 AND sync_window_end = ?21",
            rusqlite::params![
                &rollup.date,
                &rollup.app_type,
                &rollup.session_id,
                &rollup.provider_id,
                &rollup.model,
                &rollup.request_model,
                &rollup.pricing_model,
                &rollup.data_source,
                rollup.precision.as_str(),
                rollup.time_semantics.as_str(),
                rollup.request_count_semantics.as_str(),
                rollup.input_token_semantics,
                &rollup.source_identity,
                &rollup.profile_id,
                &rollup.database_identity,
                &rollup.base_url_digest,
                &rollup.billing_mode,
                &rollup.task,
                &rollup.source_version,
                rollup.sync_window_start,
                rollup.sync_window_end,
            ],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            },
        ) {
            Ok(value) => Some(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(AppError::Database(error.to_string())),
        };

        let mut merged = rollup;
        if let Some((
            old_count,
            old_input,
            old_output,
            old_cache_read,
            old_cache_creation,
            old_cost,
            old_first,
            old_last,
        )) = existing
        {
            merged.request_count = match (old_count, merged.request_count) {
                (Some(left), Some(right)) => left.checked_add(right),
                _ => None,
            };
            merged.input_tokens = merge_token_component(old_input, merged.input_tokens);
            merged.output_tokens = merge_token_component(old_output, merged.output_tokens);
            merged.cache_read_tokens =
                merge_token_component(old_cache_read, merged.cache_read_tokens);
            merged.cache_creation_tokens =
                merge_token_component(old_cache_creation, merged.cache_creation_tokens);
            merged.total_cost_usd =
                add_cost_values(old_cost.as_deref(), merged.total_cost_usd.as_deref());
            merged.first_event_at = match (old_first, merged.first_event_at) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
            merged.last_event_at = match (old_last, merged.last_event_at) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (left, right) => left.or(right),
            };
        }
        write_agent_session_usage_rollup_fact_on_conn(&tx, &merged)?;
        if let Some(request_ids) = canonical_request_ids.get(&key) {
            for request_id in request_ids {
                let marker = AgentSessionCanonicalCoverageMarker {
                    app_type: merged.app_type.clone(),
                    data_source: merged.data_source.clone(),
                    request_id: request_id.clone(),
                    canonical_session_id: Some(merged.session_id.clone()),
                    marked_at,
                };
                Database::upsert_agent_session_canonical_coverage_on_conn(&tx, &marker)?;
            }
        }
        if let Some(proxy_matches) = proxy_request_ids.get(&key) {
            for proxy in proxy_matches {
                // The proxy row is retained as the global request record, but
                // this marker makes the native transcript's canonical bucket
                // the sole session/task owner.  Use the row's stored app type
                // (Claude Desktop may be `claude-desktop`) so the marker key
                // matches the raw row exactly.
                let marker = AgentSessionCanonicalCoverageMarker {
                    app_type: proxy.app_type.clone(),
                    data_source: "proxy".to_string(),
                    request_id: proxy.request_id.clone(),
                    canonical_session_id: Some(merged.session_id.clone()),
                    marked_at,
                };
                Database::upsert_agent_session_canonical_coverage_on_conn(&tx, &marker)?;
            }
        }
    }

    // Raw compatibility rows, canonical facts, and both source coverage
    // markers share this transaction.  A matched proxy event intentionally
    // skips its duplicate native raw row while still publishing the fact and
    // markers above.  Existing source rows remain idempotent no-ops.
    let mut imported = 0u32;
    let mut skipped = 0u32;
    for prepared in prepared_messages {
        if prepared.raw_exists || prepared.matched_proxy.is_some() || prepared.skip_raw {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match insert_session_log_entry_on_conn(&tx, &prepared.request_id, &prepared.message, false)
        {
            Ok(true) => imported = imported.saturating_add(1),
            Ok(false) => skipped = skipped.saturating_add(1),
            Err(error) => {
                return Err(AppError::Database(format!(
                    "插入 Claude 会话日志失败 ({}): {error}",
                    prepared.message.message_id
                )));
            }
        }
    }
    tx.commit().map_err(|error| {
        AppError::Database(format!("提交 Claude canonical 覆盖事务失败: {error}"))
    })?;
    Ok((imported, skipped))
}

/// 获取 session_log_sync 表中某条目的同步进度。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn get_sync_state(db: &Database, file_path: &str) -> Result<(i64, i64), AppError> {
    let conn = lock_conn!(db.conn);
    let result = conn.query_row(
        "SELECT last_modified, last_line_offset FROM session_log_sync WHERE file_path = ?1",
        rusqlite::params![file_path],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    );
    Ok(result.unwrap_or((0, 0)))
}

/// 返回文件 mtime 的纳秒时间戳。
///
/// `session_log_sync.last_modified` 旧数据是秒级时间戳；新写入纳秒值不需要
/// schema 迁移，旧值会自然触发一次增量重扫，并继续依赖行 offset 避免重复导入。
pub(crate) fn metadata_modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// 更新 session_log_sync 表中某条目的同步进度。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn update_sync_state(
    db: &Database,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    update_sync_state_on_conn(&conn, file_path, last_modified, last_offset)
}

/// [`update_sync_state`] 的免锁版本，供调用方在已持锁的事务内把游标推进
/// 与数据插入绑成原子提交。
pub(crate) fn update_sync_state_on_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .and_then(|mut stmt| stmt.execute(rusqlite::params![file_path, last_modified, last_offset, now]))
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

fn claude_raw_request_exists(
    conn: &rusqlite::Connection,
    request_id: &str,
) -> Result<bool, AppError> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)",
            rusqlite::params![request_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(format!("读取 Claude raw request 失败: {error}")))?;
    Ok(exists != 0)
}

/// 插入单条会话日志到 proxy_request_logs，返回是否成功插入 (true=新插入, false=已存在)
#[cfg(test)]
fn insert_session_log_entry(
    db: &Database,
    request_id: &str,
    msg: &ParsedAssistantUsage,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    insert_session_log_entry_on_conn(&conn, request_id, msg, true)
}

/// Connection-scoped Claude raw writer used by the canonical transaction.
/// `check_dedup` remains enabled for the standalone compatibility helper but
/// is disabled after the batch arbitration has already reserved a message's
/// outcome under the same transaction.
fn insert_session_log_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    check_dedup: bool,
) -> Result<bool, AppError> {
    let created_at = parsed_event_timestamp(msg).ok_or_else(|| {
        AppError::InvalidInput("Claude session log 缺少 RFC3339 event timestamp".into())
    })?;
    let session_id = msg
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Claude session log 缺少 session_id".into()))?;

    let dedup_key = DedupKey {
        app_type: "claude",
        model: &msg.model,
        input_tokens: token_value_for_raw(msg.input_tokens),
        output_tokens: token_value_for_raw(msg.output_tokens),
        cache_read_tokens: token_value_for_raw(msg.cache_read_tokens),
        cache_creation_tokens: token_value_for_raw(msg.cache_creation_tokens),
        created_at,
    };
    if check_dedup
        && all_source_token_components_known(msg)
        && !matches!(
            session_insert_outcome(conn, request_id, &dedup_key)?,
            SessionInsertOutcome::Insert
        )
    {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: token_value_for_raw(msg.input_tokens),
        output_tokens: token_value_for_raw(msg.output_tokens),
        cache_read_tokens: token_value_for_raw(msg.cache_read_tokens),
        cache_creation_tokens: token_value_for_raw(msg.cache_creation_tokens),
        model: Some(msg.model.clone()),
        message_id: None,
    };

    let pricing = all_source_token_components_known(msg)
        .then(|| find_model_pricing_for_session(conn, &msg.model))
        .flatten();
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
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
                request_id,
                "_session",         // provider_id: 标记为会话来源
                "claude",           // app_type
                msg.model,
                msg.model,          // request_model = model
                token_value_for_raw(msg.input_tokens),
                token_value_for_raw(msg.output_tokens),
                token_value_for_raw(msg.cache_read_tokens),
                token_value_for_raw(msg.cache_creation_tokens),
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,               // latency_ms: 会话日志无此数据
                Option::<i64>::None, // first_token_ms
                200i64,             // status_code: 会话日志中的请求只要产生计费 token 即视为成功
                Option::<String>::None, // error_message
                session_id,
                Some("session_log"), // provider_type
                1i64,               // is_streaming: Claude Code 通常使用流式
                "1.0",              // cost_multiplier
                created_at,
                "session_log",      // data_source
            ],
        )
        .map_err(|e| AppError::Database(format!("插入会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

/// 从 model_pricing 表查找模型定价（支持模糊匹配）
fn find_model_pricing_for_session(
    conn: &rusqlite::Connection,
    model_id: &str,
) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

/// 查询数据来源分布统计
pub fn get_data_source_breakdown(db: &Database) -> Result<Vec<DataSourceSummary>, AppError> {
    let conn = lock_conn!(db.conn);

    let effective_filter = effective_usage_log_filter("l");
    let sql = format!(
        "SELECT COALESCE(l.data_source, 'proxy') as ds, COUNT(*) as cnt,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as cost
         FROM proxy_request_logs l
         WHERE {effective_filter}
         GROUP BY ds
         ORDER BY cnt DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map([], |row| {
        Ok(DataSourceSummary {
            data_source: row.get(0)?,
            request_count: row.get::<_, i64>(1)? as u32,
            total_cost_usd: format!("{:.6}", row.get::<_, f64>(2)?),
        })
    })?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::usage_stats::effective_session_usage_log_filter;
    use std::io::Write;

    #[test]
    fn sync_result_notification_is_coalesced_to_one_call() {
        crate::usage_events::take_test_notify_count();
        notify_sync_result(&SessionSyncResult::default());
        let result = SessionSyncResult {
            imported: 25,
            ..SessionSyncResult::default()
        };
        notify_sync_result(&result);
        assert_eq!(crate::usage_events::take_test_notify_count(), 1);
    }

    #[test]
    fn sync_step_failure_is_recorded_without_blocking_following_source() {
        let mut aggregate = SessionSyncResult::default();
        merge_sync_step(
            &mut aggregate,
            "fixture-failed-source",
            Err(AppError::Config("fixture failure".into())),
        );
        merge_sync_step(
            &mut aggregate,
            "fixture-success-source",
            Ok(SessionSyncResult {
                imported: 2,
                files_scanned: 1,
                ..SessionSyncResult::default()
            }),
        );

        assert_eq!(aggregate.imported, 2);
        assert_eq!(aggregate.files_scanned, 1);
        assert_eq!(aggregate.errors.len(), 1);
        assert!(aggregate.errors[0].contains("fixture-failed-source"));
    }

    #[test]
    fn claude_native_title_and_cwd_are_read_without_message_fallback() {
        let temp = tempfile::tempdir().expect("create Claude fixture");
        let projects = temp.path().join("projects");
        let project_dir = projects.join("fixture");
        fs::create_dir_all(&project_dir).expect("create Claude project");
        let path = project_dir.join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session\",\"cwd\":\"/workspace/native\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Native title\"}}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"Native title\"}\n",
            ),
        )
        .expect("write Claude fixture");

        let identity = discover_claude_identity(&path, &projects).expect("discover identity");
        assert_eq!(identity.session_id, "session");
        assert_eq!(
            identity.claim.metadata.title.as_deref(),
            Some("Native title")
        );
        assert_eq!(
            identity.claim.metadata.project_dir.as_deref(),
            Some("/workspace/native")
        );
    }

    #[test]
    fn hermes_sync_result_preserves_adapter_success_accounting() {
        let delta = crate::services::session_usage_hermes::HermesUsageDelta {
            source_identity: "hermes:fixture".into(),
            profile_id: "default".into(),
            database_identity: "db".into(),
            session_id: "session".into(),
            canonical_session_id: "hermes:default:db:session".into(),
            model: "fixture-model".into(),
            provider_id: "fixture-provider".into(),
            base_url_digest: "sha256:fixture".into(),
            billing_mode: "actual".into(),
            task: "fixture-task".into(),
            data_source: "hermes_session_model_usage".into(),
            source_version: "session_model_usage:v1".into(),
            precision: "sync_window_delta",
            time_semantics: "sync_window_end",
            request_count_semantics: "unavailable",
            input_token_semantics: 0,
            sync_window_start: 100,
            sync_window_end: 200,
            api_call_count: 1,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            reasoning_tokens: 0,
            cost_usd: None,
            cost_kind: crate::services::session_usage_hermes::CostKind::Unknown,
            cost_delta_kind: crate::services::session_usage_hermes::CostDeltaKind::None,
            cost_status: None,
            cost_source: None,
            first_seen_ms: Some(100),
            last_seen_ms: Some(200),
        };
        let detailed = crate::services::session_usage_hermes::HermesSyncResult {
            profiles_scanned: 3,
            baselined: 2,
            imported: 4,
            skipped: 1,
            errors: vec!["fixture profile warning".into()],
            deltas: vec![delta],
            ..Default::default()
        };

        let result = detailed.as_session_sync_result();
        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped, 3, "baseline rows remain adapter skips");
        assert_eq!(result.files_scanned, 3);
        assert_eq!(result.errors, vec!["fixture profile warning".to_string()]);
    }

    #[test]
    fn cowork_proxy_arbitration_counts_each_event_once_and_keeps_cache_differences(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES ('cowork-proxy', 'gateway', 'claude-desktop', 'fixture-model',
                          'fixture-model', 10, 2, 1, 0, '0.01', 0, 200, 1000, 'proxy')",
                [],
            )?;
        }

        let records = vec![
            crate::services::session_usage_cowork::CoworkSourceIdentity {
                source_id: "cowork:session-a:message-proxy".into(),
                app_type: "claude-desktop".into(),
                data_source: "cowork_session".into(),
                session_id: "session-a".into(),
                message_id: "message-proxy".into(),
                model: "fixture-model".into(),
                event_at: 1000,
                input_tokens: 10,
                output_tokens: 2,
                cache_read_tokens: 1,
                cache_creation_tokens: 0,
                source_path: "fixture/root.jsonl".into(),
            },
            crate::services::session_usage_cowork::CoworkSourceIdentity {
                source_id: "cowork:session-a:message-local".into(),
                app_type: "claude-desktop".into(),
                data_source: "cowork_session".into(),
                session_id: "session-a".into(),
                message_id: "message-local".into(),
                model: "fixture-model".into(),
                event_at: 1060,
                input_tokens: 10,
                output_tokens: 2,
                cache_read_tokens: 2,
                cache_creation_tokens: 0,
                source_path: "fixture/root.jsonl".into(),
            },
        ];

        assert_eq!(arbitrate_cowork_proxy_rows(&db, &records)?, 1);
        let conn = lock_conn!(db.conn);
        let row: (String, i64, i64, i64) = conn.query_row(
            "SELECT app_type, request_count, input_tokens, cache_read_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude-desktop' AND session_id = 'session-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, ("claude-desktop".into(), 1, 10, 2));
        Ok(())
    }

    #[tokio::test]
    async fn session_sync_mutex_serializes_callers() {
        let first = session_sync_mutex().lock().await;
        assert!(session_sync_mutex().try_lock().is_err());
        drop(first);
        assert!(session_sync_mutex().try_lock().is_ok());
    }

    #[test]
    fn test_parse_usage_from_jsonl_line() {
        let line = r#"{"type":"assistant","message":{"id":"msg_test123","model":"claude-opus-4-6","usage":{"input_tokens":3,"output_tokens":150,"cache_read_input_tokens":5000,"cache_creation_input_tokens":10000},"stop_reason":"end_turn"},"timestamp":"2026-04-05T12:00:00Z","sessionId":"session-abc"}"#;

        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            value.get("type").and_then(|t| t.as_str()),
            Some("assistant")
        );

        let message = value.get("message").unwrap();
        let usage = message.get("usage").unwrap();

        assert_eq!(usage.get("input_tokens").unwrap().as_u64().unwrap(), 3);
        assert_eq!(usage.get("output_tokens").unwrap().as_u64().unwrap(), 150);
        assert_eq!(
            usage
                .get("cache_read_input_tokens")
                .unwrap()
                .as_u64()
                .unwrap(),
            5000
        );
        assert_eq!(
            usage
                .get("cache_creation_input_tokens")
                .unwrap()
                .as_u64()
                .unwrap(),
            10000
        );
        assert_eq!(
            message.get("stop_reason").unwrap().as_str().unwrap(),
            "end_turn"
        );
    }

    #[test]
    fn test_dedup_by_message_id() {
        // 同一个 message.id 有多条，应该取 stop_reason 有值的那条
        let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();

        // 中间条目（无 stop_reason）
        let intermediate = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: Some(3),
            output_tokens: Some(26),
            cache_read_tokens: Some(5000),
            cache_creation_tokens: Some(10000),
            stop_reason: None,
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };
        messages.insert("msg_1".to_string(), intermediate);

        // 最终条目（有 stop_reason）
        let final_entry = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: Some(3),
            output_tokens: Some(1349),
            cache_read_tokens: Some(5000),
            cache_creation_tokens: Some(10000),
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };

        // 应该替换
        let should_replace = final_entry.stop_reason.is_some()
            && messages.get("msg_1").unwrap().stop_reason.is_none();
        assert!(should_replace);

        messages.insert("msg_1".to_string(), final_entry);
        assert_eq!(messages.get("msg_1").unwrap().output_tokens, Some(1349));
    }

    #[test]
    fn test_insert_claude_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "proxy-different-id",
                    "openai-compatible",
                    "claude",
                    "claude-sonnet-4-5",
                    "claude-sonnet-4-5",
                    100,
                    20,
                    10,
                    5,
                    "0.10",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let msg = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            input_tokens: Some(100),
            output_tokens: Some(20),
            cache_read_tokens: Some(10),
            cache_creation_tokens: Some(5),
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("1970-01-01T00:16:45Z".to_string()),
            session_id: Some("session-1".to_string()),
        };

        let inserted = insert_session_log_entry(&db, "session:msg_1", &msg)?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn claude_proxy_takeover_writes_canonical_and_both_coverage_markers() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
                ) VALUES ('proxy-null-session', 'gateway', 'claude', 'fixture-unknown',
                          'fixture-unknown', 100, 20, 10, 5, '0.10', 0, 200, NULL, 1000, 'proxy')",
                [],
            )?;
        }

        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-proxy-takeover-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("takeover.jsonl");
        let line = r#"{"type":"assistant","sessionId":"takeover-session","timestamp":"1970-01-01T00:16:45Z","message":{"id":"takeover-msg","model":"fixture-unknown","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":10,"cache_creation_input_tokens":5},"stop_reason":"end_turn"}}"#;
        fs::write(&path, format!("{line}\n")).unwrap();

        let first = sync_claude_files(&db, &projects)?;
        assert_eq!(
            first.imported, 0,
            "the duplicate native raw row is suppressed"
        );

        let conn = lock_conn!(db.conn);
        let raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            raw_count, 1,
            "the original proxy raw row remains the only raw row"
        );
        let native_raw_exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:takeover-msg'
             )",
            [],
            |row| row.get(0),
        )?;
        assert!(!native_raw_exists);

        let rollup: (i64, i64, i64, i64) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'takeover-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(rollup, (1, 100, 20, 10));

        let session_marker: (String, Option<String>) = conn.query_row(
            "SELECT data_source, canonical_session_id
             FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND request_id = 'session:takeover-msg'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            session_marker,
            ("session_log".into(), Some("takeover-session".into()))
        );
        let proxy_marker: (String, Option<String>) = conn.query_row(
            "SELECT data_source, canonical_session_id
             FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND request_id = 'proxy-null-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            proxy_marker,
            ("proxy".into(), Some("takeover-session".into()))
        );

        // Global raw usage remains exactly one request, while the task/session
        // query owns the canonical transcript bucket.
        let effective_filter = effective_usage_log_filter("l");
        let visible_global: (i64, i64) = conn.query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0)
                 FROM proxy_request_logs l
                 WHERE {effective_filter} AND l.app_type = 'claude'"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(visible_global, (1, 100));

        drop(conn);
        // Force a cursor replay with the same source message.  The takeover's
        // proxy marker must suppress raw repair and preserve one canonical
        // request, even though the transcript line is seen again.
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
        let second = sync_claude_files(&db, &projects)?;
        assert_eq!(second.imported, 0);
        let conn = lock_conn!(db.conn);
        let repeat_rollup_count: i64 = conn.query_row(
            "SELECT request_count FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'takeover-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(repeat_rollup_count, 1);
        let repeat_raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(repeat_raw_count, 1);

        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_batch_claims_distinct_matching_proxy_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for request_id in ["proxy-batch-a", "proxy-batch-b"] {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
                    ) VALUES (?1, 'gateway', 'claude', 'fixture-unknown',
                              'fixture-unknown', 100, 20, 10, 5, '0.10', 0, 200, NULL, 1000, 'proxy')",
                    [request_id],
                )?;
            }
        }

        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-proxy-batch-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("batch.jsonl");
        let message = |message_id: &str| {
            format!(
                r#"{{"type":"assistant","sessionId":"batch-session","timestamp":"1970-01-01T00:16:45Z","message":{{"id":"{message_id}","model":"fixture-unknown","usage":{{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}},"stop_reason":"end_turn"}}}}"#
            )
        };
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                message("batch-message-a"),
                message("batch-message-b")
            ),
        )
        .unwrap();

        let result = sync_claude_files(&db, &projects)?;
        assert_eq!(result.imported, 0, "both native rows take over proxy rows");

        let conn = lock_conn!(db.conn);
        let raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(raw_count, 2);
        let rollup_count: i64 = conn.query_row(
            "SELECT request_count FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'batch-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rollup_count, 2, "each native message contributes once");
        let covered_proxy_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'proxy'
               AND canonical_session_id = 'batch-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(covered_proxy_count, 2, "both proxy rows are claimed once");

        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_batch_admits_distinct_native_event_after_proxy_claim() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
                ) VALUES ('proxy-single-batch', 'gateway', 'claude', 'fixture-unknown',
                          'fixture-unknown', 100, 20, 10, 5, '0.10', 0, 200, NULL, 1000, 'proxy')",
                [],
            )?;
        }

        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-proxy-single-batch-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("single-batch.jsonl");
        let message = |message_id: &str| {
            format!(
                r#"{{"type":"assistant","sessionId":"single-batch-session","timestamp":"1970-01-01T00:16:45Z","message":{{"id":"{message_id}","model":"fixture-unknown","usage":{{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}},"stop_reason":"end_turn"}}}}"#
            )
        };
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                message("single-message-a"),
                message("single-message-b")
            ),
        )
        .unwrap();

        let result = sync_claude_files(&db, &projects)?;
        assert_eq!(result.imported, 1, "the second native event owns a raw row");
        assert_eq!(
            result.skipped, 1,
            "the first native event takes over the proxy"
        );

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'claude'),
                (SELECT request_count FROM agent_session_usage_rollups
                 WHERE app_type = 'claude' AND session_id = 'single-batch-session'),
                (SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND data_source = 'proxy'
                   AND canonical_session_id = 'single-batch-session'),
                (SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND data_source = 'session_log'
                   AND canonical_session_id = 'single-batch-session')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        // The two raw rows represent two distinct transcript events: one
        // proxy takeover plus one native-owned event.  Only the takeover gets
        // a proxy coverage marker; both canonical facts are session-owned.
        assert_eq!(counts, (2, 2, 1, 2));

        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_proxy_coverage_repair_is_specific_to_current_message() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
                ) VALUES ('proxy-a', 'gateway', 'claude', 'fixture-unknown',
                          'fixture-unknown', 100, 20, 10, 5, '0.10', 0, 200, NULL, 1000, 'proxy')",
                [],
            )?;
        }

        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-proxy-specific-repair-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("mixed.jsonl");
        let takeover = r#"{"type":"assistant","sessionId":"shared-session","timestamp":"1970-01-01T00:16:45Z","message":{"id":"message-a","model":"fixture-unknown","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":10,"cache_creation_input_tokens":5},"stop_reason":"end_turn"}}"#;
        let normal = r#"{"type":"assistant","sessionId":"shared-session","timestamp":"1970-01-01T00:16:50Z","message":{"id":"message-b","model":"fixture-unknown","usage":{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":11,"cache_creation_input_tokens":6},"stop_reason":"end_turn"}}"#;
        fs::write(&path, format!("{takeover}\n{normal}\n")).unwrap();

        let first = sync_claude_files(&db, &projects)?;
        assert_eq!(
            first.imported, 1,
            "only the unmatched normal message writes raw"
        );
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "DELETE FROM proxy_request_logs WHERE request_id = 'session:message-b'",
                [],
            )?;
            let b_coverage: bool = conn.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM agent_session_canonical_coverage
                    WHERE app_type = 'claude' AND data_source = 'session_log'
                      AND request_id = 'session:message-b'
                 )",
                [],
                |row| row.get(0),
            )?;
            assert!(b_coverage, "B keeps its canonical coverage after raw loss");
        }

        // Replay only B.  A's proxy coverage belongs to the same session but
        // has a different fingerprint; B's missing raw row must be repaired.
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(format!("{normal}\n").as_bytes())
            .unwrap();
        let second = sync_claude_files(&db, &projects)?;
        assert_eq!(second.imported, 1, "normal B raw row is repaired");

        let conn = lock_conn!(db.conn);
        let b_raw: (i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = 'session:message-b'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(b_raw, (200, 30));
        let proxy_coverage: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'proxy'
               AND request_id = 'proxy-a' AND canonical_session_id = 'shared-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(proxy_coverage, 1);

        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_includes_subagents() {
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-abc.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        assert_eq!(files.len(), 2);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-abc.jsonl")));

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_collect_jsonl_files_includes_workflow_subagents() {
        // Claude Code Workflow 把子 agent transcript 嵌在
        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/ 下，比普通子 agent 深一层。
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        let wf_dir = subagents_dir.join("workflows").join("wf_test123");
        fs::create_dir_all(&wf_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-plain.jsonl"), "{}").unwrap();
        fs::write(wf_dir.join("agent-wf.jsonl"), "{}").unwrap();
        // journal.jsonl 也会被收集，但解析时因无 assistant 行而产出 0 条
        fs::write(wf_dir.join("journal.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // 主会话 + 普通子 agent + Workflow 子 agent(agent-wf + journal) = 4
        assert_eq!(files.len(), 4);
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-plain.jsonl")));
        assert!(
            paths.iter().any(|p| p.contains("agent-wf.jsonl")),
            "Workflow 子 agent transcript 必须被收集"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_sync_imports_billable_message_without_stop_reason() -> Result<(), AppError> {
        // 回归：stop_reason 缺失但有真实 cache/input 成本的 message（Workflow /
        // 子 agent 常见的「只有 message_start 快照、没写最终块」形态）必须被计入，
        // 不能因缺 stop_reason 或 output==0 而整条丢弃；全 0 token 的占位行仍应跳过。
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("agent-wf.jsonl");

        // 第一行：无 stop_reason、output=1，但 cache_read/cache_creation 很大 → 应导入
        // 第二行：明确报告的全 0 token → 仍应作为已知零 usage 保留
        let billable = r#"{"type":"assistant","message":{"id":"msg_nostop","model":"claude-opus-4-8","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":48719,"cache_creation_input_tokens":2061}},"timestamp":"2026-06-07T13:01:23Z","sessionId":"session-wf"}"#;
        let empty = r#"{"type":"assistant","message":{"id":"msg_empty","model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-06-07T13:01:24Z","sessionId":"session-wf"}"#;
        fs::write(&file, format!("{billable}\n{empty}\n")).unwrap();

        let (imported, _skipped) = sync_single_file(&db, &file)?;
        assert_eq!(
            imported, 2,
            "有 cache 成本但无 stop_reason 的 message 与明确零 usage 都必须被导入"
        );

        let conn = lock_conn!(db.conn);
        let cache_read: i64 = conn.query_row(
            "SELECT cache_read_tokens FROM proxy_request_logs WHERE request_id = 'session:msg_nostop'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cache_read, 48719, "cache_read 必须被完整记录");
        let empty_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:msg_empty')",
            [],
            |row| row.get(0),
        )?;
        assert!(empty_exists, "来源明确的全 0 token message 必须被保留");
        let empty_tokens: (i64, i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
             FROM proxy_request_logs WHERE request_id = 'session:msg_empty'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(empty_tokens, (0, 0, 0, 0));
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    fn write_claude_fixture(
        path: &Path,
        session_id: &str,
        message_id: &str,
        usage: &str,
        timestamp: Option<&str>,
        stop_reason: Option<&str>,
    ) {
        let timestamp = timestamp
            .map(|value| format!(",\"timestamp\":\"{value}\""))
            .unwrap_or_default();
        let stop_reason = stop_reason
            .map(|value| format!(",\"stop_reason\":\"{value}\""))
            .unwrap_or_default();
        let line = format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\"{timestamp},\"message\":{{\"id\":\"{message_id}\",\"model\":\"fixture-unknown\",\"usage\":{usage}{stop_reason}}}}}"
        );
        fs::write(path, format!("{line}\n")).unwrap();
    }

    #[test]
    fn claude_batch_keeps_root_self_separate_from_structural_children() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-hierarchy-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        let root_dir = project.join("root-session");
        let subagents = root_dir.join("subagents");
        let workflows = subagents.join("workflows").join("wf_1");
        fs::create_dir_all(&workflows).unwrap();

        write_claude_fixture(
            &project.join("root-session.jsonl"),
            "root-session",
            "root-msg",
            "{\"input_tokens\":10,\"output_tokens\":2,\"cache_read_input_tokens\":1,\"cache_creation_input_tokens\":0}",
            Some("2026-08-13T10:00:01Z"),
            Some("end_turn"),
        );
        write_claude_fixture(
            &subagents.join("agent-child.jsonl"),
            "child-session",
            "child-msg",
            "{\"input_tokens\":20,\"output_tokens\":3,\"cache_read_input_tokens\":2,\"cache_creation_input_tokens\":0}",
            Some("2026-08-13T10:00:02Z"),
            Some("end_turn"),
        );
        write_claude_fixture(
            &workflows.join("agent-workflow.jsonl"),
            "workflow-session",
            "workflow-msg",
            "{\"input_tokens\":30,\"output_tokens\":4,\"cache_read_input_tokens\":3,\"cache_creation_input_tokens\":0}",
            Some("2026-08-13T10:00:03Z"),
            Some("end_turn"),
        );
        write_claude_fixture(
            &project.join("nearby.jsonl"),
            "nearby-session",
            "nearby-msg",
            "{\"input_tokens\":40,\"output_tokens\":5,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":0}",
            Some("2026-08-13T10:00:04Z"),
            Some("end_turn"),
        );

        let first = sync_claude_files(&db, &projects)?;
        assert_eq!(first.imported, 4);

        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT session_id, node_kind, parent_session_id, root_session_id, relation_confidence
             FROM agent_session_nodes WHERE app_type = 'claude' ORDER BY session_id",
        )?;
        let nodes: Vec<(String, String, Option<String>, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        assert_eq!(nodes.len(), 4);
        let root = nodes.iter().find(|row| row.0 == "root-session").unwrap();
        assert_eq!(
            (root.1.as_str(), root.2.as_deref(), root.3.as_str()),
            ("root", None, "root-session")
        );
        for child_id in ["child-session", "workflow-session"] {
            let child = nodes.iter().find(|row| row.0 == child_id).unwrap();
            assert_eq!(child.1, "child");
            assert_eq!(child.2.as_deref(), Some("root-session"));
            assert_eq!(child.3, "root-session");
            assert_eq!(child.4, "structural");
        }
        // A top-level Claude transcript is an explicit root/self source
        // location; it still has no parent relation to the adjacent session.
        let nearby = nodes.iter().find(|row| row.0 == "nearby-session").unwrap();
        assert_eq!(
            (nearby.1.as_str(), nearby.2.as_deref(), nearby.3.as_str()),
            ("root", None, "nearby-session")
        );

        drop(stmt);
        let mut stmt = conn.prepare(
            "SELECT session_id, request_count, input_tokens, output_tokens, cache_read_tokens
             FROM agent_session_usage_rollups WHERE app_type = 'claude' ORDER BY session_id",
        )?;
        let rollups: Vec<(String, i64, i64, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        assert_eq!(rollups.len(), 4);
        let root_rollup = rollups.iter().find(|row| row.0 == "root-session").unwrap();
        assert_eq!(
            (root_rollup.1, root_rollup.2, root_rollup.3, root_rollup.4),
            (1, 10, 2, 1)
        );
        assert_eq!(
            rollups
                .iter()
                .find(|row| row.0 == "child-session")
                .unwrap()
                .2,
            20
        );
        assert_eq!(
            rollups
                .iter()
                .find(|row| row.0 == "workflow-session")
                .unwrap()
                .2,
            30
        );
        assert_eq!(
            rollups
                .iter()
                .find(|row| row.0 == "nearby-session")
                .unwrap()
                .2,
            40
        );
        let covered_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'session_log'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            covered_count, 4,
            "each direct Claude raw request has a marker"
        );
        drop(stmt);
        drop(conn);

        let second = sync_claude_files(&db, &projects)?;
        assert_eq!(
            second.imported, 0,
            "cursor rerun must not import children again"
        );
        let conn = lock_conn!(db.conn);
        let raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        let bucket_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        let second_marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'session_log'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(raw_count, 4);
        assert_eq!(bucket_count, 4);
        assert_eq!(second_marker_count, 4);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_unrecognized_nested_transcript_stays_standalone() {
        let projects = Path::new("anonymous-projects");
        let nearby = projects
            .join("fixture-project")
            .join("archive")
            .join("nearby.jsonl");
        assert!(matches!(
            claude_relation_for_path(&nearby, projects),
            RelationClaim::Standalone
        ));
    }

    #[test]
    fn claude_indirect_descendant_keeps_root_ownership() {
        let claims = [
            SessionRelationClaim::root("claude", "root"),
            SessionRelationClaim::child(
                "claude",
                "workflow-child",
                "root",
                RelationConfidence::Structural,
            ),
            SessionRelationClaim::child(
                "claude",
                "workflow-grandchild",
                "workflow-child",
                RelationConfidence::Structural,
            ),
        ];
        let normalized = normalize_session_relations(&claims).unwrap();
        let grandchild = normalized
            .iter()
            .find(|node| node.session_id == "workflow-grandchild")
            .unwrap();
        assert_eq!(grandchild.node_kind.as_str(), "child");
        assert_eq!(
            grandchild.parent_session_id.as_deref(),
            Some("workflow-child")
        );
        assert_eq!(grandchild.root_session_id, "root");
    }

    #[test]
    fn claude_missing_timestamp_is_not_rewritten_to_now() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!(
            "cc-switch-claude-missing-time-{}",
            uuid::Uuid::new_v4()
        ));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("missing-time.jsonl"),
            "{\"type\":\"assistant\",\"message\":{\"id\":\"missing-time-msg\",\"model\":\"fixture-unknown\",\"usage\":{\"input_tokens\":7,\"output_tokens\":1,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"stop_reason\":\"end_turn\"}}\n",
        )
        .unwrap();
        sync_claude_files(&db, &projects)?;
        let conn = lock_conn!(db.conn);
        let raw_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:missing-time-msg')",
            [],
            |row| row.get(0),
        )?;
        assert!(!raw_exists, "缺失 event time 不得写 legacy raw row");
        let bucket_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups WHERE session_id = 'missing-time'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            bucket_count, 0,
            "event-time bucket needs a proven timestamp"
        );
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE request_id = 'session:missing-time-msg'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_count, 0, "缺失 event time 不得写 coverage marker");
        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_zero_and_cache_only_usage_are_known_and_unknown_cost_is_null() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let tmp =
            std::env::temp_dir().join(format!("cc-switch-claude-zero-{}", uuid::Uuid::new_v4()));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("zero.jsonl");
        let zero = "{\"type\":\"assistant\",\"sessionId\":\"zero-session\",\"timestamp\":\"2026-08-13T10:01:00Z\",\"message\":{\"id\":\"zero-msg\",\"model\":\"fixture-unknown\",\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"stop_reason\":\"end_turn\"}}";
        let cache_only = "{\"type\":\"assistant\",\"sessionId\":\"zero-session\",\"timestamp\":\"2026-08-13T10:01:01Z\",\"message\":{\"id\":\"cache-msg\",\"model\":\"fixture-unknown\",\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"cache_read_input_tokens\":9,\"cache_creation_input_tokens\":0}}}";
        fs::write(&path, format!("{zero}\n{cache_only}\n")).unwrap();
        sync_claude_files(&db, &projects)?;
        let conn = lock_conn!(db.conn);
        let row: (i64, i64, i64, Option<String>) = conn.query_row(
            "SELECT request_count, input_tokens, cache_read_tokens, total_cost_usd
             FROM agent_session_usage_rollups WHERE session_id = 'zero-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, (2, 0, 9, None));
        drop(conn);
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn claude_partial_components_merge_to_null_and_are_marker_covered() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp =
            std::env::temp_dir().join(format!("cc-switch-claude-partial-{}", uuid::Uuid::new_v4()));
        let projects = tmp.join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("partial.jsonl");
        let complete = r#"{"type":"assistant","sessionId":"partial-session","timestamp":"2026-08-13T10:02:00Z","message":{"id":"partial-complete","model":"fixture-unknown","usage":{"input_tokens":10,"output_tokens":2,"cache_read_input_tokens":4,"cache_creation_input_tokens":7},"stop_reason":"end_turn"}}"#;
        // The omitted cache-creation field is intentionally distinct from a
        // source-reported zero and must remain NULL in the aggregate.
        let partial = r#"{"type":"assistant","sessionId":"partial-session","timestamp":"2026-08-13T10:02:01Z","message":{"id":"partial-missing-cache","model":"fixture-unknown","usage":{"input_tokens":20,"output_tokens":3,"cache_read_input_tokens":5}}}"#;
        fs::write(&path, format!("{complete}\n{partial}\n")).unwrap();

        let first = sync_claude_files(&db, &projects)?;
        assert_eq!(first.imported, 2);
        let conn = lock_conn!(db.conn);
        let row: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'partial-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, (Some(30), Some(5), Some(9), None));
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'session_log'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_count, 2);
        let raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE app_type = 'claude' AND data_source = 'session_log'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(raw_count, 2);
        let effective_filter = effective_session_usage_log_filter("l");
        let visible_marked_raw: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM proxy_request_logs l
                 WHERE {effective_filter} AND l.app_type = 'claude'
                   AND l.data_source = 'session_log'"
            ),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            visible_marked_raw, 0,
            "coverage markers exclude compatibility raw rows"
        );
        drop(conn);

        let second = sync_claude_files(&db, &projects)?;
        assert_eq!(second.imported, 0);
        let conn = lock_conn!(db.conn);
        let marker_count_after: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'claude' AND data_source = 'session_log'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            marker_count_after, 2,
            "cursor replay must not drift markers"
        );
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }
}
