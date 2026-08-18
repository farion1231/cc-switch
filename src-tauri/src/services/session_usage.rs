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
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{
    local_usage_date, normalize_session_relations, NormalizedSessionNode,
    NormalizedUsageRollupFact, RelationClaim, RelationConfidence, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use crate::services::session_usage_pipeline::{
    clear_replaced_coverage_on_conn, publish_canonical_batch_on_conn,
    publish_canonical_batch_on_conn_after_coverage_clear, reserve_canonical_coverage_on_conn,
    CanonicalReplaceScope, CanonicalUsageBatch, RawUsageLogRow, SessionUsageAdapter,
    StaticSessionUsageAdapter, UsagePublishTarget, UsageSourceSpec,
};
use crate::services::usage_stats::{
    effective_usage_log_filter, find_matching_cowork_proxy_usage, find_model_pricing,
    has_matching_proxy_usage_coverage_for_session, session_insert_outcome_excluding_claimed,
    DedupKey, MatchingProxyUsageLog, SessionInsertOutcome,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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

/// One source of truth for native usage adapter order and rebuild ownership.
/// The order intentionally remains the historical sync order: some source
/// adapters consult proxy coverage created by an earlier adapter, so this is
/// an explicit product contract rather than an incidental array order.
pub(crate) fn registered_usage_adapters() -> &'static [StaticSessionUsageAdapter] {
    static ADAPTERS: [StaticSessionUsageAdapter; 9] = [
        StaticSessionUsageAdapter {
            app_type: "claude",
            display_name: "Claude",
            sync: sync_claude_session_logs,
            rebuild_sync: Some(sync_claude_session_logs),
            preflight: Some(preflight_claude_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "codex",
            display_name: "Codex",
            sync: crate::services::session_usage_codex::sync_codex_usage_with_replay,
            rebuild_sync: Some(crate::services::session_usage_codex::sync_codex_usage),
            preflight: Some(crate::services::session_usage_codex::preflight_codex_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "gemini",
            display_name: "Gemini",
            sync: crate::services::session_usage_gemini::sync_gemini_usage,
            rebuild_sync: None,
            preflight: None,
        },
        StaticSessionUsageAdapter {
            app_type: "opencode",
            display_name: "OpenCode",
            sync: crate::services::session_usage_opencode::sync_opencode_usage,
            rebuild_sync: Some(crate::services::session_usage_opencode::sync_opencode_usage),
            preflight: Some(preflight_opencode_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "grokbuild",
            display_name: "Grok Build",
            sync: crate::services::session_usage_grokbuild::sync_grokbuild_usage,
            rebuild_sync: Some(crate::services::session_usage_grokbuild::sync_grokbuild_usage),
            preflight: Some(preflight_grokbuild_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "pi",
            display_name: "Pi",
            sync: crate::services::session_usage_pi::sync_pi_usage,
            rebuild_sync: Some(crate::services::session_usage_pi::sync_pi_usage),
            preflight: Some(preflight_pi_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "openclaw",
            display_name: "OpenClaw",
            sync: sync_openclaw_session_nodes,
            rebuild_sync: None,
            preflight: None,
        },
        StaticSessionUsageAdapter {
            app_type: "hermes",
            display_name: "Hermes",
            sync: sync_hermes_session_nodes,
            rebuild_sync: Some(sync_hermes_session_nodes),
            preflight: Some(preflight_hermes_usage),
        },
        StaticSessionUsageAdapter {
            app_type: "cowork",
            display_name: "Claude Desktop / Cowork",
            sync: sync_cowork_session_usage,
            rebuild_sync: None,
            preflight: None,
        },
    ];
    &ADAPTERS
}

fn preflight_claude_usage() -> Result<(), AppError> {
    ensure_readable_usage_directory(&get_claude_config_dir().join("projects"), "Claude projects")
}

fn preflight_grokbuild_usage() -> Result<(), AppError> {
    let roots = crate::session_manager::providers::grokbuild::session_roots();
    if roots.iter().any(|root| root.is_dir()) {
        Ok(())
    } else {
        Err(AppError::Config(
            "没有找到可用于 Grok Build 用量重建的会话目录".to_string(),
        ))
    }
}

fn preflight_opencode_usage() -> Result<(), AppError> {
    let db_path = crate::opencode_config::get_opencode_db_path();
    let storage = db_path
        .parent()
        .map(|path| path.join("storage"))
        .unwrap_or_else(|| PathBuf::from("storage"));
    if db_path.is_file() || storage.is_dir() {
        Ok(())
    } else {
        Err(AppError::Config(
            "没有找到可用于 OpenCode 用量重建的来源".to_string(),
        ))
    }
}

fn preflight_hermes_usage() -> Result<(), AppError> {
    let root = crate::hermes_config::get_hermes_dir();
    let has_profile_database = root.join("profiles").is_dir()
        && fs::read_dir(root.join("profiles"))
            .ok()
            .into_iter()
            .flatten()
            .any(|entry| {
                entry
                    .ok()
                    .is_some_and(|entry| entry.path().join("state.db").is_file())
            });
    if root.join("state.db").is_file() || has_profile_database {
        Ok(())
    } else {
        Err(AppError::Config(
            "没有找到可用于 Hermes 用量重建的 state.db".to_string(),
        ))
    }
}

fn preflight_pi_usage() -> Result<(), AppError> {
    let files = crate::session_manager::providers::pi::session_files()
        .map_err(|error| AppError::Config(format!("无法发现 Pi 会话来源: {error}")))?;
    if files.is_empty() {
        Err(AppError::Config(
            "没有找到可用于 Pi 用量重建的会话文件".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn ensure_readable_usage_directory(path: &Path, label: &str) -> Result<(), AppError> {
    if !path.is_dir() {
        return Err(AppError::Config(format!(
            "{label} 目录不存在: {}",
            path.display()
        )));
    }
    fs::read_dir(path).map(|_| ()).map_err(|error| {
        AppError::Config(format!("无法读取 {label} 目录 {}: {error}", path.display()))
    })
}

/// 调用方必须持有 [`session_sync_mutex`]。此函数是同步内核，供后台任务、
/// 手动同步和 Codex 重建共享，避免 tokio Mutex 重入。
pub fn sync_all_unlocked(db: &Database) -> SessionSyncResult {
    let mut result = SessionSyncResult::default();
    for adapter in registered_usage_adapters() {
        merge_sync_step(
            &mut result,
            adapter.display_name(),
            SessionUsageAdapter::sync(adapter, db),
        );
    }
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
        db,
        crate::openclaw_config::get_openclaw_dir(),
    )?;
    publish_openclaw_session_nodes(db, scan)
}

fn publish_openclaw_session_nodes(
    db: &Database,
    scan: crate::services::session_usage_openclaw::OpenClawSyncResult,
) -> Result<SessionSyncResult, AppError> {
    if scan.source_revisions.is_empty() && scan.removed_source_paths.is_empty() {
        return Ok(SessionSyncResult {
            skipped: scan.files_skipped,
            files_scanned: scan.files_scanned,
            errors: scan.errors,
            ..SessionSyncResult::default()
        });
    }
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 OpenClaw canonical 事务失败: {error}"))
    })?;
    for source_path in &scan.removed_source_paths {
        tx.execute(
            "DELETE FROM agent_session_nodes
             WHERE app_type = 'openclaw' AND source_path = ?1",
            [source_path],
        )?;
        tx.execute(
            "DELETE FROM session_log_sync WHERE file_path = ?1",
            [source_path],
        )?;
    }
    if !scan.relation_claims.is_empty() {
        let canonical_batch = CanonicalUsageBatch {
            relation_claims: scan.relation_claims,
            ..CanonicalUsageBatch::default()
        };
        publish_canonical_batch_on_conn(&tx, UsagePublishTarget::Published, canonical_batch)?;
    }
    for revision in &scan.source_revisions {
        update_sync_state_on_conn(
            &tx,
            &revision.file_path,
            revision.last_modified,
            revision.last_offset,
        )?;
    }
    tx.commit().map_err(|error| {
        AppError::Database(format!("提交 OpenClaw canonical 事务失败: {error}"))
    })?;

    Ok(SessionSyncResult {
        skipped: scan.files_skipped,
        files_scanned: scan.files_scanned,
        errors: scan.errors,
        ..SessionSyncResult::default()
    })
}

#[cfg(test)]
fn sync_openclaw_session_nodes_from_root(
    db: &Database,
    root: &Path,
) -> Result<SessionSyncResult, AppError> {
    let scan = crate::services::session_usage_openclaw::sync_openclaw_session_usage(db, root)?;
    publish_openclaw_session_nodes(db, scan)
}

/// Register Hermes' detailed read-only source in the shared sync pass.
/// Its adapter publishes standalone nodes, full-dimension facts, and
/// cumulative snapshots as one canonical transaction; Hermes facts are never
/// flattened into `proxy_request_logs` or written a second time here.
fn sync_hermes_session_nodes(db: &Database) -> Result<SessionSyncResult, AppError> {
    crate::services::session_usage_hermes::sync_hermes_usage_detailed(db)
}

/// Register Cowork's canonical `claude-desktop` source.  Its adapter emits
/// canonical buckets only; proxy/raw arbitration remains a central concern.
/// Cowork emits no `cowork_session` compatibility rows. When a real proxy row
/// matches, its coverage marker transfers task/session attribution to the
/// transcript while the proxy row remains the global request record.
fn sync_cowork_session_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut cowork = crate::services::session_usage_cowork::collect_cowork_usage(db)?;
    let source_revisions = cowork.source_revisions.clone();
    if source_revisions.is_empty() {
        return Ok(cowork.result);
    }
    arbitrate_cowork_proxy_rows(db, &mut cowork.canonical_batch, &source_revisions)?;
    Ok(cowork.result)
}

/// Apply the central Desktop gateway arbitration after the Cowork adapter has
/// parsed its transcript. A matching successful `claude-desktop` proxy row
/// remains the global record, while the transcript observation stays canonical
/// for native task attribution. The coverage reservation and canonical fact
/// commit together; cache-token differences remain distinct.
pub(crate) fn arbitrate_cowork_proxy_rows(
    db: &Database,
    batch: &mut CanonicalUsageBatch,
    source_revisions: &[crate::services::session_usage_cowork::CoworkSourceRevision],
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 Cowork 来源仲裁事务失败: {error}")))?;

    clear_replaced_coverage_on_conn(&tx, UsagePublishTarget::Published, &batch.replace_scopes)?;
    for observation in &batch.replacement_observations {
        let fact = &observation.fact;
        let (
            Some(event_at),
            Some(input_tokens),
            Some(output_tokens),
            Some(cache_read_tokens),
            Some(cache_creation_tokens),
        ) = (
            fact.first_event_at,
            fact.input_tokens,
            fact.output_tokens,
            fact.cache_read_tokens,
            fact.cache_creation_tokens,
        )
        else {
            continue;
        };
        if let Some(proxy) = find_matching_cowork_proxy_usage(
            &tx,
            &fact.model,
            event_at,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        )? {
            reserve_canonical_coverage_on_conn(
                &tx,
                UsagePublishTarget::Published,
                &proxy.app_type,
                "proxy",
                &proxy.request_id,
                Some(&fact.session_id),
                event_at,
            )?;
        }
    }
    publish_canonical_batch_on_conn_after_coverage_clear(
        &tx,
        UsagePublishTarget::Published,
        std::mem::take(batch),
    )?;
    for revision in source_revisions {
        update_sync_state_on_conn(
            &tx,
            &revision.file_path,
            revision.last_modified,
            revision.last_offset,
        )?;
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Cowork 来源仲裁事务失败: {error}")))?;
    Ok(())
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

fn source_token_components(message: &ParsedAssistantUsage) -> [Option<i64>; 4] {
    [
        message.input_tokens,
        message.output_tokens,
        message.cache_read_tokens,
        message.cache_creation_tokens,
    ]
}

fn parse_token_component(usage: &serde_json::Value, field: &str) -> Option<i64> {
    usage
        .get(field)
        .and_then(|value| value.as_u64())
        .and_then(|value| i64::try_from(value).ok())
}

fn has_source_token_component(message: &ParsedAssistantUsage) -> bool {
    source_token_components(message)
        .into_iter()
        .any(|value| value.is_some())
}

fn all_source_token_components_known(message: &ParsedAssistantUsage) -> bool {
    source_token_components(message)
        .into_iter()
        .all(|value| value.is_some())
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

#[derive(Debug, Clone)]
struct PreparedClaudeMessage {
    request_id: String,
    message: ParsedAssistantUsage,
    raw_exists: bool,
    matched_proxy: Option<MatchingProxyUsageLog>,
    skip_raw: bool,
}

struct ClaudeFlushState<'a> {
    file_path: &'a str,
    last_modified: i64,
    last_offset: i64,
    refresh_existing_raw: bool,
    replacement_session_id: Option<&'a str>,
}

struct ParsedClaudeFile {
    line_offset: i64,
    current_session_id: Option<String>,
    messages: HashMap<String, ParsedAssistantUsage>,
    complete: bool,
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
    let normalized_by_identity: HashMap<(String, String), NormalizedSessionNode> = normalized_nodes
        .into_iter()
        .map(|node| ((node.app_type.clone(), node.session_id.clone()), node))
        .collect();

    for (file_path, identity) in &files {
        result.files_scanned += 1;
        let node = normalized_by_identity
            .get(&(identity.claim.app_type.clone(), identity.session_id.clone()))
            .ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "Claude relation normalizer omitted {}:{}",
                    identity.claim.app_type, identity.session_id
                ))
            })?;

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
    sync_single_file_with_context(db, file_path, identity.as_ref(), node.as_ref())
}

fn parse_claude_usage_file(
    file_path: &Path,
    identity: Option<&ClaudeFileIdentity>,
    start_offset: i64,
) -> Result<ParsedClaudeFile, AppError> {
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);
    let mut parsed_file = ParsedClaudeFile {
        line_offset: 0,
        current_session_id: None,
        messages: HashMap::new(),
        complete: true,
    };

    for line_result in reader.lines() {
        parsed_file.line_offset += 1;
        if parsed_file.line_offset <= start_offset {
            continue;
        }

        let line = match line_result {
            Ok(line) => line,
            Err(_) => {
                parsed_file.complete = false;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                parsed_file.complete = false;
                continue;
            }
        };
        if parsed_file.current_session_id.is_none() {
            if let Some(session_id) = value.get("sessionId").and_then(|value| value.as_str()) {
                parsed_file.current_session_id = Some(session_id.to_string());
            }
        }
        if value.get("type").and_then(|value| value.as_str()) != Some("assistant") {
            continue;
        }

        let Some(message) = value.get("message") else {
            parsed_file.complete = false;
            continue;
        };
        let Some(message_id) = message.get("id").and_then(|value| value.as_str()) else {
            parsed_file.complete = false;
            continue;
        };
        let Some(usage) = message.get("usage") else {
            parsed_file.complete = false;
            continue;
        };
        let parsed = ParsedAssistantUsage {
            message_id: message_id.to_string(),
            model: message
                .get("model")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string(),
            input_tokens: parse_token_component(usage, "input_tokens"),
            output_tokens: parse_token_component(usage, "output_tokens"),
            cache_read_tokens: parse_token_component(usage, "cache_read_input_tokens"),
            cache_creation_tokens: parse_token_component(usage, "cache_creation_input_tokens"),
            stop_reason: message
                .get("stop_reason")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            timestamp: value
                .get("timestamp")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            session_id: identity
                .map(|value| value.session_id.clone())
                .or_else(|| parsed_file.current_session_id.clone()),
        };

        // Repeated message IDs are progressive snapshots of one Claude
        // response. Prefer a completed record, otherwise the largest output.
        let should_replace = match parsed_file.messages.get(message_id) {
            None => true,
            Some(existing) if parsed.stop_reason.is_some() && existing.stop_reason.is_none() => {
                true
            }
            Some(existing) if parsed.stop_reason.is_some() == existing.stop_reason.is_some() => {
                parsed.output_tokens > existing.output_tokens
            }
            Some(_) => false,
        };
        if should_replace {
            parsed_file.messages.insert(message_id.to_string(), parsed);
        }
    }

    Ok(parsed_file)
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
    let mut canonical_batch = CanonicalUsageBatch::default();
    if let Some(node) = node {
        canonical_batch.nodes.push(node.clone());
    }

    // A file older than its committed cursor cannot safely be replayed. Equal
    // mtimes remain eligible below because some filesystems preserve them for
    // consecutive appends; the incremental line count distinguishes a real
    // append from an unchanged transcript.
    if file_modified < last_modified {
        flush_claude_rollups(
            db,
            canonical_batch,
            &[],
            &[],
            ClaudeFlushState {
                file_path: &file_path_str,
                last_modified,
                last_offset,
                refresh_existing_raw: false,
                replacement_session_id: None,
            },
        )?;
        return Ok((0, 0));
    }

    // Most appends only parse new lines. A repeated covered message means
    // Claude appended a later snapshot of an existing response; a truncated
    // file or a same-length file with a newer mtime needs full replacement.
    let mut parsed_file = parse_claude_usage_file(file_path, identity, last_offset)?;
    if file_modified == last_modified && parsed_file.line_offset == last_offset {
        return Ok((0, 0));
    }
    let covered_message_seen = {
        let conn = lock_conn!(db.conn);
        parsed_file
            .messages
            .keys()
            .try_fold(false, |seen, message_id| {
                if seen {
                    Ok(true)
                } else {
                    let request_id = format!(
                        "{}{}",
                        crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
                        message_id
                    );
                    Database::has_agent_session_canonical_coverage_on_conn(
                        &conn,
                        "claude",
                        "session_log",
                        &request_id,
                    )
                }
            })?
    };
    let requires_full_replacement = covered_message_seen || parsed_file.line_offset <= last_offset;
    if requires_full_replacement {
        parsed_file = parse_claude_usage_file(file_path, identity, 0)?;
    }
    let line_offset = parsed_file.line_offset;
    let current_session_id = parsed_file.current_session_id;
    let messages = parsed_file.messages;
    let mut replacement_is_complete = requires_full_replacement && parsed_file.complete;
    // Keep the JSONL identity as a fallback when the incremental cursor starts
    // after the metadata line.
    let fallback_session_id = identity.map(|value| value.session_id.clone());

    // 写入数据库
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    let mut proxy_coverage = Vec::new();
    let mut claimed_proxy_request_ids = HashSet::new();
    let mut prepared_messages = Vec::new();
    let mut canonical_candidates = Vec::new();
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
            replacement_is_complete = false;
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

        let fact = node.and_then(|node| claude_rollup_for_message(db, node, &msg));
        let Some(fact) = fact else {
            replacement_is_complete = false;
            skipped += 1;
            continue;
        };

        let canonical_session_id = fact.session_id.clone();
        canonical_candidates.push((request_id.clone(), fact, covered));
        if let Some(proxy) = matched_proxy.as_ref() {
            proxy_coverage.push((proxy.clone(), canonical_session_id));
        }
        prepared_messages.push(PreparedClaudeMessage {
            request_id,
            message: msg,
            raw_exists,
            matched_proxy,
            skip_raw,
        });
    }

    if replacement_is_complete {
        if let Some(node) = node {
            canonical_batch.replace_scopes.push(CanonicalReplaceScope {
                app_type: "claude".into(),
                session_id: node.session_id.clone(),
                data_source: "session_log".into(),
            });
        } else {
            replacement_is_complete = false;
        }
    }
    for (request_id, fact, covered) in canonical_candidates {
        if replacement_is_complete {
            canonical_batch.replace_observe(request_id, fact);
        } else if !covered {
            canonical_batch.observe(request_id, fact);
        }
    }

    let (new_imported, new_skipped) = flush_claude_rollups(
        db,
        canonical_batch,
        &proxy_coverage,
        &prepared_messages,
        ClaudeFlushState {
            file_path: &file_path_str,
            last_modified: file_modified,
            last_offset: line_offset,
            refresh_existing_raw: replacement_is_complete,
            replacement_session_id: if replacement_is_complete {
                node.map(|node| node.session_id.as_str())
            } else {
                None
            },
        },
    )?;
    imported = imported.saturating_add(new_imported);
    skipped = skipped.saturating_add(new_skipped);

    Ok((imported, skipped))
}

fn parsed_event_timestamp(message: &ParsedAssistantUsage) -> Option<i64> {
    message.timestamp.as_deref().and_then(|timestamp| {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .ok()
            .map(|value| value.timestamp())
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
    let date = local_usage_date(event_at)?;
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

    let source = UsageSourceSpec::new(
        "claude",
        "_session",
        "session_log",
        UsagePrecision::RequestExact,
        TimeSemantics::EventTime,
        RequestCountSemantics::AssistantMessage,
    );
    let mut fact = source.fact(
        date,
        session_id,
        message.model.clone(),
        message.model.clone(),
        pricing_model,
    );
    fact.request_count = Some(1);
    fact.input_tokens = message.input_tokens;
    fact.output_tokens = message.output_tokens;
    fact.cache_read_tokens = message.cache_read_tokens;
    fact.cache_creation_tokens = message.cache_creation_tokens;
    fact.total_cost_usd = total_cost_usd;
    fact.cost_status = Some(if all_source_token_components_known(message) {
        if pricing.is_some() {
            "complete".to_string()
        } else {
            "unknown".to_string()
        }
    } else {
        "partial".to_string()
    });
    fact.cost_source = Some("session_log".to_string());
    fact.first_event_at = Some(event_at);
    fact.last_event_at = Some(event_at);
    Some(fact)
}

fn flush_claude_rollups(
    db: &Database,
    canonical_batch: CanonicalUsageBatch,
    proxy_coverage: &[(MatchingProxyUsageLog, String)],
    prepared_messages: &[PreparedClaudeMessage],
    state: ClaudeFlushState<'_>,
) -> Result<(u32, u32), AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 Claude canonical 覆盖事务失败: {error}"))
    })?;
    let marked_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    if state.refresh_existing_raw {
        let session_id = state.replacement_session_id.ok_or_else(|| {
            AppError::InvalidInput("Claude complete replacement lacks a session ID".into())
        })?;
        delete_stale_claude_session_rows_on_conn(&tx, session_id, prepared_messages)?;
    }
    publish_canonical_batch_on_conn(&tx, UsagePublishTarget::Published, canonical_batch)?;
    for (proxy, canonical_session_id) in proxy_coverage {
        // The proxy row remains the global request record, while this marker
        // assigns the session/task representation to the native transcript.
        // Preserve the stored app type so a Claude Desktop proxy row still
        // matches the coverage key exactly.
        reserve_canonical_coverage_on_conn(
            &tx,
            UsagePublishTarget::Published,
            &proxy.app_type,
            "proxy",
            &proxy.request_id,
            Some(canonical_session_id),
            marked_at,
        )?;
    }

    // Raw compatibility rows, canonical facts, and both source coverage
    // markers share this transaction.  A matched proxy event intentionally
    // skips its duplicate native raw row while still publishing the fact and
    // markers above.  Existing source rows remain idempotent no-ops.
    let mut imported = 0u32;
    let mut skipped = 0u32;
    for prepared in prepared_messages {
        if prepared.matched_proxy.is_some()
            || prepared.skip_raw
            || (prepared.raw_exists && !state.refresh_existing_raw)
        {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match insert_session_log_entry_on_conn(
            &tx,
            &prepared.request_id,
            &prepared.message,
            prepared.raw_exists && state.refresh_existing_raw,
        ) {
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
    // The cursor participates in the same commit as raw compatibility rows,
    // canonical facts, and ownership markers.  A failed publish therefore
    // cannot make an admitted Claude event permanently invisible.
    update_sync_state_on_conn(&tx, state.file_path, state.last_modified, state.last_offset)?;
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

/// A complete Claude transcript is a replacement snapshot for one source
/// session. Remove native compatibility rows for messages that disappeared
/// from that snapshot in the same transaction as the canonical replacement.
/// Proxy rows and other native sources remain outside this ownership scope.
fn delete_stale_claude_session_rows_on_conn(
    conn: &rusqlite::Connection,
    session_id: &str,
    prepared_messages: &[PreparedClaudeMessage],
) -> Result<(), AppError> {
    let current_request_ids = prepared_messages
        .iter()
        .map(|message| message.request_id.as_str())
        .collect::<HashSet<_>>();
    let mut statement = conn
        .prepare(
            "SELECT request_id FROM proxy_request_logs
             WHERE app_type = 'claude' AND data_source = 'session_log' AND session_id = ?1",
        )
        .map_err(|error| {
            AppError::Database(format!("读取 Claude raw replacement 范围失败: {error}"))
        })?;
    let stale_request_ids = statement
        .query_map([session_id], |row| row.get::<_, String>(0))
        .map_err(|error| AppError::Database(format!("查询过期 Claude raw 行失败: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(format!("读取过期 Claude raw 行失败: {error}")))?
        .into_iter()
        .filter(|request_id| !current_request_ids.contains(request_id.as_str()))
        .collect::<Vec<_>>();

    for request_id in stale_request_ids {
        conn.execute(
            "DELETE FROM proxy_request_logs
             WHERE request_id = ?1
               AND app_type = 'claude' AND data_source = 'session_log' AND session_id = ?2",
            rusqlite::params![request_id, session_id],
        )
        .map_err(|error| AppError::Database(format!("删除过期 Claude raw 行失败: {error}")))?;
    }
    Ok(())
}

/// Connection-scoped Claude raw writer used by the canonical transaction.
/// A completed repeated message may refresh only its existing Claude-native
/// row; a proxy-owned request with the same ID remains untouched.
fn insert_session_log_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    refresh_existing: bool,
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

    let mut row = RawUsageLogRow::native_session(
        request_id,
        "_session",
        "claude",
        msg.model.as_str(),
        Some(session_id),
        "session_log",
        created_at,
    );
    row.input_tokens = i64::from(token_value_for_raw(msg.input_tokens));
    row.output_tokens = i64::from(token_value_for_raw(msg.output_tokens));
    row.cache_read_tokens = i64::from(token_value_for_raw(msg.cache_read_tokens));
    row.cache_creation_tokens = i64::from(token_value_for_raw(msg.cache_creation_tokens));
    row.input_cost_usd = input_cost;
    row.output_cost_usd = output_cost;
    row.cache_read_cost_usd = cache_read_cost;
    row.cache_creation_cost_usd = cache_creation_cost;
    row.total_cost_usd = total_cost;
    if refresh_existing {
        let updated = conn
            .execute(
                "UPDATE proxy_request_logs
                 SET model = ?1, request_model = ?1,
                     input_tokens = ?2, output_tokens = ?3,
                     cache_read_tokens = ?4, cache_creation_tokens = ?5,
                     input_cost_usd = ?6, output_cost_usd = ?7,
                     cache_read_cost_usd = ?8, cache_creation_cost_usd = ?9,
                     total_cost_usd = ?10, session_id = ?11, created_at = ?12
                 WHERE request_id = ?13
                   AND app_type = 'claude' AND data_source = 'session_log'
                   AND (
                       model IS NOT ?1 OR request_model IS NOT ?1
                       OR input_tokens IS NOT ?2 OR output_tokens IS NOT ?3
                       OR cache_read_tokens IS NOT ?4 OR cache_creation_tokens IS NOT ?5
                       OR input_cost_usd IS NOT ?6 OR output_cost_usd IS NOT ?7
                       OR cache_read_cost_usd IS NOT ?8 OR cache_creation_cost_usd IS NOT ?9
                       OR total_cost_usd IS NOT ?10 OR session_id IS NOT ?11
                       OR created_at IS NOT ?12
                   )",
                rusqlite::params![
                    &row.model,
                    row.input_tokens,
                    row.output_tokens,
                    row.cache_read_tokens,
                    row.cache_creation_tokens,
                    &row.input_cost_usd,
                    &row.output_cost_usd,
                    &row.cache_read_cost_usd,
                    &row.cache_creation_cost_usd,
                    &row.total_cost_usd,
                    &row.session_id,
                    row.created_at,
                    &row.request_id,
                ],
            )
            .map_err(|error| {
                AppError::Database(format!(
                    "更新 Claude 会话 raw compatibility 行失败: {error}"
                ))
            })?;
        if updated > 0 {
            return Ok(true);
        }
    }
    row.insert_or_ignore_on_conn(conn, UsagePublishTarget::Published)
        .map_err(|error| AppError::Database(format!("插入会话日志失败: {error}")))
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
    use std::io::Write;
    use tempfile::tempdir;

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

        let tmp = tempdir().expect("tempdir");
        let projects = tmp.path().join("projects");
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
        Ok(())
    }

    #[test]
    fn duplicate_claude_session_files_reuse_normalized_node() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempdir().expect("tempdir");
        let projects = tmp.path().join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        for (file_name, message_id) in
            [("copy-a.jsonl", "message-a"), ("copy-b.jsonl", "message-b")]
        {
            let line = format!(
                "{{\"type\":\"assistant\",\"sessionId\":\"copied-session\",\"timestamp\":\"1970-01-01T00:16:45Z\",\"message\":{{\"id\":\"{message_id}\",\"model\":\"fixture-unknown\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":2,\"cache_read_input_tokens\":1,\"cache_creation_input_tokens\":0}},\"stop_reason\":\"end_turn\"}}}}\n"
            );
            fs::write(project.join(file_name), line).unwrap();
        }

        let result = sync_claude_files(&db, &projects)?;
        assert_eq!(result.files_scanned, 2);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
        let conn = lock_conn!(db.conn);
        let node_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_nodes
             WHERE app_type = 'claude' AND session_id = 'copied-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(node_count, 1);
        Ok(())
    }

    #[test]
    fn unchanged_openclaw_session_does_not_republish_node() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("openclaw");
        let sessions = root.join("agents").join("fixture-agent").join("sessions");
        fs::create_dir_all(&sessions).expect("create OpenClaw sessions");
        let path = sessions.join("fixture.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session\",\"id\":\"fixture\",\"cwd\":\"/first\",\"timestamp\":\"2026-08-18T00:00:00Z\"}\n",
        )
        .expect("write OpenClaw fixture");
        fs::write(
            sessions.join("sessions.json"),
            "{\"fixture\":{\"sessionId\":\"fixture\",\"displayName\":\"Initial title\"}}",
        )
        .expect("write OpenClaw index");

        assert_eq!(
            sync_openclaw_session_nodes_from_root(&db, &root)?.files_scanned,
            1
        );
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE agent_session_nodes
                 SET last_synced_at = 1
                 WHERE app_type = 'openclaw' AND session_id = 'fixture-agent:fixture'",
                [],
            )?;
        }

        let idle = sync_openclaw_session_nodes_from_root(&db, &root)?;
        assert_eq!(idle.files_scanned, 1);
        let unchanged: (i64, String) = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT last_synced_at, title
                 FROM agent_session_nodes
                 WHERE app_type = 'openclaw' AND session_id = 'fixture-agent:fixture'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
        };
        assert_eq!(unchanged, (1, "Initial title".to_string()));

        fs::write(
            sessions.join("sessions.json"),
            "{\"fixture\":{\"sessionId\":\"fixture\",\"displayName\":\"Updated fixture title\"}}",
        )
        .expect("rewrite OpenClaw index");
        let refreshed = sync_openclaw_session_nodes_from_root(&db, &root)?;
        assert_eq!(refreshed.files_scanned, 1);
        let updated: String = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT title FROM agent_session_nodes
                 WHERE app_type = 'openclaw' AND session_id = 'fixture-agent:fixture'",
                [],
                |row| row.get(0),
            )?
        };
        assert_eq!(updated, "Updated fixture title");
        Ok(())
    }

    #[test]
    fn deleted_openclaw_session_removes_node_and_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("openclaw");
        let sessions = root.join("agents").join("fixture-agent").join("sessions");
        fs::create_dir_all(&sessions).expect("create OpenClaw sessions");
        let session_path = sessions.join("fixture.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session\",\"id\":\"fixture\",\"cwd\":\"/first\",\"timestamp\":\"2026-08-18T00:00:00Z\"}\n",
        )
        .expect("write OpenClaw fixture");
        fs::write(
            sessions.join("sessions.json"),
            "{\"fixture\":{\"sessionId\":\"fixture\",\"displayName\":\"Fixture\"}}",
        )
        .expect("write OpenClaw index");

        sync_openclaw_session_nodes_from_root(&db, &root)?;
        fs::remove_file(&session_path).expect("delete OpenClaw fixture");
        sync_openclaw_session_nodes_from_root(&db, &root)?;

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_session_nodes
                 WHERE app_type = 'openclaw' AND session_id = 'fixture-agent:fixture'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1)",
            [session_path.to_string_lossy().as_ref()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(counts, (0, 0));
        Ok(())
    }

    #[test]
    fn missing_openclaw_root_preserves_existing_nodes_and_cursors() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("openclaw");
        let sessions = root.join("agents").join("fixture-agent").join("sessions");
        fs::create_dir_all(&sessions).expect("create OpenClaw sessions");
        let session_path = sessions.join("fixture.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session\",\"id\":\"fixture\",\"cwd\":\"/first\",\"timestamp\":\"2026-08-18T00:00:00Z\"}\n",
        )
        .expect("write OpenClaw fixture");
        sync_openclaw_session_nodes_from_root(&db, &root)?;

        fs::remove_dir_all(&root).expect("remove OpenClaw root");
        sync_openclaw_session_nodes_from_root(&db, &root)?;

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_session_nodes
                 WHERE app_type = 'openclaw' AND session_id = 'fixture-agent:fixture'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1)",
            [session_path.to_string_lossy().as_ref()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(counts, (1, 1));
        Ok(())
    }

    #[test]
    fn claude_completed_message_replaces_covered_canonical_and_raw() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let projects = temp.path().join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("progressive.jsonl");
        let provisional = r#"{"type":"assistant","sessionId":"progressive-session","timestamp":"2026-08-13T10:00:00Z","message":{"id":"progressive-message","model":"provisional-model","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        let completed = r#"{"type":"assistant","sessionId":"progressive-session","timestamp":"2026-08-14T10:00:00Z","message":{"id":"progressive-message","model":"completed-model","usage":{"input_tokens":2,"output_tokens":9,"cache_read_input_tokens":3,"cache_creation_input_tokens":1},"stop_reason":"end_turn"}}"#;
        fs::write(&path, format!("{provisional}\n")).unwrap();

        let first = sync_claude_files(&db, &projects)?;
        assert_eq!(first.imported, 1);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(format!("{completed}\n").as_bytes())
            .unwrap();

        let second = sync_claude_files(&db, &projects)?;
        assert_eq!(second.imported, 1, "the native raw row should be refreshed");
        {
            let conn = lock_conn!(db.conn);
            let rollups: Vec<(String, String, i64, i64, i64)> = conn
                .prepare(
                    "SELECT date, model, request_count, output_tokens, cache_read_tokens
                     FROM agent_session_usage_rollups
                     WHERE app_type = 'claude' AND session_id = 'progressive-session'",
                )?
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
            assert_eq!(
                rollups,
                vec![("2026-08-14".into(), "completed-model".into(), 1, 9, 3)]
            );
            let raw: (i64, String, i64, i64, i64) = conn.query_row(
                "SELECT COUNT(*), model, output_tokens, cache_read_tokens, created_at
                 FROM proxy_request_logs
                 WHERE request_id = 'session:progressive-message'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )?;
            assert_eq!(raw, (1, "completed-model".into(), 9, 3, 1_786_701_600));
        }

        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(format!("{completed}\n").as_bytes())
            .unwrap();
        let repeated = sync_claude_files(&db, &projects)?;
        assert_eq!(repeated.imported, 0);
        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_session_usage_rollups
                 WHERE app_type = 'claude' AND session_id = 'progressive-session'),
                (SELECT COUNT(*) FROM proxy_request_logs
                 WHERE request_id = 'session:progressive-message'),
                (SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND data_source = 'session_log'
                   AND request_id = 'session:progressive-message')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (1, 1, 1));
        Ok(())
    }

    #[test]
    fn complete_claude_rescan_removes_obsolete_compatibility_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let projects = temp.path().join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).expect("create project");
        let path = project.join("rewrite.jsonl");
        let retained = r#"{"type":"assistant","sessionId":"rewrite-session","timestamp":"2026-08-13T10:00:00Z","message":{"id":"retained-message","model":"fixture-unknown","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"stop_reason":"end_turn"}}"#;
        let removed = r#"{"type":"assistant","sessionId":"rewrite-session","timestamp":"2026-08-13T10:01:00Z","message":{"id":"removed-message","model":"fixture-unknown","usage":{"input_tokens":3,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"stop_reason":"end_turn"}}"#;
        fs::write(&path, format!("{retained}\n{removed}\n")).expect("write initial transcript");
        assert_eq!(sync_claude_files(&db, &projects)?.imported, 2);

        // A shorter complete transcript forces a full source/session
        // replacement. The deleted message must not survive through the raw
        // compatibility fallback after its canonical fact is removed.
        fs::write(&path, format!("{retained}\n")).expect("write rewritten transcript");
        sync_claude_files(&db, &projects)?;

        let conn = lock_conn!(db.conn);
        let raw_request_ids: Vec<String> = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE app_type = 'claude' AND data_source = 'session_log'
                   AND session_id = 'rewrite-session'
                 ORDER BY request_id",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        assert_eq!(raw_request_ids, vec!["session:retained-message"]);
        let canonical_counts: (i64, i64) = conn.query_row(
            "SELECT
                (SELECT COALESCE(SUM(request_count), 0)
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'claude' AND session_id = 'rewrite-session'),
                (SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude' AND data_source = 'session_log'
                   AND request_id = 'session:removed-message')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(canonical_counts, (1, 0));
        Ok(())
    }

    #[test]
    fn unchanged_claude_transcript_skips_replacement_but_same_timestamp_append_imports(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let projects = temp.path().join("projects");
        let project = projects.join("fixture-project");
        fs::create_dir_all(&project).expect("create project");
        let path = project.join("stable.jsonl");
        let first_line = r#"{"type":"assistant","sessionId":"stable-session","timestamp":"2026-08-13T10:00:00Z","message":{"id":"stable-first","model":"fixture-unknown","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"stop_reason":"end_turn"}}"#;
        fs::write(&path, format!("{first_line}\n")).expect("write initial transcript");

        assert_eq!(sync_claude_files(&db, &projects)?.imported, 1);
        let file_path = path.to_string_lossy().to_string();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE agent_session_nodes
                 SET last_synced_at = 1
                 WHERE app_type = 'claude' AND session_id = 'stable-session'",
                [],
            )?;
        }

        assert_eq!(sync_claude_files(&db, &projects)?.imported, 0);
        let (last_synced_at, last_offset): (i64, i64) = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT n.last_synced_at, s.last_line_offset
                 FROM agent_session_nodes n
                 JOIN session_log_sync s ON s.file_path = ?1
                 WHERE n.app_type = 'claude' AND n.session_id = 'stable-session'",
                [&file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
        };
        assert_eq!(
            last_synced_at, 1,
            "unchanged input must not republish the node"
        );
        assert_eq!(last_offset, 1);

        let second_line = r#"{"type":"assistant","sessionId":"stable-session","timestamp":"2026-08-13T10:01:00Z","message":{"id":"stable-second","model":"fixture-unknown","usage":{"input_tokens":3,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"stop_reason":"end_turn"}}"#;
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open transcript")
            .write_all(format!("{second_line}\n").as_bytes())
            .expect("append transcript");

        // Simulate a coarse filesystem whose append preserved the old mtime.
        let appended_modified = metadata_modified_nanos(&fs::metadata(&path).expect("metadata"));
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE session_log_sync SET last_modified = ?1 WHERE file_path = ?2",
                rusqlite::params![appended_modified, file_path],
            )?;
        }

        assert_eq!(sync_claude_files(&db, &projects)?.imported, 1);
        let conn = lock_conn!(db.conn);
        let request_count: i64 = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0)
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'stable-session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(request_count, 2);
        Ok(())
    }

    #[test]
    fn cowork_proxy_takeover_preserves_native_task_ownership() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
                ) VALUES ('cowork-proxy', 'gateway', 'claude-desktop', 'cowork-model',
                          'cowork-model', 100, 20, 10, 5, '0.10', 0, 200, NULL, 1000, 'proxy')",
                [],
            )?;
        }

        let temp = tempdir().expect("tempdir");
        let project = temp
            .path()
            .join("workspace")
            .join("group")
            .join("local_fixture")
            .join(".claude")
            .join("projects")
            .join("demo-project");
        fs::create_dir_all(&project).expect("create Cowork fixture project");
        fs::write(
            project.join("cowork-session.jsonl"),
            concat!(
                r#"{"sessionId":"cowork-session","type":"assistant","message":{"id":"cowork-message","model":"cowork-model","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}},"timestamp":"1970-01-01T00:16:45Z"}"#,
                "\n",
            ),
        )
        .expect("write Cowork fixture");

        let roots = [temp.path().to_path_buf()];
        for _ in 0..2 {
            let mut cowork =
                crate::services::session_usage_cowork::collect_cowork_usage_from_roots(&roots)?;
            assert_eq!(cowork.result.imported, 1);
            arbitrate_cowork_proxy_rows(&db, &mut cowork.canonical_batch, &[])?;
        }

        let conn = lock_conn!(db.conn);
        let raw: (i64, i64) = conn.query_row(
            "SELECT COUNT(*), SUM(input_tokens)
             FROM proxy_request_logs WHERE app_type = 'claude-desktop'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(raw, (1, 100));
        let rollup: (i64, i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), request_count, input_tokens, output_tokens, cache_read_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude-desktop' AND session_id = 'cowork-session'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(rollup, (1, 1, 100, 20, 10));

        let markers = conn
            .prepare(
                "SELECT data_source, request_id, canonical_session_id
                 FROM agent_session_canonical_coverage
                 WHERE app_type = 'claude-desktop'
                 ORDER BY data_source, request_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            markers,
            vec![
                (
                    "cowork_session".into(),
                    "cowork:cowork-session:cowork-message".into(),
                    Some("cowork-session".into()),
                ),
                (
                    "proxy".into(),
                    "cowork-proxy".into(),
                    Some("cowork-session".into()),
                ),
            ]
        );

        let task_filter = crate::services::usage_stats::effective_session_usage_log_filter("l");
        let task_raw_count: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM proxy_request_logs l
                 WHERE l.app_type = 'claude-desktop' AND {task_filter}"
            ),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(task_raw_count, 0, "task path must use the canonical fact");
        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_includes_workflow_subagents() {
        // Claude Code Workflow 把子 agent transcript 嵌在
        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/ 下，比普通子 agent 深一层。
        let tmp = tempdir().expect("tempdir");
        let project = tmp.path().join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        let wf_dir = subagents_dir.join("workflows").join("wf_test123");
        fs::create_dir_all(&wf_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-plain.jsonl"), "{}").unwrap();
        fs::write(wf_dir.join("agent-wf.jsonl"), "{}").unwrap();
        // journal.jsonl 也会被收集，但解析时因无 assistant 行而产出 0 条
        fs::write(wf_dir.join("journal.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(tmp.path());
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
    }

    #[test]
    fn test_sync_imports_billable_message_without_stop_reason() -> Result<(), AppError> {
        // 回归：stop_reason 缺失但有真实 cache/input 成本的 message（Workflow /
        // 子 agent 常见的「只有 message_start 快照、没写最终块」形态）必须被计入，
        // 不能因缺 stop_reason 或 output==0 而整条丢弃；全 0 token 的占位行仍应跳过。
        let db = Database::memory()?;
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("agent-wf.jsonl");

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

        Ok(())
    }

    #[test]
    fn claude_missing_timestamp_is_not_rewritten_to_now() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempdir().expect("tempdir");
        let projects = tmp.path().join("projects");
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
        Ok(())
    }

    #[test]
    fn claude_zero_and_cache_only_usage_are_known_and_unknown_cost_is_null() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let tmp = tempdir().expect("tempdir");
        let projects = tmp.path().join("projects");
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
        Ok(())
    }
}
