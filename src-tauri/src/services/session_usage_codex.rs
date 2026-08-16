//! Codex 会话日志使用追踪
//!
//! 从 ~/.codex/sessions/ 下的 JSONL 会话文件中提取精确 token 使用数据，
//! 替代原有的 state_5.sqlite 估算方案。
//!
//! ## 数据流
//! ```text
//! ~/.codex/sessions/YYYY/MM/DD/*.jsonl → 增量解析 → delta 计算 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 解析的事件类型
//! - `session_meta` → 提取唯一 thread_id（子代理的 session_id 指向父线程）
//! - `turn_context` → 提取当前 model
//! - `event_msg` (type=token_count) → 提取累计 token 用量，计算 delta

use crate::codex_config::{get_codex_config_dir, read_codex_config_text};
use crate::codex_state_db::codex_state_db_paths;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{
    local_usage_date, RelationClaim, RelationConfidence, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::session_usage_pipeline::{
    has_canonical_coverage_on_conn, publish_canonical_batch_on_conn,
    reserve_canonical_coverage_on_conn, CanonicalUsageBatch, RawUsageLogRow, UsageObservation,
    UsagePublishTarget as CodexStorage, UsageSourceSpec,
};
use crate::services::usage_stats::{
    find_matching_proxy_usage_log, find_matching_proxy_usage_log_for_coverage_source,
    find_model_pricing, has_proxy_request_id, has_suspected_codex_session_duplicate,
    should_skip_session_insert, DedupKey, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
};

const CODEX_THREAD_REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";
const CODEX_REPLAY_STATE_KEY: &str = "codex_usage_canonical_replay_v1";
const CODEX_REPLAY_PENDING: &str = "pending";
const CODEX_REPLAYING: &str = "replaying";
const CODEX_REPLAY_COMPLETE: &str = "complete";
fn has_codex_storage_coverage_on_conn(
    conn: &Connection,
    storage: CodexStorage,
    source: &str,
    request_id: &str,
) -> Result<bool, AppError> {
    has_canonical_coverage_on_conn(conn, storage, "codex", source, request_id)
}

fn has_codex_storage_session_log_on_conn(
    conn: &Connection,
    storage: CodexStorage,
    request_id: &str,
) -> Result<bool, AppError> {
    if storage == CodexStorage::Published {
        return has_proxy_request_id(conn, request_id);
    }
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE request_id = ?1)",
        storage.session_log_table()
    );
    conn.query_row(&sql, [request_id], |row| row.get(0))
        .map_err(|error| AppError::Database(format!("读取 Codex 重放会话明细失败: {error}")))
}

/// Reserve a proxy request for this Codex canonical event immediately in the
/// caller-owned transaction.  The matching SQL excludes rows with a proxy
/// coverage marker, so writing the reservation before moving to the next event
/// makes duplicate fingerprints in one batch claim distinct proxy rows (or
/// fall back to a native compatibility row when no proxy row remains).
///
/// This intentionally runs on the same transaction as the canonical fact.  A
/// later fact/coverage/cursor failure therefore rolls the reservation back with
/// the rest of the batch instead of leaving an orphan marker.
fn reserve_codex_proxy_coverage_on_conn(
    conn: &Connection,
    storage: CodexStorage,
    request_id: &str,
    session_id: &str,
    marked_at: i64,
) -> Result<(), AppError> {
    reserve_canonical_coverage_on_conn(
        conn,
        storage,
        "codex",
        "proxy",
        request_id,
        Some(session_id),
        marked_at,
    )
}

/// 累计 token 用量（跟踪 total_token_usage 字段）
#[derive(Debug, Clone, Default)]
struct CumulativeTokens {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// 单次 API 调用的 token 增量
#[derive(Debug)]
struct DeltaTokens {
    input: u32,
    cached_input: u32,
    output: u32,
}

/// Source-presence-aware components for one token_count event.  The raw
/// compatibility table has no nullable component distinction, so canonical
/// facts carry these options directly from the JSON source instead of
/// treating a missing field as zero.
#[derive(Debug, Clone, Default)]
struct ParsedUsageComponents {
    input: Option<u32>,
    cache_read: Option<u32>,
    output: Option<u32>,
    reasoning: Option<u32>,
}

impl DeltaTokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.cached_input == 0 && self.output == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenCountersSignature {
    input: Option<u64>,
    cached_input: Option<u64>,
    output: Option<u64>,
    reasoning_output: Option<u64>,
    total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenUsageSignature {
    total: Option<TokenCountersSignature>,
    last: Option<TokenCountersSignature>,
}

#[derive(Debug)]
struct TimestampedTokenSignature {
    timestamp: DateTime<Utc>,
    signature: TokenUsageSignature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParentFileStamp {
    modified_nanos: i64,
    size: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume_serial: u64,
    #[cfg(windows)]
    file_id: [u8; 16],
}

impl ParentFileStamp {
    fn from_file(file: &fs::File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        #[cfg(windows)]
        let (volume_serial, file_id) = windows_file_identity(file)?;
        Some(Self {
            modified_nanos: metadata_modified_nanos(&metadata),
            size: metadata.len(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(windows)]
            volume_serial,
            #[cfg(windows)]
            file_id,
        })
    }
}

#[cfg(windows)]
fn windows_file_identity(file: &fs::File) -> Option<(u64, [u8; 16])> {
    let mut information = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle for this call, and `information` is a
    // valid writable FILE_ID_INFO buffer of the size passed to Windows.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            std::ptr::addr_of_mut!(information).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } != 0;
    succeeded.then_some((
        information.VolumeSerialNumber,
        information.FileId.Identifier,
    ))
}

#[derive(Debug)]
struct ParentTokenTimeline {
    events: Vec<TimestampedTokenSignature>,
    max_timestamp: Option<DateTime<Utc>>,
    has_token_without_timestamp: bool,
}

impl ParentTokenTimeline {
    fn parent_file_is_stable_before_cutoff(parent_path: &Path, cutoff: DateTime<Utc>) -> bool {
        let Ok(metadata) = fs::metadata(parent_path) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(duration) = modified.duration_since(SystemTime::UNIX_EPOCH) else {
            return false;
        };
        // A closed parent rollout whose file mtime predates the child fork is
        // complete evidence, even when its last token snapshot happened much
        // earlier.  Requiring a token at the exact fork time would otherwise
        // leave historical forks deferred forever.
        duration.as_secs() < cutoff.timestamp().max(0) as u64
    }

    fn signatures_before(
        &self,
        parent_path: &Path,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<TokenUsageSignature>, String> {
        if self.has_token_without_timestamp {
            return Err(format!(
                "父 rollout {} 的 token_count 缺少有效 timestamp",
                parent_path.display()
            ));
        }
        if self
            .max_timestamp
            .is_none_or(|timestamp| timestamp < cutoff)
            && !Self::parent_file_is_stable_before_cutoff(parent_path, cutoff)
        {
            return Err(format!(
                "父 rollout {} 尚未写到 child fork 时刻",
                parent_path.display()
            ));
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.timestamp <= cutoff)
            .map(|event| event.signature.clone())
            .collect())
    }
}

#[derive(Debug)]
struct CachedParentTimeline {
    stamp: ParentFileStamp,
    timeline: Arc<ParentTokenTimeline>,
}

#[derive(Debug)]
struct CachedReplayPrefix {
    modified: i64,
    size: u64,
    prefix: usize,
}

#[derive(Debug)]
struct ParsedTokenEvent {
    line_offset: i64,
    signature: TokenUsageSignature,
    delta: DeltaTokens,
    event_index: Option<u32>,
    model: String,
    timestamp: Option<String>,
    precision: UsagePrecision,
    source_components: ParsedUsageComponents,
}

#[derive(Debug)]
enum ParentResolution {
    None,
    Parent(String),
    Deferred(String),
}

#[derive(Debug)]
struct ParsedCodexFile {
    root_thread_id: Option<String>,
    root_meta_seen: bool,
    root_timestamp: Option<DateTime<Utc>>,
    project_dir: Option<String>,
    parent: ParentResolution,
    token_events: Vec<ParsedTokenEvent>,
    line_offset: i64,
    has_billable_tokens: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingReason {
    MissingParent(String),
    Stable(String),
    Retryable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingEntry {
    modified: i64,
    size: u64,
    reason: PendingReason,
}

#[derive(Debug, Default)]
struct CodexReplayCaches {
    parent_timelines: HashMap<PathBuf, CachedParentTimeline>,
    replay_prefixes: HashMap<PathBuf, CachedReplayPrefix>,
    pending: HashMap<PathBuf, PendingEntry>,
    request_precisions: HashMap<String, UsagePrecision>,
}

static CODEX_REPLAY_CACHES: OnceLock<Mutex<CodexReplayCaches>> = OnceLock::new();

fn replay_caches() -> &'static Mutex<CodexReplayCaches> {
    CODEX_REPLAY_CACHES.get_or_init(|| Mutex::new(CodexReplayCaches::default()))
}

fn remember_request_precision(request_id: &str, precision: UsagePrecision) {
    if let Ok(mut caches) = replay_caches().lock() {
        caches
            .request_precisions
            .insert(request_id.to_string(), precision);
    }
}

pub(crate) fn clear_codex_replay_caches() {
    if let Ok(mut caches) = replay_caches().lock() {
        *caches = CodexReplayCaches::default();
    }
}

fn is_rollout_filename(file_name: &str) -> bool {
    if !file_name.starts_with("rollout-") || !file_name.ends_with(".jsonl") {
        return false;
    }
    let stem = file_name.trim_end_matches(".jsonl");
    stem.get(stem.len().saturating_sub(36)..)
        .is_some_and(|candidate| uuid::Uuid::parse_str(candidate).is_ok())
}

fn is_codex_cursor_path(file_path: &str, codex_dir: &Path) -> bool {
    let path = Path::new(file_path);
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or_default();
    if !is_rollout_filename(file_name) {
        return false;
    }

    if path.starts_with(codex_dir.join("sessions"))
        || path.starts_with(codex_dir.join("archived_sessions"))
    {
        return true;
    }

    // 兼容用户改过 CODEX_HOME 后遗留、且源文件已不存在的 cursor。只接受
    // 明确目录段 + Codex rollout UUID 文件名，避免宽 codex_dir 误删其他 importer。
    file_path
        .replace('\\', "/")
        .split('/')
        .any(|segment| matches!(segment, "sessions" | "archived_sessions"))
}

pub(crate) fn reset_codex_usage_on_conn(
    conn: &rusqlite::Connection,
    codex_dir: &Path,
) -> Result<(), AppError> {
    if Database::table_exists(conn, "proxy_request_logs")?
        && Database::has_column(conn, "proxy_request_logs", "data_source")?
    {
        conn.execute(
            "DELETE FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 会话明细失败: {error}")))?;
    }
    if Database::table_exists(conn, "usage_daily_rollups")?
        && Database::has_column(conn, "usage_daily_rollups", "provider_id")?
    {
        conn.execute(
            "DELETE FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 用量汇总失败: {error}")))?;
    }
    if Database::table_exists(conn, "session_log_sync")?
        && Database::has_column(conn, "session_log_sync", "file_path")?
    {
        let paths = {
            let mut statement = conn
                .prepare("SELECT file_path FROM session_log_sync")
                .map_err(|error| {
                    AppError::Database(format!("读取会话同步 cursor 失败: {error}"))
                })?;
            let paths = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| AppError::Database(format!("查询会话同步 cursor 失败: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    AppError::Database(format!("解析会话同步 cursor 失败: {error}"))
                })?;
            paths
        };
        for file_path in paths
            .into_iter()
            .filter(|path| is_codex_cursor_path(path, codex_dir))
        {
            conn.execute(
                "DELETE FROM session_log_sync WHERE file_path = ?1",
                [file_path],
            )
            .map_err(|error| AppError::Database(format!("清理 Codex 同步 cursor 失败: {error}")))?;
        }
    }
    Ok(())
}

fn codex_replay_state_on_conn(conn: &Connection) -> Result<String, AppError> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [CODEX_REPLAY_STATE_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|value| value.unwrap_or_else(|| CODEX_REPLAY_COMPLETE.to_string()))
    .map_err(|error| AppError::Database(format!("读取 Codex replay 状态失败: {error}")))
}

fn set_codex_replay_state_on_conn(conn: &Connection, state: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![CODEX_REPLAY_STATE_KEY, state],
    )
    .map_err(|error| AppError::Database(format!("写入 Codex 重放状态失败: {error}")))?;
    Ok(())
}

pub(crate) fn codex_replay_in_progress_on_conn(conn: &Connection) -> bool {
    codex_replay_state_on_conn(conn)
        .map(|state| matches!(state.as_str(), CODEX_REPLAY_PENDING | CODEX_REPLAYING))
        .unwrap_or(false)
}

fn readable_codex_files(codex_dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let files = collect_codex_session_files(codex_dir);
    if files.is_empty() {
        return Err(AppError::Config(
            "没有找到可用于 Codex 用量重放的 rollout 文件".into(),
        ));
    }
    for path in &files {
        fs::File::open(path).map_err(|error| {
            AppError::Config(format!(
                "无法读取 Codex rollout {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(files)
}

fn clear_codex_replay_stage_on_conn(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "DELETE FROM codex_replay_nodes;
         DELETE FROM codex_replay_rollups;
         DELETE FROM codex_replay_coverage;
         DELETE FROM codex_replay_session_logs;
         DELETE FROM codex_replay_sync;",
    )
    .map_err(|error| AppError::Database(format!("清理 Codex 重放影子数据失败: {error}")))
}

fn reset_codex_usage_and_mark_replaying(db: &Database) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    conn.execute("SAVEPOINT reset_codex_usage_replay", [])
        .map_err(|error| AppError::Database(format!("开启 Codex 重放事务失败: {error}")))?;
    let result = (|| {
        clear_codex_replay_stage_on_conn(&conn)?;
        set_codex_replay_state_on_conn(&conn, CODEX_REPLAYING)
    })();
    match result {
        Ok(()) => {
            conn.execute("RELEASE reset_codex_usage_replay", [])
                .map_err(|error| AppError::Database(format!("提交 Codex 重放事务失败: {error}")))?;
            drop(conn);
            clear_codex_replay_caches();
            Ok(())
        }
        Err(error) => {
            conn.execute("ROLLBACK TO reset_codex_usage_replay", [])
                .ok();
            conn.execute("RELEASE reset_codex_usage_replay", []).ok();
            Err(error)
        }
    }
}

fn publish_codex_replay_on_conn(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "UPDATE codex_replay_nodes SET app_type = 'codex';
         UPDATE codex_replay_rollups SET app_type = 'codex';
         UPDATE codex_replay_coverage
         SET app_type = 'codex',
             data_source = CASE data_source
                 WHEN 'codex_session_replay' THEN 'codex_session'
                 WHEN 'proxy_replay' THEN 'proxy'
                 ELSE data_source
             END;
         DELETE FROM agent_session_usage_rollups WHERE app_type = 'codex';
         DELETE FROM agent_session_nodes WHERE app_type = 'codex';
         DELETE FROM agent_session_canonical_coverage
         WHERE app_type = 'codex' AND data_source IN ('codex_session', 'proxy');
         INSERT OR REPLACE INTO agent_session_nodes
         SELECT * FROM codex_replay_nodes;
         INSERT OR REPLACE INTO agent_session_usage_rollups
         SELECT * FROM codex_replay_rollups;
         INSERT OR REPLACE INTO agent_session_canonical_coverage
         SELECT * FROM codex_replay_coverage;",
    )
    .map_err(|error| AppError::Database(format!("发布 Codex 重放数据失败: {error}")))?;
    clear_codex_replay_stage_on_conn(conn)
}

fn codex_replay_state(db: &Database) -> Result<String, AppError> {
    let conn = lock_conn!(db.conn);
    codex_replay_state_on_conn(&conn)
}

fn finish_codex_replay_if_ready(db: &Database, result: &SessionSyncResult) -> Result<(), AppError> {
    if result.files_scanned == 0 || !result.errors.is_empty() || result.deferred_files != 0 {
        return Ok(());
    }
    let conn = lock_conn!(db.conn);
    // A replay is eligible when the scan is complete and it produced at least
    // one durable, parsed session identity.  Rollups are intentionally not a
    // requirement: valid metadata-only/zero-usage rollouts still need to
    // publish their node generation and transition out of `replaying`.
    let valid_identity_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM codex_replay_nodes
         WHERE app_type = 'codex_replay' AND TRIM(COALESCE(session_id, '')) <> ''",
        [],
        |row| row.get(0),
    )?;
    if valid_identity_count == 0 {
        return Ok(());
    }
    conn.execute("SAVEPOINT publish_codex_replay", [])
        .map_err(|error| AppError::Database(format!("开启 Codex 重放发布事务失败: {error}")))?;
    let publish_result = (|| {
        publish_codex_replay_on_conn(&conn)?;
        set_codex_replay_state_on_conn(&conn, CODEX_REPLAY_COMPLETE)
    })();
    match publish_result {
        Ok(()) => conn
            .execute("RELEASE publish_codex_replay", [])
            .map_err(|error| AppError::Database(format!("提交 Codex 重放发布事务失败: {error}")))
            .map(|_| ())?,
        Err(error) => {
            conn.execute("ROLLBACK TO publish_codex_replay", []).ok();
            conn.execute("RELEASE publish_codex_replay", []).ok();
            return Err(error);
        }
    }
    Ok(())
}

/// Sync Codex usage and perform a guarded one-time canonical replay when the
/// schema migration requested it.
pub fn sync_codex_usage_with_replay(db: &Database) -> Result<SessionSyncResult, AppError> {
    let state = codex_replay_state(db)?;
    if state == CODEX_REPLAY_PENDING {
        let codex_dir = get_codex_config_dir();
        readable_codex_files(&codex_dir)?;
        db.backup_database_file()?;
        reset_codex_usage_and_mark_replaying(db)?;
    }

    let result = if state == CODEX_REPLAY_COMPLETE {
        sync_codex_usage(db)?
    } else {
        sync_codex_usage_to_storage(db, CodexStorage::CodexReplay)?
    };
    if state == CODEX_REPLAY_PENDING || state == CODEX_REPLAYING {
        finish_codex_replay_if_ready(db, &result)?;
    }
    Ok(result)
}

/// Source-only preflight used by the provider-scoped rebuild command.  It is
/// intentionally read-only and does not touch replay state or the database.
pub(crate) fn preflight_codex_usage() -> Result<(), AppError> {
    let codex_dir = get_codex_config_dir();
    readable_codex_files(&codex_dir).map(|_| ())
}

fn non_empty_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn thread_id_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    uuid::Uuid::parse_str(candidate)
        .ok()
        .map(|value| value.hyphenated().to_string())
}

fn explicit_parent_from_meta(payload: &serde_json::Value) -> ParentResolution {
    let forked_from = non_empty_string(payload.get("forked_from_id"));
    let spawned_from = payload
        .get("source")
        .and_then(|source| source.get("subagent"))
        .and_then(|subagent| subagent.get("thread_spawn"))
        .and_then(|spawn| non_empty_string(spawn.get("parent_thread_id")));

    match (forked_from, spawned_from) {
        (None, None) => ParentResolution::None,
        (Some(parent), None) | (None, Some(parent)) => ParentResolution::Parent(parent),
        (Some(forked), Some(spawned)) if forked == spawned => ParentResolution::Parent(forked),
        (Some(forked), Some(spawned)) => ParentResolution::Deferred(format!(
            "forked_from_id ({forked}) 与 thread_spawn.parent_thread_id ({spawned}) 不一致"
        )),
    }
}

fn parse_timestamp(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn parse_signature_counters(value: Option<&serde_json::Value>) -> Option<TokenCountersSignature> {
    let value = value?.as_object()?;
    Some(TokenCountersSignature {
        input: value
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64),
        cached_input: value
            .get("cached_input_tokens")
            .or_else(|| value.get("cache_read_input_tokens"))
            .and_then(serde_json::Value::as_u64),
        output: value
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64),
        reasoning_output: value
            .get("reasoning_output_tokens")
            .and_then(serde_json::Value::as_u64),
        total: value
            .get("total_tokens")
            .and_then(serde_json::Value::as_u64),
    })
}

fn parse_token_signature(info: &serde_json::Value) -> Option<TokenUsageSignature> {
    let total = parse_signature_counters(info.get("total_token_usage"));
    let last = parse_signature_counters(info.get("last_token_usage"));
    (total.is_some() || last.is_some()).then_some(TokenUsageSignature { total, last })
}

fn token_snapshot_source(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("rate_limits")
        .and_then(|rate_limits| rate_limits.get("limit_id"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 单个同步 pass 的共享状态。
///
/// - `cursors`：pass 开始时一次性预载的 `session_log_sync` 快照，替代逐文件
///   SELECT（尤其是 archived 继承的 `substr` 后缀匹配无法走索引，逐文件跑等于
///   每 pass 全表扫 N 次）。快照语义：同 pass 内其他文件刚写入的游标对后续
///   archived 继承不可见——影响仅是多一轮由 request_id 去重兜底的重扫，
///   不丢数据、不双算。
/// - `pricing`：模型定价 pass 级缓存。定价表在 pass 进行中被修改时本 pass
///   仍用旧价，下一个同步 pass 生效。
struct CodexSyncPass {
    cursors: HashMap<String, (i64, i64)>,
    pricing: HashMap<String, Option<ModelPricing>>,
}

impl CodexSyncPass {
    fn load(db: &Database, storage: CodexStorage) -> Result<Self, AppError> {
        let conn = lock_conn!(db.conn);
        let sql = format!(
            "SELECT file_path, last_modified, last_line_offset FROM {}",
            storage.cursor_table()
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("预载同步游标失败: {e}")))?;
        let cursors = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
                ))
            })
            .and_then(|rows| rows.collect::<Result<HashMap<_, _>, _>>())
            .map_err(|e| AppError::Database(format!("预载同步游标失败: {e}")))?;
        Ok(Self {
            cursors,
            pricing: HashMap::new(),
        })
    }
}

fn get_codex_sync_state(
    db: &Database,
    file_path: &Path,
    cursors: &HashMap<String, (i64, i64)>,
    storage: CodexStorage,
) -> Result<(i64, i64), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let state = cursors.get(&file_path_str).copied().unwrap_or((0, 0));
    if state != (0, 0)
        || file_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some("archived_sessions")
    {
        return Ok(state);
    }

    let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
        return Ok(state);
    };
    let slash_suffix = format!("/{file_name}");
    let backslash_suffix = format!("\\{file_name}");
    // 与原 SQL 等价：ORDER BY last_line_offset DESC, last_modified DESC LIMIT 1
    // → 在快照上按 (offset, modified) 取最大。
    let inherited = cursors
        .iter()
        .filter(|(path, _)| {
            path.as_str() != file_path_str
                && (path.ends_with(&slash_suffix) || path.ends_with(&backslash_suffix))
        })
        .map(|(_, &(modified, offset))| (offset, modified))
        .max();

    match inherited {
        Some((offset, modified)) => {
            update_codex_sync_state(db, &file_path_str, modified, offset, storage)?;
            Ok((modified, offset))
        }
        None => Ok(state),
    }
}

fn update_codex_sync_state(
    db: &Database,
    file_path: &str,
    modified: i64,
    offset: i64,
    storage: CodexStorage,
) -> Result<(), AppError> {
    if storage == CodexStorage::Published {
        return update_sync_state(db, file_path, modified, offset);
    }
    let conn = lock_conn!(db.conn);
    let sql = format!(
        "INSERT INTO {} (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(file_path) DO UPDATE SET
             last_modified = excluded.last_modified,
             last_line_offset = excluded.last_line_offset,
             last_synced_at = excluded.last_synced_at",
        storage.cursor_table()
    );
    conn.execute(&sql, rusqlite::params![file_path, modified, offset])
        .map_err(|error| AppError::Database(format!("更新 Codex 重放 cursor 失败: {error}")))?;
    Ok(())
}

fn update_codex_sync_state_on_conn(
    conn: &Connection,
    file_path: &str,
    modified: i64,
    offset: i64,
    storage: CodexStorage,
) -> Result<(), AppError> {
    if storage == CodexStorage::Published {
        return update_sync_state_on_conn(conn, file_path, modified, offset);
    }
    let sql = format!(
        "INSERT INTO {} (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(file_path) DO UPDATE SET
             last_modified = excluded.last_modified,
             last_line_offset = excluded.last_line_offset,
             last_synced_at = excluded.last_synced_at",
        storage.cursor_table()
    );
    conn.execute(&sql, rusqlite::params![file_path, modified, offset])
        .map_err(|error| AppError::Database(format!("写入 Codex 重放 cursor 失败: {error}")))?;
    Ok(())
}

/// 归一化 Codex 模型名
///
/// 处理规则（按顺序）：
/// 1. 转小写：`GLM-4.6` → `glm-4.6`
/// 2. 剥离 provider 前缀：`openai/gpt-5.4` → `gpt-5.4`
/// 3. 剥离 ISO 日期后缀：`gpt-5.4-2026-03-05` → `gpt-5.4`
/// 4. 剥离紧凑日期后缀：`gpt-5.4-20260305` → `gpt-5.4`
fn normalize_codex_model(raw: &str) -> String {
    // Step 1: 小写
    let mut name = raw.to_lowercase();

    // Step 2: 剥离 "provider/" 前缀（如 openai/, azure/）
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }

    // Step 3: 剥离 ISO 日期后缀 -YYYY-MM-DD（正好 11 字符）
    if name.len() > 11 && name.is_char_boundary(name.len() - 11) {
        let suffix = &name[name.len() - 11..];
        if suffix.is_ascii()
            && suffix.as_bytes()[0] == b'-'
            && suffix[1..5].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[5] == b'-'
            && suffix[6..8].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[8] == b'-'
            && suffix[9..11].chars().all(|c| c.is_ascii_digit())
        {
            name.truncate(name.len() - 11);
        }
    }

    // Step 4: 剥离紧凑日期后缀 -YYYYMMDD（正好 9 字符）
    if name.len() > 9 {
        let parts: Vec<&str> = name.rsplitn(2, '-').collect();
        if parts.len() == 2 {
            if let Some(suffix) = parts.first() {
                if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
                    name = parts[1].to_string();
                }
            }
        }
    }

    name
}

/// 计算两次累计值之间的 delta
fn compute_delta(prev: &Option<CumulativeTokens>, current: &CumulativeTokens) -> DeltaTokens {
    match prev {
        None => DeltaTokens {
            input: current.input as u32,
            cached_input: current.cached_input as u32,
            output: current.output as u32,
        },
        Some(p) => DeltaTokens {
            input: current.input.saturating_sub(p.input) as u32,
            cached_input: current.cached_input.saturating_sub(p.cached_input) as u32,
            output: current.output.saturating_sub(p.output) as u32,
        },
    }
}

fn update_high_water(high_water: &mut CumulativeTokens, current: &CumulativeTokens) {
    high_water.input = high_water.input.max(current.input);
    high_water.cached_input = high_water.cached_input.max(current.cached_input);
    high_water.output = high_water.output.max(current.output);
}

fn update_presence_high_water(
    high_water: &mut ParsedUsageComponents,
    current: &ParsedUsageComponents,
) {
    fn update(high_water: &mut Option<u32>, current: Option<u32>) {
        if let Some(current) = current {
            *high_water = Some(high_water.unwrap_or(0).max(current));
        }
    }
    update(&mut high_water.input, current.input);
    update(&mut high_water.cache_read, current.cache_read);
    update(&mut high_water.output, current.output);
    update(&mut high_water.reasoning, current.reasoning);
}

/// Parse a cumulative token snapshot while retaining which source fields were
/// actually present so missing cached-input/cache-read is never coerced to 0.
fn parse_cumulative_components(
    total_usage: &serde_json::Value,
) -> Option<(CumulativeTokens, ParsedUsageComponents)> {
    let fields = total_usage.as_object()?;
    if ![
        "input_tokens",
        "cached_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ]
    .iter()
    .any(|field| fields.contains_key(*field))
    {
        return None;
    }
    let input = total_usage
        .get("input_tokens")
        .and_then(serde_json::Value::as_u64);
    let cache_read = total_usage
        .get("cached_input_tokens")
        .or_else(|| total_usage.get("cache_read_input_tokens"))
        .and_then(serde_json::Value::as_u64);
    let output = total_usage
        .get("output_tokens")
        .and_then(serde_json::Value::as_u64);
    let reasoning = total_usage
        .get("reasoning_output_tokens")
        .and_then(serde_json::Value::as_u64);
    Some((
        CumulativeTokens {
            input: input.unwrap_or(0),
            cached_input: cache_read.unwrap_or(0),
            output: output.unwrap_or(0),
        },
        ParsedUsageComponents {
            input: input.map(|value| value.min(u32::MAX as u64) as u32),
            cache_read: cache_read.map(|value| value.min(u32::MAX as u64) as u32),
            output: output.map(|value| value.min(u32::MAX as u64) as u32),
            reasoning: reasoning.map(|value| value.min(u32::MAX as u64) as u32),
        },
    ))
}

fn cumulative_component_delta(
    previous: Option<&ParsedUsageComponents>,
    current: &ParsedUsageComponents,
) -> ParsedUsageComponents {
    fn delta(
        previous: Option<&ParsedUsageComponents>,
        field: fn(&ParsedUsageComponents) -> Option<u32>,
        current: Option<u32>,
    ) -> Option<u32> {
        match (previous, current) {
            (None, current) => current,
            (Some(previous), Some(current)) => {
                field(previous).map(|previous| current.saturating_sub(previous))
            }
            (Some(_), None) => None,
        }
    }
    ParsedUsageComponents {
        input: delta(previous, |value| value.input, current.input),
        cache_read: delta(previous, |value| value.cache_read, current.cache_read),
        output: delta(previous, |value| value.output, current.output),
        reasoning: delta(previous, |value| value.reasoning, current.reasoning),
    }
}

type RolloutIndex = HashMap<String, Vec<PathBuf>>;

#[derive(Debug, Default)]
struct CodexFileSyncResult {
    imported: u32,
    skipped: u32,
    suspected_duplicates: u32,
    deferred: bool,
}

/// 同步 Codex 使用数据（从 JSONL 会话日志）
pub fn sync_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_codex_usage_to_storage(db, CodexStorage::Published)
}

fn sync_codex_usage_to_storage(
    db: &Database,
    storage: CodexStorage,
) -> Result<SessionSyncResult, AppError> {
    let codex_dir = get_codex_config_dir();
    let files = collect_codex_session_files(&codex_dir);
    let thread_titles = load_native_thread_titles();
    let rollout_index = build_rollout_index(&files);
    let mut pass = CodexSyncPass::load(db, storage)?;

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    // Normalize every discovered rollout relation in one graph pass before
    // importing usage.  This is what lets root → child → grandchild resolve
    // without ever folding a child's own ID into its parent's node.
    if let Err(error) =
        persist_codex_nodes_for_files_with_titles_to_storage(db, &files, &thread_titles, storage)
    {
        result
            .errors
            .push(format!("Codex 会话节点写入失败: {error}"));
    }

    for file_path in &files {
        match sync_single_codex_file(db, file_path, &rollout_index, &mut pass, storage) {
            Ok(file_result) => {
                result.imported = result.imported.saturating_add(file_result.imported);
                result.skipped = result.skipped.saturating_add(file_result.skipped);
                result.suspected_duplicates = result
                    .suspected_duplicates
                    .saturating_add(file_result.suspected_duplicates);
                if file_result.deferred {
                    result.deferred_files = result.deferred_files.saturating_add(1);
                }
            }
            Err(e) => {
                let msg = format!("Codex 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[CODEX-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    // Single-file compatibility writes above intentionally fail closed when a
    // parent claim is not in that call's scope.  Re-run the complete graph
    // after ingestion so the public sync leaves every discovered depth with
    // its normalized root/parent ownership.
    if let Err(error) =
        persist_codex_nodes_for_files_with_titles_to_storage(db, &files, &thread_titles, storage)
    {
        result
            .errors
            .push(format!("Codex 会话节点归一化失败: {error}"));
    }

    if result.imported > 0 || result.deferred_files > 0 {
        log::info!(
            "[CODEX-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, deferred {} 个, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.deferred_files,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Codex 会话 JSONL 文件
fn collect_codex_session_files(codex_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // 1. 扫描 sessions/YYYY/MM/DD/*.jsonl（日期分区目录）
    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.is_dir() {
        collect_jsonl_recursive(&sessions_dir, &mut files, 0, 3);
    }

    // 2. 扫描 archived_sessions/*.jsonl（扁平归档目录）
    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&archived_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    files.push(path);
                }
            }
        }
    }

    files.sort();
    files
}

fn build_rollout_index(files: &[PathBuf]) -> RolloutIndex {
    let mut index = RolloutIndex::new();
    for path in files {
        if let Some(thread_id) = thread_id_from_filename(path) {
            index.entry(thread_id).or_default().push(path.clone());
        }
    }
    for paths in index.values_mut() {
        paths.sort();
    }
    index
}

/// 递归扫描目录下的 .jsonl 文件（限制最大深度）
fn collect_jsonl_recursive(dir: &Path, files: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_jsonl_recursive(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn parse_codex_file(
    file_path: &Path,
    root_thread_id: Option<String>,
) -> Result<ParsedCodexFile, AppError> {
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);
    let mut root_meta_seen = false;
    let mut root_timestamp = None;
    let mut project_dir = None;
    let mut parent = ParentResolution::None;
    let mut current_model = "unknown".to_string();
    // `total_token_usage` is session-cumulative, including across model and
    // rate-limit bucket changes. Divergent snapshots are handled by preferring
    // exact `last_token_usage`, not by splitting the cumulative baseline.
    let mut total_high_water = None;
    // Rate-limit refreshes can re-emit unchanged token info under another
    // `limit_id`. Same-source repeats are identified by that source's latest
    // full snapshot; cross-source repeats must match the immediately preceding
    // token event. Do not compare against other sources' older snapshots:
    // those stale signatures can legitimately recur after a counter reset.
    let mut last_signature_by_source: HashMap<Option<String>, TokenUsageSignature> = HashMap::new();
    let mut previous_token_signature = None;
    let mut total_presence_high_water: Option<ParsedUsageComponents> = None;
    let mut event_index = 0u32;
    let mut token_events = Vec::new();
    let mut line_offset = 0i64;
    let mut has_billable_tokens = false;

    for line_result in reader.lines() {
        line_offset += 1;
        let line = match line_result {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let is_event_msg = line.contains("\"event_msg\"");
        let is_turn_context = line.contains("\"turn_context\"");
        let is_session_meta = line.contains("\"session_meta\"");
        if !is_event_msg && !is_turn_context && !is_session_meta {
            continue;
        }
        if is_event_msg && !line.contains("\"token_count\"") {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(event_type) = value.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };

        match event_type {
            "session_meta" if !root_meta_seen => {
                root_meta_seen = true;
                root_timestamp = parse_timestamp(value.get("timestamp"));
                let payload = value.get("payload").unwrap_or(&serde_json::Value::Null);
                project_dir = non_empty_string(payload.get("cwd"));
                parent = explicit_parent_from_meta(payload);

                let meta_thread_id = non_empty_string(
                    payload
                        .get("id")
                        .or_else(|| payload.get("thread_id"))
                        .or_else(|| payload.get("threadId")),
                );
                if let (Some(filename_id), Some(meta_id)) = (&root_thread_id, meta_thread_id) {
                    if filename_id != &meta_id {
                        parent = ParentResolution::Deferred(format!(
                            "文件名线程 ID ({filename_id}) 与 root meta ID ({meta_id}) 不一致"
                        ));
                    }
                }

                if let ParentResolution::Parent(parent_id) = &mut parent {
                    match uuid::Uuid::parse_str(parent_id) {
                        Ok(value) => *parent_id = value.hyphenated().to_string(),
                        Err(_) => {
                            parent = ParentResolution::Deferred(format!(
                                "显式 parent_thread_id 不是有效 UUID: {parent_id}"
                            ));
                        }
                    }
                }
                if matches!((&root_thread_id, &parent), (Some(root), ParentResolution::Parent(parent_id)) if root == parent_id)
                {
                    parent = ParentResolution::Deferred(
                        "parent_thread_id 与 root_thread_id 相同".to_string(),
                    );
                }
            }
            "turn_context" => {
                if let Some(payload) = value.get("payload") {
                    if let Some(model) = payload
                        .get("model")
                        .or_else(|| payload.get("info").and_then(|info| info.get("model")))
                        .and_then(serde_json::Value::as_str)
                    {
                        current_model = normalize_codex_model(model);
                    }
                }
            }
            "event_msg" => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(serde_json::Value::as_str) != Some("token_count") {
                    continue;
                }
                let Some(info) = payload.get("info").filter(|info| !info.is_null()) else {
                    continue;
                };
                let Some(signature) = parse_token_signature(info) else {
                    continue;
                };

                if let Some(model) = info
                    .get("model")
                    .or_else(|| info.get("model_name"))
                    .or_else(|| payload.get("model"))
                    .and_then(serde_json::Value::as_str)
                {
                    current_model = normalize_codex_model(model);
                }

                let snapshot_source = token_snapshot_source(payload);
                let total_snapshot = info
                    .get("total_token_usage")
                    .and_then(parse_cumulative_components);
                let last_snapshot = info
                    .get("last_token_usage")
                    .and_then(parse_cumulative_components);
                if total_snapshot.is_none() && last_snapshot.is_none() {
                    continue;
                }
                let has_total_snapshot = total_snapshot.is_some();
                let has_exact_last_usage = last_snapshot.is_some();
                let duplicate_snapshot = has_total_snapshot
                    && (last_signature_by_source.get(&snapshot_source) == Some(&signature)
                        || previous_token_signature.as_ref() == Some(&signature));
                if has_total_snapshot {
                    last_signature_by_source.insert(snapshot_source, signature.clone());
                }
                previous_token_signature = Some(signature.clone());

                let (delta, source_components) = if duplicate_snapshot {
                    (
                        DeltaTokens {
                            input: 0,
                            cached_input: 0,
                            output: 0,
                        },
                        ParsedUsageComponents::default(),
                    )
                } else if let Some((last, components)) = last_snapshot {
                    // Codex provides the exact per-request usage. Prefer it to
                    // subtracting cumulative snapshots, which may come from
                    // multiple independently advancing rate-limit lanes.
                    (
                        DeltaTokens {
                            input: last.input as u32,
                            cached_input: last.cached_input as u32,
                            output: last.output as u32,
                        },
                        components,
                    )
                } else if let Some((total, components)) = total_snapshot.as_ref() {
                    (
                        compute_delta(&total_high_water, total),
                        cumulative_component_delta(total_presence_high_water.as_ref(), components),
                    )
                } else {
                    continue;
                };
                if let Some((total, components)) = total_snapshot {
                    if let Some(high_water) = total_high_water.as_mut() {
                        update_high_water(high_water, &total);
                    } else {
                        total_high_water = Some(total);
                    }
                    if let Some(high_water) = total_presence_high_water.as_mut() {
                        update_presence_high_water(high_water, &components);
                    } else {
                        total_presence_high_water = Some(components);
                    }
                }
                let delta = DeltaTokens {
                    cached_input: delta.cached_input.min(delta.input),
                    ..delta
                };
                let precision = if has_exact_last_usage {
                    UsagePrecision::RequestExact
                } else {
                    UsagePrecision::SessionExact
                };
                let nonzero_index = if delta.is_zero()
                    && source_components
                        .reasoning
                        .is_none_or(|reasoning| reasoning == 0)
                {
                    None
                } else {
                    has_billable_tokens = true;
                    event_index = event_index.saturating_add(1);
                    Some(event_index)
                };

                token_events.push(ParsedTokenEvent {
                    line_offset,
                    signature,
                    delta,
                    event_index: nonzero_index,
                    model: current_model.clone(),
                    timestamp: value
                        .get("timestamp")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    precision,
                    source_components,
                });
            }
            _ => {}
        }
    }

    Ok(ParsedCodexFile {
        root_thread_id,
        root_meta_seen,
        root_timestamp,
        project_dir,
        parent,
        token_events,
        line_offset,
        has_billable_tokens,
    })
}

fn event_timestamp_epoch(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.timestamp())
}

/// Read the titles Codex itself exposes in the desktop sidebar. The state
/// database is authoritative when available; the legacy session index fills
/// gaps. The query intentionally selects only `id` and `title`, so no prompt
/// body or `first_user_message` value is read or persisted.
fn load_native_thread_titles() -> HashMap<String, String> {
    let config_dir = get_codex_config_dir();
    let mut titles = load_native_thread_titles_from_index(&config_dir.join("session_index.jsonl"));
    let config_text = read_codex_config_text().unwrap_or_default();
    for db_path in codex_state_db_paths(&config_dir, &config_text) {
        titles.extend(load_native_thread_titles_from_db(&db_path));
    }
    titles
}

fn load_native_thread_titles_from_index(path: &Path) -> HashMap<String, String> {
    let Ok(file) = fs::File::open(path) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(id) = non_empty_string(value.get("id")) else {
            continue;
        };
        let Some(title) = non_empty_string(value.get("thread_name")) else {
            continue;
        };
        titles.insert(id, title);
    }
    titles
}

fn load_native_thread_titles_from_db(path: &Path) -> HashMap<String, String> {
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(conn) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return HashMap::new();
    };
    if conn.busy_timeout(Duration::from_secs(2)).is_err() {
        return HashMap::new();
    }
    let Ok(mut statement) =
        conn.prepare("SELECT id, title FROM threads WHERE TRIM(COALESCE(title, '')) <> ''")
    else {
        return HashMap::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        Ok((id, title))
    }) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for (id, title) in rows.flatten() {
        let id = id.trim();
        let title = title.trim();
        if !id.is_empty() && !title.is_empty() {
            titles.insert(id.to_string(), title.to_string());
        }
    }
    titles
}

/// Turn one parsed rollout into the only relation evidence accepted by the
/// normalized service.  The filename UUID is always the node's own identity;
/// parentage can only come from the two explicit metadata fields parsed above.
fn relation_claim_from_parsed(
    file_path: &Path,
    parsed: &ParsedCodexFile,
    file_modified: i64,
    thread_titles: &HashMap<String, String>,
) -> Option<SessionRelationClaim> {
    let session_id = parsed.root_thread_id.clone()?;
    if !parsed.root_meta_seen {
        return None;
    }

    let relation = match &parsed.parent {
        ParentResolution::None => RelationClaim::Root,
        ParentResolution::Parent(parent_session_id) => RelationClaim::Parent {
            parent_session_id: parent_session_id.clone(),
            confidence: RelationConfidence::Explicit,
        },
        // RelationClaim has no standalone conflict variant.  A self edge with
        // Conflict confidence is intentionally fail-closed in T03's graph
        // normalizer and cannot become an ownership edge.
        ParentResolution::Deferred(_) => RelationClaim::Parent {
            parent_session_id: session_id.clone(),
            confidence: RelationConfidence::Conflict,
        },
    };

    let last_active_at = parsed
        .token_events
        .iter()
        .filter_map(|event| event_timestamp_epoch(event.timestamp.as_deref()))
        .max();
    let title = thread_titles.get(&session_id).cloned();
    Some(SessionRelationClaim {
        app_type: "codex".to_string(),
        session_id,
        relation,
        metadata: SessionNodeMetadata {
            title,
            project_dir: parsed.project_dir.clone(),
            source_path: Some(file_path.to_string_lossy().to_string()),
            created_at: parsed.root_timestamp.map(|timestamp| timestamp.timestamp()),
            last_active_at,
            last_synced_at: file_modified,
        },
    })
}

fn persist_codex_relation_claims_to_storage(
    db: &Database,
    claims: &[SessionRelationClaim],
    storage: CodexStorage,
) -> Result<(), AppError> {
    if claims.is_empty() {
        return Ok(());
    }
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 Codex 会话节点事务失败: {error}")))?;
    publish_canonical_batch_on_conn(
        &tx,
        storage,
        CanonicalUsageBatch {
            relation_claims: claims.to_vec(),
            ..CanonicalUsageBatch::default()
        },
    )?;
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Codex 会话节点事务失败: {error}")))
}

fn persist_codex_node_for_parsed(
    db: &Database,
    file_path: &Path,
    parsed: &ParsedCodexFile,
    file_modified: i64,
    storage: CodexStorage,
) -> Result<(), AppError> {
    if let Some(claim) =
        relation_claim_from_parsed(file_path, parsed, file_modified, &HashMap::new())
    {
        persist_codex_relation_claims_to_storage(db, &[claim], storage)?;
    }
    Ok(())
}

fn persist_codex_nodes_for_files_with_titles_to_storage(
    db: &Database,
    files: &[PathBuf],
    thread_titles: &HashMap<String, String>,
    storage: CodexStorage,
) -> Result<(), AppError> {
    let mut claims = Vec::new();
    for file_path in files {
        let metadata = match fs::metadata(file_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let parsed = match parse_codex_file(file_path, thread_id_from_filename(file_path)) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if let Some(claim) = relation_claim_from_parsed(
            file_path,
            &parsed,
            metadata_modified_nanos(&metadata),
            thread_titles,
        ) {
            claims.push(claim);
        }
    }
    persist_codex_relation_claims_to_storage(db, &claims, storage)
}

fn codex_fact_from_event(
    request_id: &str,
    session_id: &str,
    event: &ParsedTokenEvent,
) -> Option<UsageObservation> {
    let timestamp = event_timestamp_epoch(event.timestamp.as_deref())?;
    let components = &event.source_components;
    if components.input.is_none()
        && components.cache_read.is_none()
        && components.output.is_none()
        && components.reasoning.is_none()
    {
        return None;
    }
    let date = local_usage_date(timestamp)?;
    let source = UsageSourceSpec::new(
        "codex",
        "_codex_session",
        "codex_session",
        event.precision,
        TimeSemantics::EventTime,
        RequestCountSemantics::AgentCall,
    );
    let mut fact = source.fact(
        date,
        session_id,
        event.model.clone(),
        event.model.clone(),
        String::new(),
    );
    fact.request_count = Some(1);
    fact.input_tokens = components.input.map(i64::from);
    fact.output_tokens = components.output.map(i64::from);
    fact.cache_read_tokens = components.cache_read.map(i64::from);
    fact.reasoning_tokens = components.reasoning.map(i64::from);
    fact.first_event_at = Some(timestamp);
    fact.last_event_at = Some(timestamp);
    Some(UsageObservation {
        request_id: request_id.to_string(),
        fact,
    })
}

fn parent_signatures_before(
    parent_path: &Path,
    cutoff: DateTime<Utc>,
) -> Result<Vec<TokenUsageSignature>, String> {
    let file = fs::File::open(parent_path)
        .map_err(|error| format!("无法打开父 rollout {}: {error}", parent_path.display()))?;
    let stamp = ParentFileStamp::from_file(&file);
    let cached_timeline = stamp.and_then(|stamp| {
        replay_caches().lock().ok().and_then(|caches| {
            caches
                .parent_timelines
                .get(parent_path)
                .filter(|entry| entry.stamp == stamp)
                .map(|entry| Arc::clone(&entry.timeline))
        })
    });
    if let Some(timeline) = cached_timeline {
        return timeline.signatures_before(parent_path, cutoff);
    }

    let mut events = Vec::new();
    let mut max_timestamp: Option<DateTime<Utc>> = None;
    let mut has_token_without_timestamp = false;

    // 必须扫描完整父文件，不能在首个未来时间戳处 break：rollout 写入顺序
    // 不承诺时间戳严格单调。缓存完整时间线后，不同 child cutoff 只需内存过滤。
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let timestamp = parse_timestamp(value.get("timestamp"));
        if let Some(timestamp) = timestamp {
            max_timestamp = Some(max_timestamp.map_or(timestamp, |current| current.max(timestamp)));
        }
        if value.get("type").and_then(serde_json::Value::as_str) != Some("event_msg")
            || value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(serde_json::Value::as_str)
                != Some("token_count")
        {
            continue;
        }
        let Some(info) = value
            .get("payload")
            .and_then(|payload| payload.get("info"))
            .filter(|info| !info.is_null())
        else {
            continue;
        };
        let Some(signature) = parse_token_signature(info) else {
            continue;
        };
        let Some(timestamp) = timestamp else {
            has_token_without_timestamp = true;
            continue;
        };
        events.push(TimestampedTokenSignature {
            timestamp,
            signature,
        });
    }

    let timeline = Arc::new(ParentTokenTimeline {
        events,
        max_timestamp,
        has_token_without_timestamp,
    });
    let result = timeline.signatures_before(parent_path, cutoff);
    if let (Some(stamp), Ok(mut caches)) = (stamp, replay_caches().lock()) {
        caches.parent_timelines.insert(
            parent_path.to_path_buf(),
            CachedParentTimeline {
                stamp,
                timeline: Arc::clone(&timeline),
            },
        );
    }
    result
}

fn resolve_parent_signatures(
    parent_id: &str,
    cutoff: DateTime<Utc>,
    rollout_index: &RolloutIndex,
) -> Result<Vec<TokenUsageSignature>, String> {
    let Some(candidates) = rollout_index.get(parent_id) else {
        return Err(format!("找不到父 rollout: {parent_id}"));
    };

    let mut snapshots = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        snapshots.push(parent_signatures_before(candidate, cutoff)?);
    }
    let Some(first) = snapshots.first() else {
        return Err(format!("找不到父 rollout: {parent_id}"));
    };
    if snapshots.iter().skip(1).any(|snapshot| snapshot != first) {
        return Err(format!(
            "父 rollout UUID {parent_id} 对应多个内容不一致的文件"
        ));
    }
    Ok(first.clone())
}

fn matching_replay_prefix(child: &[ParsedTokenEvent], parent: &[TokenUsageSignature]) -> usize {
    let mut parent_offset = 0usize;
    let mut matched = 0usize;
    for event in child {
        let Some(relative_match) = parent[parent_offset..]
            .iter()
            .position(|signature| signature == &event.signature)
        else {
            break;
        };
        parent_offset += relative_match + 1;
        matched += 1;
    }
    matched
}

fn mark_deferred(
    file_path: &Path,
    modified: i64,
    size: u64,
    reason: PendingReason,
) -> CodexFileSyncResult {
    let entry = PendingEntry {
        modified,
        size,
        reason,
    };
    let should_warn = replay_caches()
        .lock()
        .ok()
        .and_then(|mut caches| {
            caches
                .pending
                .insert(file_path.to_path_buf(), entry.clone())
        })
        .as_ref()
        != Some(&entry);
    if should_warn {
        let reason = match &entry.reason {
            PendingReason::MissingParent(parent) => format!("找不到父 rollout {parent}"),
            PendingReason::Stable(reason) | PendingReason::Retryable(reason) => reason.clone(),
        };
        log::warn!("[CODEX-SYNC] deferred {}: {reason}", file_path.display());
    }
    CodexFileSyncResult {
        deferred: true,
        ..CodexFileSyncResult::default()
    }
}

/// 单文件批量插入的事务粒度。批内 UI 查询会被连接互斥锁挡住约几毫秒，
/// 批间释放锁让读侧插队——兼顾吞吐（避免逐行 autocommit 的每行 fsync）
/// 与大文件重导期间面板的响应性。
const CODEX_INSERT_BATCH_SIZE: usize = 1000;

/// 同步单个 Codex JSONL 文件。
fn sync_single_codex_file(
    db: &Database,
    file_path: &Path,
    rollout_index: &RolloutIndex,
    pass: &mut CodexSyncPass,
    storage: CodexStorage,
) -> Result<CodexFileSyncResult, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_size = metadata.len();

    // 检查同步状态
    let (last_modified, last_offset) = get_codex_sync_state(db, file_path, &pass.cursors, storage)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok(CodexFileSyncResult::default());
    }

    if let Ok(mut caches) = replay_caches().lock() {
        if let Some(pending) = caches.pending.get(file_path).cloned() {
            if pending.modified == file_modified && pending.size == file_size {
                match &pending.reason {
                    PendingReason::MissingParent(parent) if !rollout_index.contains_key(parent) => {
                        return Ok(CodexFileSyncResult {
                            deferred: true,
                            ..CodexFileSyncResult::default()
                        });
                    }
                    PendingReason::Stable(_) => {
                        return Ok(CodexFileSyncResult {
                            deferred: true,
                            ..CodexFileSyncResult::default()
                        });
                    }
                    PendingReason::Retryable(_) => {
                        caches.pending.remove(file_path);
                    }
                    _ => {
                        caches.pending.remove(file_path);
                    }
                }
            }
        }
    }

    let parsed = parse_codex_file(file_path, thread_id_from_filename(file_path))?;
    // Direct callers (including focused fixtures) still get a durable node;
    // the public sync path repeats this write with all claims in one graph so
    // missing parents can be upgraded from unknown to child safely.
    persist_codex_node_for_parsed(db, file_path, &parsed, file_modified, storage)?;
    if !parsed.has_billable_tokens {
        update_codex_sync_state(
            db,
            &file_path_str,
            file_modified,
            parsed.line_offset,
            storage,
        )?;
        return Ok(CodexFileSyncResult::default());
    }
    let Some(root_thread_id) = parsed.root_thread_id.as_deref() else {
        return Ok(mark_deferred(
            file_path,
            file_modified,
            file_size,
            PendingReason::Stable("文件名缺少有效的尾部 UUID".to_string()),
        ));
    };
    if !parsed.root_meta_seen {
        return Ok(mark_deferred(
            file_path,
            file_modified,
            file_size,
            PendingReason::Stable("含计费 token 但尚无 session_meta".to_string()),
        ));
    }
    if parsed.token_events.iter().any(|event| {
        event.event_index.is_some() && event_timestamp_epoch(event.timestamp.as_deref()).is_none()
    }) {
        return Ok(mark_deferred(
            file_path,
            file_modified,
            file_size,
            PendingReason::Stable("含计费 token 但缺少有效 event timestamp".to_string()),
        ));
    }

    let replay_prefix = match &parsed.parent {
        ParentResolution::None => 0,
        ParentResolution::Deferred(reason) => {
            return Ok(mark_deferred(
                file_path,
                file_modified,
                file_size,
                PendingReason::Stable(reason.clone()),
            ));
        }
        ParentResolution::Parent(parent_id) => {
            let Some(cutoff) = parsed.root_timestamp else {
                return Ok(mark_deferred(
                    file_path,
                    file_modified,
                    file_size,
                    PendingReason::Stable(
                        "parented rollout 的 root meta 缺少有效 timestamp".to_string(),
                    ),
                ));
            };
            if let Ok(caches) = replay_caches().lock() {
                if let Some(prefix) = caches
                    .replay_prefixes
                    .get(file_path)
                    .filter(|cached| cached.modified == file_modified && cached.size == file_size)
                    .map(|cached| cached.prefix)
                {
                    prefix
                } else {
                    drop(caches);
                    let parent_signatures =
                        match resolve_parent_signatures(parent_id, cutoff, rollout_index) {
                            Ok(signatures) => signatures,
                            Err(reason) => {
                                let pending_reason = if rollout_index.contains_key(parent_id) {
                                    PendingReason::Retryable(reason)
                                } else {
                                    PendingReason::MissingParent(parent_id.clone())
                                };
                                return Ok(mark_deferred(
                                    file_path,
                                    file_modified,
                                    file_size,
                                    pending_reason,
                                ));
                            }
                        };
                    let prefix = matching_replay_prefix(&parsed.token_events, &parent_signatures);
                    if let Ok(mut caches) = replay_caches().lock() {
                        caches.replay_prefixes.insert(
                            file_path.to_path_buf(),
                            CachedReplayPrefix {
                                modified: file_modified,
                                size: file_size,
                                prefix,
                            },
                        );
                    }
                    prefix
                }
            } else {
                let parent_signatures = resolve_parent_signatures(parent_id, cutoff, rollout_index)
                    .map_err(AppError::Config)?;
                matching_replay_prefix(&parsed.token_events, &parent_signatures)
            }
        }
    };

    if let Ok(mut caches) = replay_caches().lock() {
        caches.pending.remove(file_path);
    }

    let mut result = CodexFileSyncResult::default();
    let mut to_insert: Vec<(&ParsedTokenEvent, u32)> = Vec::new();
    for (token_offset, event) in parsed.token_events.iter().enumerate() {
        let Some(event_index) = event.event_index else {
            continue;
        };
        if token_offset < replay_prefix {
            if event.line_offset > last_offset {
                result.skipped = result.skipped.saturating_add(1);
            }
            continue;
        }
        if event.line_offset <= last_offset {
            continue;
        }
        to_insert.push((event, event_index));
    }

    // 分批事务写库：逐行 autocommit（journal_mode=delete 下每行一整套
    // journal 建立/fsync/删除）是全量重导的最大耗时项。批内单条插入失败
    // 沿用旧行为跳过该条继续；某批 commit 失败则该批整体回滚且游标不推进，
    // 下一 pass 重扫时由 request_id 主键 + 指纹去重兜底，不会双算。
    let batch_count = to_insert.len().div_ceil(CODEX_INSERT_BATCH_SIZE);
    for (batch_index, batch) in to_insert.chunks(CODEX_INSERT_BATCH_SIZE).enumerate() {
        let is_last_batch = batch_index + 1 == batch_count;
        let conn = lock_conn!(db.conn);
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("开启 Codex 会话写入事务失败: {e}")))?;

        let mut batch_imported = 0u32;
        let mut batch_skipped = 0u32;
        let mut batch_suspected = 0u32;
        let mut canonical_batch = CanonicalUsageBatch::default();
        for (event, event_index) in batch {
            let request_id =
                format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{root_thread_id}:{event_index}");
            // A raw compatibility row is admitted only when this exact event
            // can also produce a canonical fact.  The legacy row shape cannot
            // carry source-field presence or a safe event time, so inserting
            // first would leave an unmarked/uncanonical row that later raw
            // retention could interpret incorrectly.
            let Some(fact_observation) = codex_fact_from_event(&request_id, root_thread_id, event)
            else {
                batch_skipped = batch_skipped.saturating_add(1);
                continue;
            };
            if has_codex_storage_coverage_on_conn(&tx, storage, "codex_session", &request_id)?
                || has_codex_storage_session_log_on_conn(&tx, storage, &request_id)?
            {
                batch_skipped = batch_skipped.saturating_add(1);
                continue;
            }

            let created_at = event_timestamp_epoch(event.timestamp.as_deref()).unwrap_or(0);
            let dedup_key = DedupKey {
                app_type: "codex",
                model: &event.model,
                input_tokens: event.delta.input,
                output_tokens: event.delta.output,
                cache_read_tokens: event.delta.cached_input,
                cache_creation_tokens: 0,
                created_at,
            };
            let matched_proxy = if storage == CodexStorage::Published {
                find_matching_proxy_usage_log(&tx, &dedup_key)?
            } else {
                find_matching_proxy_usage_log_for_coverage_source(&tx, &dedup_key)?
            };
            if let Some(proxy_request_id) = matched_proxy.as_deref() {
                // Reserve before looking at the next event.  The reservation
                // is visible to this transaction's subsequent matcher call,
                // which prevents two same-batch events from claiming one
                // proxy row while still allowing a second proxy row to match.
                reserve_codex_proxy_coverage_on_conn(
                    &tx,
                    storage,
                    proxy_request_id,
                    root_thread_id,
                    created_at,
                )?;
            }
            let inserted_compatibility_row = if matched_proxy.is_some() {
                false
            } else {
                match insert_codex_session_entry_on_conn(
                    &tx,
                    &request_id,
                    &event.delta,
                    &event.model,
                    Some(root_thread_id),
                    event.timestamp.as_deref(),
                    &mut batch_suspected,
                    &mut pass.pricing,
                    storage,
                ) {
                    Ok(inserted) => inserted,
                    Err(e) => {
                        log::warn!("[CODEX-SYNC] 插入失败 ({request_id}): {e}");
                        false
                    }
                }
            };

            if inserted_compatibility_row || matched_proxy.is_some() {
                remember_request_precision(&request_id, event.precision);
                canonical_batch.observations.push(fact_observation);
                batch_imported = batch_imported.saturating_add(1);
            } else {
                batch_skipped = batch_skipped.saturating_add(1);
            }
        }
        publish_canonical_batch_on_conn(&tx, storage, canonical_batch)?;
        if is_last_batch {
            // 游标推进与最后一批数据同事务提交：中途崩溃时两者一起回滚，
            // 不会出现"游标已推进但数据缺失"的丢数据窗口。
            update_codex_sync_state_on_conn(
                &tx,
                &file_path_str,
                file_modified,
                parsed.line_offset,
                storage,
            )?;
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Codex 会话写入事务失败: {e}")))?;

        result.imported = result.imported.saturating_add(batch_imported);
        result.skipped = result.skipped.saturating_add(batch_skipped);
        result.suspected_duplicates = result.suspected_duplicates.saturating_add(batch_suspected);
    }

    if to_insert.is_empty() {
        update_codex_sync_state(
            db,
            &file_path_str,
            file_modified,
            parsed.line_offset,
            storage,
        )?;
    }
    Ok(result)
}

/// 插入单条 Codex 会话记录到 proxy_request_logs。
///
/// 调用方负责持锁/事务；`pricing_cache` 按原始 model 字符串键控（
/// `find_codex_pricing` 是纯函数式查找，同串必同结果），全量重导时把
/// 每事件一次的定价 SELECT 降为每模型一次。
#[allow(clippy::too_many_arguments)]
fn insert_codex_session_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    delta: &DeltaTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
    suspected_duplicates: &mut u32,
    pricing_cache: &mut HashMap<String, Option<ModelPricing>>,
    storage: CodexStorage,
) -> Result<bool, AppError> {
    let created_at = timestamp
        .and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
        })
        // Event time is the only safe timestamp for normalized usage.  The
        // parser defers billable events without one; retain epoch here only
        // for the legacy proxy row shape and never use it in a rollup.
        .unwrap_or(0);

    let dedup_key = DedupKey {
        app_type: "codex",
        model,
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: 0,
        created_at,
    };
    let should_skip = if storage == CodexStorage::Published {
        should_skip_session_insert(conn, request_id, &dedup_key)?
    } else {
        has_codex_storage_session_log_on_conn(conn, storage, request_id)?
    };
    if should_skip {
        return Ok(false);
    }
    let has_suspected_duplicate = if storage == CodexStorage::Published {
        has_suspected_codex_session_duplicate(conn, request_id, &dedup_key)?
    } else {
        has_suspected_codex_replay_duplicate(conn, request_id, &dedup_key)?
    };
    if has_suspected_duplicate {
        *suspected_duplicates = suspected_duplicates.saturating_add(1);
        log::warn!(
            "[CODEX-SYNC] 疑似重复会话用量: request_id={request_id}, model={model}, input={}, output={}, cache_read={}",
            delta.input,
            delta.output,
            delta.cached_input
        );
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = pricing_cache
        .entry(model.to_string())
        .or_insert_with(|| find_codex_pricing(conn, model));
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app("codex", &usage, p, multiplier);
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
        "_codex_session",
        "codex",
        model,
        session_id,
        "codex_session",
        created_at,
    );
    row.input_tokens = i64::from(delta.input);
    row.output_tokens = i64::from(delta.output);
    row.cache_read_tokens = i64::from(delta.cached_input);
    row.input_cost_usd = input_cost;
    row.output_cost_usd = output_cost;
    row.cache_read_cost_usd = cache_read_cost;
    row.cache_creation_cost_usd = cache_creation_cost;
    row.total_cost_usd = total_cost;
    row.insert_or_ignore_on_conn(conn, storage)
        .map_err(|error| AppError::Database(format!("插入 Codex 会话日志失败: {error}")))
}

/// 查找 Codex 模型定价（带归一化）
fn find_codex_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, &normalize_codex_model(model_id))
}

fn has_suspected_codex_replay_duplicate(
    conn: &rusqlite::Connection,
    request_id: &str,
    key: &DedupKey,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM codex_replay_session_logs
            WHERE request_id <> ?1
              AND app_type = 'codex'
              AND LOWER(model) = LOWER(?2)
              AND input_tokens = ?3
              AND output_tokens = ?4
              AND cache_read_tokens = ?5
              AND created_at BETWEEN ?6 - ?7 AND ?6 + ?7
        )",
        rusqlite::params![
            request_id,
            key.model,
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
        ],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询重放疑似重复 Codex 会话用量失败: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::session_usage::get_sync_state;
    use tempfile::tempdir;

    const PARENT_ID: &str = "00000000-0000-4000-8000-000000000001";
    const CHILD_A_ID: &str = "00000000-0000-4000-8000-000000000002";

    fn write_jsonl(path: &Path, values: &[serde_json::Value]) {
        let contents = values
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(path, contents).unwrap();
    }

    fn rollout_path(dir: &Path, thread_id: &str) -> PathBuf {
        dir.join(format!("rollout-2026-07-10T03-00-00-{thread_id}.jsonl"))
    }

    fn session_meta_at(
        thread_id: &str,
        forked_from_id: Option<&str>,
        spawned_from_id: Option<&str>,
        timestamp: &str,
    ) -> serde_json::Value {
        let source = spawned_from_id.map_or_else(
            || serde_json::Value::String("cli".to_string()),
            |parent| {
                serde_json::json!({
                    "subagent": {
                        "thread_spawn": { "parent_thread_id": parent }
                    }
                })
            },
        );
        serde_json::json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "forked_from_id": forked_from_id,
                "source": source
            }
        })
    }

    fn session_meta(thread_id: &str) -> serde_json::Value {
        session_meta_at(thread_id, None, None, "2026-07-10T03:00:00Z")
    }

    fn session_meta_with_cwd(thread_id: &str, cwd: &str, timestamp: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "cwd": cwd,
                "source": "cli"
            }
        })
    }

    fn turn_context_for_model_at(model: &str, timestamp: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "turn_context",
            "payload": { "model": model }
        })
    }

    fn turn_context_at(timestamp: &str) -> serde_json::Value {
        turn_context_for_model_at("gpt-5.6-sol", timestamp)
    }

    fn turn_context() -> serde_json::Value {
        turn_context_at("2026-07-10T03:00:01Z")
    }

    #[test]
    fn codex_claim_keeps_native_title_and_session_meta_cwd() {
        let temp = tempdir().unwrap();
        let path = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &path,
            &[
                session_meta_with_cwd(PARENT_ID, "/workspace/codex", "2026-07-10T03:00:00Z"),
                serde_json::json!({
                    "type": "user",
                    "first_user_message": "prompt body must not become title"
                }),
            ],
        );
        let parsed = parse_codex_file(&path, Some(PARENT_ID.to_string())).unwrap();
        let titles = HashMap::from([(PARENT_ID.to_string(), "Native task title".to_string())]);
        let claim = relation_claim_from_parsed(&path, &parsed, 100, &titles).unwrap();
        assert_eq!(claim.metadata.title.as_deref(), Some("Native task title"));
        assert_eq!(
            claim.metadata.project_dir.as_deref(),
            Some("/workspace/codex")
        );
    }

    fn token_count_at(input: u64, cached: u64, output: u64, timestamp: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": { "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output,
                    "reasoning_output_tokens": 0,
                    "total_tokens": input + output
                }}
            }
        })
    }

    fn token_count(input: u64, cached: u64, output: u64) -> serde_json::Value {
        token_count_at(input, cached, output, "2026-07-10T03:00:02Z")
    }

    fn token_count_without_timestamp(input: u64, cached: u64, output: u64) -> serde_json::Value {
        let mut value = token_count(input, cached, output);
        value
            .as_object_mut()
            .expect("token_count must be an object")
            .remove("timestamp");
        value
    }

    #[allow(clippy::too_many_arguments)]
    fn token_count_with_last_at(
        total_input: u64,
        total_cached: u64,
        total_output: u64,
        last_input: u64,
        last_cached: u64,
        last_output: u64,
        limit_id: &str,
        timestamp: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total_input,
                        "cached_input_tokens": total_cached,
                        "output_tokens": total_output,
                        "reasoning_output_tokens": 0,
                        "total_tokens": total_input + total_output
                    },
                    "last_token_usage": {
                        "input_tokens": last_input,
                        "cached_input_tokens": last_cached,
                        "output_tokens": last_output,
                        "reasoning_output_tokens": 0,
                        "total_tokens": last_input + last_output
                    }
                },
                "rate_limits": { "limit_id": limit_id }
            }
        })
    }

    fn sync_test_file(
        db: &Database,
        file: &Path,
        all_files: &[&Path],
    ) -> Result<CodexFileSyncResult, AppError> {
        let files = all_files
            .iter()
            .map(|path| path.to_path_buf())
            .collect::<Vec<_>>();
        let mut pass = CodexSyncPass::load(db, CodexStorage::Published)?;
        sync_single_codex_file(
            db,
            file,
            &build_rollout_index(&files),
            &mut pass,
            CodexStorage::Published,
        )
    }

    #[test]
    fn test_interleaved_counter_lanes_use_exact_last_usage() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        let bengal_event = token_count_with_last_at(
            87_709_262,
            83_563_008,
            240_919,
            151_258,
            147_200,
            87,
            "codex_bengalfox",
            "2026-07-10T03:00:03Z",
        );
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_with_last_at(
                    76_780_408,
                    73_010_432,
                    243_036,
                    175_074,
                    169_728,
                    6_827,
                    "codex",
                    "2026-07-10T03:00:02Z",
                ),
                bengal_event.clone(),
                token_count_with_last_at(
                    76_962_538,
                    73_180_160,
                    243_258,
                    182_130,
                    169_728,
                    222,
                    "codex",
                    "2026-07-10T03:00:04Z",
                ),
                // Repeated snapshots are notifications, not additional API usage.
                bengal_event,
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| {
                (
                    event.delta.input,
                    event.delta.cached_input,
                    event.delta.output,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            deltas,
            vec![
                (175_074, 169_728, 6_827),
                (151_258, 147_200, 87),
                (182_130, 169_728, 222),
            ]
        );
        assert!(parsed.token_events[3].delta.is_zero());
        Ok(())
    }

    #[test]
    fn test_adjacent_replay_burst_across_multiple_sources_is_deduped() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
                token_count_with_last_at(
                    1_000,
                    0,
                    10,
                    100,
                    0,
                    10,
                    "codex_bengalfox",
                    "2026-07-10T03:00:03Z",
                ),
                token_count_with_last_at(
                    1_000,
                    0,
                    10,
                    100,
                    0,
                    10,
                    "codex_spark",
                    "2026-07-10T03:00:04Z",
                ),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100]);
        Ok(())
    }

    #[test]
    fn test_stale_cross_source_signature_does_not_swallow_reset() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                // `codex` emits snapshot X.
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
                // X is replayed under another rate-limit source.
                token_count_with_last_at(
                    1_000,
                    0,
                    10,
                    100,
                    0,
                    10,
                    "codex_bengalfox",
                    "2026-07-10T03:00:03Z",
                ),
                // The original source advances to Y.
                token_count_with_last_at(2_000, 0, 20, 100, 0, 10, "codex", "2026-07-10T03:00:04Z"),
                // A genuine reset later reproduces X. The stale copy retained
                // by `codex_bengalfox` must not classify this as a replay.
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:05Z"),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100, 100, 100]);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn replay_source_missing_keeps_pending_data_intact() -> Result<(), AppError> {
        let temp = tempdir().unwrap();
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, cache_read_tokens, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('pending-row', '_codex_session', 'codex', 'gpt-5.6-sol',
                           10, 2, 0, 0, 200, 1, 'codex_session')",
                [],
            )?;
            set_codex_replay_state_on_conn(&conn, CODEX_REPLAY_PENDING)?;
        }

        let error = sync_codex_usage_with_replay(&db).expect_err("missing source must block reset");
        assert!(error.to_string().contains("没有找到可用于 Codex 用量重放"));
        let conn = lock_conn!(db.conn);
        let row_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'pending-row'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(row_count, 1);
        assert_eq!(codex_replay_state_on_conn(&conn)?, CODEX_REPLAY_PENDING);
        drop(conn);

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        Ok(())
    }

    #[test]
    fn replay_state_transitions_are_idempotent_and_partial_safe() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, cache_read_tokens, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('replay-row', '_codex_session', 'codex', 'gpt-5.6-sol',
                           10, 2, 0, 0, 200, 1, 'codex_session')",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                     date, app_type, provider_id, model, request_count, input_tokens
                 ) VALUES ('2026-08-01', 'codex', '_codex_session', 'gpt-5.6-sol', 7, 70)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                     file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('C:\\Users\\admin\\.codex\\sessions\\2026\\08\\rollout-live.jsonl',
                           1, 1, 1)",
                [],
            )?;
            set_codex_replay_state_on_conn(&conn, CODEX_REPLAY_PENDING)?;
        }

        reset_codex_usage_and_mark_replaying(&db)?;
        {
            let conn = lock_conn!(db.conn);
            let row_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
                [],
                |row| row.get(0),
            )?;
            // Shadow replay never removes the last published generation.
            assert_eq!(row_count, 1);
            let staged_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM codex_replay_session_logs",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(staged_count, 0);
            assert_eq!(codex_replay_state_on_conn(&conn)?, CODEX_REPLAYING);
        }

        finish_codex_replay_if_ready(
            &db,
            &SessionSyncResult {
                files_scanned: 1,
                deferred_files: 1,
                ..Default::default()
            },
        )?;
        {
            let conn = lock_conn!(db.conn);
            assert_eq!(codex_replay_state_on_conn(&conn)?, CODEX_REPLAYING);
            conn.execute(
                "INSERT INTO codex_replay_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('codex_replay', 'root', 'root', 'root', 'explicit', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO codex_replay_rollups (
                    date, app_type, session_id, provider_id, model, data_source,
                    request_count
                 ) VALUES ('2026-08-15', 'codex_replay', 'root', '_codex_session',
                           'gpt-5.6-sol', 'codex_session', 1)",
                [],
            )?;
        }
        finish_codex_replay_if_ready(
            &db,
            &SessionSyncResult {
                files_scanned: 1,
                ..Default::default()
            },
        )?;
        let conn = lock_conn!(db.conn);
        assert_eq!(codex_replay_state_on_conn(&conn)?, CODEX_REPLAY_COMPLETE);
        let legacy_counts: (i64, i64) = conn.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM proxy_request_logs
                  WHERE request_id = 'replay-row'),
                 (SELECT request_count FROM usage_daily_rollups
                  WHERE provider_id = '_codex_session' AND model = 'gpt-5.6-sol')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(legacy_counts, (1, 7));
        let live_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync
             WHERE file_path = 'C:\\Users\\admin\\.codex\\sessions\\2026\\08\\rollout-live.jsonl'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(live_cursor_count, 1);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_thread_spawn_parent_strips_replay_and_keeps_live_usage() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
                turn_context_at("2026-07-10T03:00:10Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
                turn_context(),
                token_count_at(1_000, 900, 100, "2026-07-10T03:00:06Z"),
                token_count_at(1_300, 1_050, 150, "2026-07-10T03:00:07Z"),
            ],
        );

        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!(
            (result.imported, result.skipped, result.deferred),
            (1, 1, false)
        );

        let conn = lock_conn!(db.conn);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
            [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:2")],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (300, 150, 50));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_missing_parent_is_deferred_and_recovered_without_child_change() -> Result<(), AppError>
    {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
                token_count_at(900, 400, 90, "2026-07-10T03:00:06Z"),
            ],
        );

        let deferred = sync_test_file(&db, &child, &[&child])?;
        assert!(deferred.deferred);
        assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));
        {
            let conn = lock_conn!(db.conn);
            let node: (String, String) = conn.query_row(
                "SELECT root_session_id, node_kind FROM agent_session_nodes
                 WHERE app_type = 'codex' AND session_id = ?1",
                [CHILD_A_ID],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            assert_eq!(node, (CHILD_A_ID.to_string(), "unknown".to_string()));
        }

        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                turn_context_at("2026-07-10T03:00:10Z"),
            ],
        );
        let recovered = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!((recovered.imported, recovered.deferred), (1, false));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_matching_proxy_row_still_persists_canonical_fact_and_coverage() -> Result<(), AppError>
    {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        let event_time = "2026-07-10T03:00:02Z";
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "proxy-match-1",
                    "openai",
                    "codex",
                    "gpt-5.6-sol",
                    "gpt-5.6-sol",
                    10,
                    2,
                    1,
                    0,
                    "0.01",
                    100,
                    200,
                    DateTime::parse_from_rfc3339(event_time)
                        .unwrap()
                        .timestamp(),
                    "proxy"
                ],
            )?;
        }
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_at(10, 1, 2, event_time),
            ],
        );

        let result = sync_test_file(&db, &file, &[&file])?;
        assert_eq!(result.imported, 1);
        let conn = lock_conn!(db.conn);
        let compatibility_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        let fact_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        let (source_coverage, proxy_coverage): (i64, i64) = conn.query_row(
            "SELECT
                SUM(CASE WHEN data_source = 'codex_session' THEN 1 ELSE 0 END),
                SUM(CASE WHEN data_source = 'proxy' THEN 1 ELSE 0 END)
             FROM agent_session_canonical_coverage
             WHERE app_type = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(compatibility_rows, 0);
        assert_eq!(fact_rows, 1);
        assert_eq!((source_coverage, proxy_coverage), (1, 1));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_missing_timestamp_never_inserts_unmarked_raw_or_fact() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_without_timestamp(100, 10, 5),
            ],
        );

        let result = sync_test_file(&db, &file, &[&file])?;
        assert!(result.deferred);
        let conn = lock_conn!(db.conn);
        let raw_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        let fact_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((raw_count, fact_count, marker_count), (0, 0, 0));
        drop(conn);
        assert_eq!(get_sync_state(&db, &file.to_string_lossy())?, (0, 0));
        Ok(())
    }
}
