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
use crate::database::{
    lock_conn, AgentSessionCanonicalCoverageMarker, AgentSessionUsageRollupFact, Database,
};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{
    normalize_session_relations, write_agent_session_usage_rollup_fact_on_conn,
    NormalizedUsageRollupFact, RelationClaim, RelationConfidence, RequestCountSemantics,
    SessionNodeMetadata, SessionRelationClaim, TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::usage_stats::{
    find_matching_proxy_usage_log, find_matching_proxy_usage_log_for_coverage_source,
    find_model_pricing, has_proxy_request_id, has_suspected_codex_session_duplicate,
    should_skip_session_insert, DedupKey, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use chrono::{DateTime, Local, TimeZone, Utc};
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
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
};

const CODEX_THREAD_REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";
const CODEX_REPLAY_STATE_KEY: &str = "codex_usage_canonical_replay_v3";
const CODEX_REPLAY_STATE_KEY_V2: &str = "codex_usage_canonical_replay_v2";
const CODEX_REPLAY_PENDING: &str = "pending";
const CODEX_REPLAYING: &str = "replaying";
const CODEX_REPLAY_COMPLETE: &str = "complete";
const CODEX_REPLAY_APP_TYPE: &str = "codex_replay";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexStorage {
    Published,
    Replay,
}

impl CodexStorage {
    fn app_type(self) -> &'static str {
        match self {
            Self::Published => "codex",
            Self::Replay => CODEX_REPLAY_APP_TYPE,
        }
    }

    fn node_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_nodes",
            Self::Replay => "codex_replay_nodes",
        }
    }

    fn rollup_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_usage_rollups",
            Self::Replay => "codex_replay_rollups",
        }
    }

    fn coverage_table(self) -> &'static str {
        match self {
            Self::Published => "agent_session_canonical_coverage",
            Self::Replay => "codex_replay_coverage",
        }
    }

    fn coverage_source(self, source: &str) -> String {
        match self {
            Self::Published => source.to_string(),
            Self::Replay => match source {
                "codex_session" => "codex_session_replay".to_string(),
                "proxy" => "proxy_replay".to_string(),
                _ => source.to_string(),
            },
        }
    }

    fn session_log_table(self) -> &'static str {
        match self {
            Self::Published => "proxy_request_logs",
            Self::Replay => "codex_replay_session_logs",
        }
    }

    fn cursor_table(self) -> &'static str {
        match self {
            Self::Published => "session_log_sync",
            Self::Replay => "codex_replay_sync",
        }
    }
}

fn has_codex_storage_coverage_on_conn(
    conn: &Connection,
    storage: CodexStorage,
    source: &str,
    request_id: &str,
) -> Result<bool, AppError> {
    let sql = format!(
        "SELECT EXISTS(
             SELECT 1 FROM {} WHERE app_type = ?1 AND data_source = ?2 AND request_id = ?3
         )",
        storage.coverage_table()
    );
    conn.query_row(
        &sql,
        rusqlite::params![
            storage.app_type(),
            storage.coverage_source(source),
            request_id
        ],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("读取 Codex 重放覆盖标记失败: {error}")))
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
    let sql = format!(
        "INSERT INTO {} (
             app_type, data_source, request_id, canonical_session_id, marked_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(app_type, data_source, request_id) DO UPDATE SET
             canonical_session_id = excluded.canonical_session_id,
             marked_at = excluded.marked_at",
        storage.coverage_table()
    );
    conn.execute(
        &sql,
        rusqlite::params![
            storage.app_type(),
            storage.coverage_source("proxy"),
            request_id,
            session_id,
            marked_at
        ],
    )
    .map_err(|error| AppError::Database(format!("写入 Codex 代理覆盖预留失败: {error}")))?;
    Ok(())
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

fn sqlite_table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询表 {table} 失败: {error}")))
}

fn sqlite_column_exists(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        rusqlite::params![table, column],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询列 {table}.{column} 失败: {error}")))
}

pub(crate) fn reset_codex_usage_on_conn(
    conn: &rusqlite::Connection,
    codex_dir: &Path,
) -> Result<(), AppError> {
    if sqlite_table_exists(conn, "proxy_request_logs")?
        && sqlite_column_exists(conn, "proxy_request_logs", "data_source")?
    {
        conn.execute(
            "DELETE FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 会话明细失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "usage_daily_rollups")?
        && sqlite_column_exists(conn, "usage_daily_rollups", "provider_id")?
    {
        conn.execute(
            "DELETE FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 用量汇总失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "agent_session_usage_rollups")?
        && sqlite_column_exists(conn, "agent_session_usage_rollups", "app_type")?
    {
        conn.execute(
            "DELETE FROM agent_session_usage_rollups WHERE app_type = 'codex'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 会话用量桶失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "agent_session_nodes")?
        && sqlite_column_exists(conn, "agent_session_nodes", "app_type")?
    {
        conn.execute(
            "DELETE FROM agent_session_nodes WHERE app_type = 'codex'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 会话节点失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "agent_session_canonical_coverage")? {
        Database::delete_agent_session_canonical_coverage_for_source_on_conn(
            conn,
            "codex",
            "codex_session",
        )?;
        Database::delete_agent_session_canonical_coverage_for_source_on_conn(
            conn, "codex", "proxy",
        )?;
    }
    if sqlite_table_exists(conn, "session_log_sync")?
        && sqlite_column_exists(conn, "session_log_sync", "file_path")?
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
    let current = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [CODEX_REPLAY_STATE_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| AppError::Database(format!("读取 Codex 重放状态失败: {error}")))?;
    if let Some(value) = current {
        return Ok(value);
    }
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [CODEX_REPLAY_STATE_KEY_V2],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|value| value.unwrap_or_else(|| CODEX_REPLAY_COMPLETE.to_string()))
    .map_err(|error| AppError::Database(format!("读取 Codex v2 重放状态失败: {error}")))
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
        "DELETE FROM proxy_request_logs
         WHERE app_type = 'codex' AND data_source = 'codex_session';
         DELETE FROM agent_session_usage_rollups WHERE app_type = 'codex';
         DELETE FROM agent_session_nodes WHERE app_type = 'codex';
         DELETE FROM agent_session_canonical_coverage
         WHERE app_type = 'codex' AND data_source IN ('codex_session', 'proxy');
         DELETE FROM session_log_sync
         WHERE file_path LIKE '%/sessions/%/rollout-%'
            OR file_path LIKE '%\\sessions\\%\\rollout-%'
            OR file_path LIKE '%/archived_sessions/rollout-%'
            OR file_path LIKE '%\\archived_sessions\\rollout-%';
         INSERT OR REPLACE INTO proxy_request_logs (
             request_id, provider_id, app_type, model, request_model,
             pricing_model, input_tokens, output_tokens, cache_read_tokens,
             cache_creation_tokens, input_cost_usd, output_cost_usd,
             cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
             latency_ms, first_token_ms, duration_ms, status_code, error_message,
             session_id, provider_type, is_streaming, cost_multiplier, created_at,
             data_source, input_token_semantics
         ) SELECT request_id, provider_id, 'codex', model, request_model,
             pricing_model, input_tokens, output_tokens, cache_read_tokens,
             cache_creation_tokens, input_cost_usd, output_cost_usd,
             cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
             latency_ms, first_token_ms, duration_ms, status_code, error_message,
             session_id, provider_type, is_streaming, cost_multiplier, created_at,
             data_source, input_token_semantics
         FROM codex_replay_session_logs;
         INSERT OR REPLACE INTO agent_session_nodes (
             app_type, session_id, parent_session_id, root_session_id,
             node_kind, relation_confidence, title, project_dir, source_path,
             created_at, last_active_at, last_synced_at
         ) SELECT 'codex', session_id, parent_session_id, root_session_id,
             node_kind, relation_confidence, title, project_dir, source_path,
             created_at, last_active_at, last_synced_at
         FROM codex_replay_nodes;
         INSERT OR REPLACE INTO agent_session_usage_rollups (
             date, app_type, session_id, provider_id, model, request_model,
             pricing_model, data_source, precision, time_semantics,
             request_count_semantics, input_token_semantics, source_identity,
             profile_id, database_identity, base_url_digest, billing_mode, task,
             source_version, sync_window_start, sync_window_end, request_count,
             api_call_count, input_tokens, output_tokens, cache_read_tokens,
             cache_creation_tokens, cache_write_tokens, reasoning_tokens,
             total_cost_usd, cost_status, cost_source, cost_delta_kind,
             correction_state, first_event_at, last_event_at
         ) SELECT date, 'codex', session_id, provider_id, model, request_model,
             pricing_model, data_source, precision, time_semantics,
             request_count_semantics, input_token_semantics, source_identity,
             profile_id, database_identity, base_url_digest, billing_mode, task,
             source_version, sync_window_start, sync_window_end, request_count,
             api_call_count, input_tokens, output_tokens, cache_read_tokens,
             cache_creation_tokens, cache_write_tokens, reasoning_tokens,
             total_cost_usd, cost_status, cost_source, cost_delta_kind,
             correction_state, first_event_at, last_event_at
         FROM codex_replay_rollups;
         INSERT OR REPLACE INTO agent_session_canonical_coverage (
             app_type, data_source, request_id, canonical_session_id, marked_at
         ) SELECT 'codex',
             CASE data_source
                 WHEN 'codex_session_replay' THEN 'codex_session'
                 WHEN 'proxy_replay' THEN 'proxy'
                 ELSE data_source
             END,
             request_id, canonical_session_id, marked_at
         FROM codex_replay_coverage;
         INSERT OR REPLACE INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_synced_at)
         SELECT file_path, last_modified, last_line_offset, last_synced_at
         FROM codex_replay_sync;",
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
        sync_codex_usage_to_storage(db, CodexStorage::Replay)?
    };
    if state == CODEX_REPLAY_PENDING || state == CODEX_REPLAYING {
        finish_codex_replay_if_ready(db, &result)?;
    }
    Ok(result)
}

/// Explicit manual rebuild shares the automatic replay state machine.
/// Execute the explicit Codex rebuild.  `create_backup` is false only when a
/// higher-level provider-scoped rebuild has already taken the single safety
/// backup for the whole operation.  The replay state machine itself remains
/// unchanged: parsing happens in the shadow generation and publication only
/// occurs after a complete, eligible scan.
pub(crate) fn rebuild_codex_usage_with_backup(
    db: &Database,
    create_backup: bool,
) -> Result<SessionSyncResult, AppError> {
    let codex_dir = get_codex_config_dir();
    readable_codex_files(&codex_dir)?;
    if create_backup {
        db.backup_database_file()?;
    }
    reset_codex_usage_and_mark_replaying(db)?;
    let result = sync_codex_usage_to_storage(db, CodexStorage::Replay)?;
    finish_codex_replay_if_ready(db, &result)?;
    Ok(result)
}

#[cfg(test)]
pub fn rebuild_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    rebuild_codex_usage_with_backup(db, true)
}

/// Source-only preflight used by the provider-scoped rebuild command.  It is
/// intentionally read-only and does not touch replay state or the database.
pub(crate) fn preflight_codex_usage() -> Result<(), AppError> {
    let codex_dir = get_codex_config_dir();
    readable_codex_files(&codex_dir).map(|_| ())
}

/// Report whether the most recent Codex replay reached the published state.
/// A scan with errors, deferred files, or no valid identity leaves the state in
/// `replaying`, allowing the caller to return `keptPrevious` without replacing
/// the live generation.
pub(crate) fn codex_rebuild_is_published(db: &Database) -> Result<bool, AppError> {
    Ok(codex_replay_state(db)? == CODEX_REPLAY_COMPLETE)
}

impl Database {
    #[cfg(test)]
    pub(crate) fn reset_codex_usage(&self) -> Result<(), AppError> {
        let codex_dir = get_codex_config_dir();
        let conn = lock_conn!(self.conn);
        conn.execute("SAVEPOINT reset_codex_usage", [])
            .map_err(|error| AppError::Database(format!("开启 Codex 重建事务失败: {error}")))?;
        let result = reset_codex_usage_on_conn(&conn, &codex_dir);
        match result {
            Ok(()) => {
                conn.execute("RELEASE reset_codex_usage", [])
                    .map_err(|error| {
                        AppError::Database(format!("提交 Codex 重建事务失败: {error}"))
                    })?;
                drop(conn);
                clear_codex_replay_caches();
                Ok(())
            }
            Err(error) => {
                conn.execute("ROLLBACK TO reset_codex_usage", []).ok();
                conn.execute("RELEASE reset_codex_usage", []).ok();
                Err(error)
            }
        }
    }
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

/// 从 JSON Value 中提取累计 token 用量
#[cfg(test)]
fn parse_cumulative_tokens(total_usage: &serde_json::Value) -> Option<CumulativeTokens> {
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
    Some(CumulativeTokens {
        input: total_usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cached_input: total_usage
            .get("cached_input_tokens")
            .or_else(|| total_usage.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output: total_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

/// Parse a cumulative token snapshot while retaining which source fields were
/// actually present.  `parse_cumulative_tokens` remains the compatibility
/// helper used by the legacy delta tests; canonical writes use this richer
/// representation so missing cached-input/cache-read is never coerced to 0.
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

    if !files.is_empty() {
        if let Err(error) = rebuild_codex_normalized_rollups(db, storage) {
            result
                .errors
                .push(format!("Codex 会话用量桶重建失败: {error}"));
        }
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

#[cfg(test)]
fn persist_codex_relation_claims(
    db: &Database,
    claims: &[SessionRelationClaim],
) -> Result<(), AppError> {
    persist_codex_relation_claims_to_storage(db, claims, CodexStorage::Published)
}

fn persist_codex_relation_claims_to_storage(
    db: &Database,
    claims: &[SessionRelationClaim],
    storage: CodexStorage,
) -> Result<(), AppError> {
    if claims.is_empty() {
        return Ok(());
    }
    let normalized = normalize_session_relations(claims)?;
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 Codex 会话节点事务失败: {error}")))?;
    for node in &normalized {
        write_codex_node_for_storage_on_conn(&tx, node, storage)?;
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Codex 会话节点事务失败: {error}")))
}

fn write_codex_node_for_storage_on_conn(
    conn: &Connection,
    node: &crate::services::agent_session_usage::NormalizedSessionNode,
    storage: CodexStorage,
) -> Result<(), AppError> {
    let sql = format!(
        "INSERT INTO {} (
             app_type, session_id, parent_session_id, root_session_id,
             node_kind, relation_confidence, title, project_dir, source_path,
             created_at, last_active_at, last_synced_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(app_type, session_id) DO UPDATE SET
             parent_session_id = excluded.parent_session_id,
             root_session_id = excluded.root_session_id,
             node_kind = excluded.node_kind,
             relation_confidence = excluded.relation_confidence,
             title = COALESCE(excluded.title, {}.title),
             project_dir = COALESCE(excluded.project_dir, {}.project_dir),
             source_path = COALESCE(excluded.source_path, {}.source_path),
             created_at = COALESCE(excluded.created_at, {}.created_at),
             last_active_at = COALESCE(excluded.last_active_at, {}.last_active_at),
             last_synced_at = excluded.last_synced_at",
        storage.node_table(),
        storage.node_table(),
        storage.node_table(),
        storage.node_table(),
        storage.node_table(),
        storage.node_table(),
    );
    conn.execute(
        &sql,
        rusqlite::params![
            storage.app_type(),
            &node.session_id,
            &node.parent_session_id,
            &node.root_session_id,
            node.node_kind.as_str(),
            node.relation_confidence.as_str(),
            &node.title,
            &node.project_dir,
            &node.source_path,
            node.created_at,
            node.last_active_at,
            node.last_synced_at,
        ],
    )
    .map_err(|error| AppError::Database(format!("写入 Codex 会话节点失败: {error}")))?;
    Ok(())
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

#[cfg(test)]
fn persist_codex_nodes_for_files(db: &Database, files: &[PathBuf]) -> Result<(), AppError> {
    let thread_titles = load_native_thread_titles();
    persist_codex_nodes_for_files_with_titles_to_storage(
        db,
        files,
        &thread_titles,
        CodexStorage::Published,
    )
}

#[cfg(test)]
fn persist_codex_nodes_for_files_with_titles(
    db: &Database,
    files: &[PathBuf],
    thread_titles: &HashMap<String, String>,
) -> Result<(), AppError> {
    persist_codex_nodes_for_files_with_titles_to_storage(
        db,
        files,
        thread_titles,
        CodexStorage::Published,
    )
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CodexFactKey {
    date: String,
    session_id: String,
    model: String,
    precision: String,
}

#[derive(Debug, Clone, Default)]
struct CodexFactAccumulator {
    initialized: bool,
    request_count: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    first_event_at: Option<i64>,
    last_event_at: Option<i64>,
    request_ids: Vec<String>,
}

fn merge_optional_sum(current: &mut Option<i64>, incoming: Option<i64>) {
    match (current.as_mut(), incoming) {
        (Some(current), Some(incoming)) => *current = (*current).saturating_add(incoming),
        // A nullable component on an already-existing durable row is an
        // unknown value, not an empty accumulator.  Never upgrade that
        // unknown to a later known value.
        (None, Some(_)) => {}
        (_, None) => *current = None,
    }
}

fn merge_optional_min(current: &mut Option<i64>, incoming: Option<i64>) {
    match (current.as_mut(), incoming) {
        (Some(current), Some(incoming)) => *current = (*current).min(incoming),
        (None, Some(_)) => {}
        (_, None) => *current = None,
    }
}

fn merge_optional_max(current: &mut Option<i64>, incoming: Option<i64>) {
    match (current.as_mut(), incoming) {
        (Some(current), Some(incoming)) => *current = (*current).max(incoming),
        (None, Some(_)) => {}
        (_, None) => *current = None,
    }
}

fn merge_codex_fact_accumulator(
    current: &mut CodexFactAccumulator,
    incoming: CodexFactAccumulator,
) {
    if !current.initialized {
        *current = incoming;
        current.initialized = true;
        return;
    }
    merge_optional_sum(&mut current.request_count, incoming.request_count);
    merge_optional_sum(&mut current.input_tokens, incoming.input_tokens);
    merge_optional_sum(&mut current.output_tokens, incoming.output_tokens);
    merge_optional_sum(&mut current.cache_read_tokens, incoming.cache_read_tokens);
    merge_optional_sum(&mut current.reasoning_tokens, incoming.reasoning_tokens);
    merge_optional_min(&mut current.first_event_at, incoming.first_event_at);
    merge_optional_max(&mut current.last_event_at, incoming.last_event_at);
    current.request_ids.extend(incoming.request_ids);
}

fn codex_fact_from_event(
    request_id: &str,
    session_id: &str,
    event: &ParsedTokenEvent,
) -> Option<(CodexFactKey, CodexFactAccumulator)> {
    let timestamp = event_timestamp_epoch(event.timestamp.as_deref())?;
    let components = &event.source_components;
    if components.input.is_none()
        && components.cache_read.is_none()
        && components.output.is_none()
        && components.reasoning.is_none()
    {
        return None;
    }
    let key = CodexFactKey {
        date: Local
            .timestamp_opt(timestamp, 0)
            .single()?
            .date_naive()
            .to_string(),
        session_id: session_id.to_string(),
        model: event.model.clone(),
        precision: event.precision.as_str().to_string(),
    };
    let accumulator = CodexFactAccumulator {
        initialized: true,
        request_count: Some(1),
        input_tokens: components.input.map(i64::from),
        output_tokens: components.output.map(i64::from),
        cache_read_tokens: components.cache_read.map(i64::from),
        reasoning_tokens: components.reasoning.map(i64::from),
        first_event_at: Some(timestamp),
        last_event_at: Some(timestamp),
        request_ids: vec![request_id.to_string()],
    };
    Some((key, accumulator))
}

fn read_existing_codex_fact_on_conn(
    conn: &rusqlite::Connection,
    key: &CodexFactKey,
    storage: CodexStorage,
) -> Result<Option<CodexFactAccumulator>, AppError> {
    let sql = format!(
        "SELECT request_count, input_tokens, output_tokens, cache_read_tokens,
                 reasoning_tokens, first_event_at, last_event_at
         FROM {}
         WHERE date = ?1 AND app_type = ?5 AND session_id = ?2
           AND provider_id = '_codex_session' AND model = ?3 AND request_model = ?3
           AND pricing_model = '' AND data_source = 'codex_session'
           AND precision = ?4 AND time_semantics = 'event_time'
           AND request_count_semantics = 'agent_call' AND input_token_semantics = 0
           AND source_identity = '' AND profile_id = '' AND database_identity = ''
           AND base_url_digest = '' AND billing_mode = '' AND task = ''
           AND source_version = ''
            AND sync_window_start = 0 AND sync_window_end = 0",
        storage.rollup_table()
    );
    let result = conn.query_row(
        &sql,
        rusqlite::params![
            &key.date,
            &key.session_id,
            &key.model,
            &key.precision,
            storage.app_type()
        ],
        |row| {
            Ok(CodexFactAccumulator {
                initialized: true,
                request_count: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                cache_read_tokens: row.get(3)?,
                reasoning_tokens: row.get(4)?,
                first_event_at: row.get(5)?,
                last_event_at: row.get(6)?,
                request_ids: Vec::new(),
            })
        },
    );
    match result {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(AppError::Database(format!(
            "读取 Codex 规范用量桶失败: {error}"
        ))),
    }
}

/// Persist source-aware Codex facts and exact request coverage markers in the
/// same caller-owned transaction as the raw insert.  Legacy raw rows are not
/// a reconstruction source because they cannot distinguish missing components
/// from compatibility zeros.
fn persist_codex_facts_on_conn(
    conn: &rusqlite::Connection,
    observations: impl IntoIterator<Item = (CodexFactKey, CodexFactAccumulator)>,
    storage: CodexStorage,
) -> Result<(), AppError> {
    for (key, incoming) in observations {
        let mut aggregate =
            read_existing_codex_fact_on_conn(conn, &key, storage)?.unwrap_or_default();
        let request_ids = incoming.request_ids.clone();
        merge_codex_fact_accumulator(&mut aggregate, incoming);
        let fact = NormalizedUsageRollupFact {
            date: key.date,
            app_type: "codex".to_string(),
            session_id: key.session_id.clone(),
            provider_id: "_codex_session".to_string(),
            model: key.model.clone(),
            request_model: key.model,
            pricing_model: String::new(),
            data_source: "codex_session".to_string(),
            precision: UsagePrecision::from_str(&key.precision)
                .unwrap_or(UsagePrecision::SessionExact),
            time_semantics: TimeSemantics::EventTime,
            request_count_semantics: RequestCountSemantics::AgentCall,
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
            request_count: aggregate.request_count,
            api_call_count: None,
            input_tokens: aggregate.input_tokens,
            output_tokens: aggregate.output_tokens,
            cache_read_tokens: aggregate.cache_read_tokens,
            cache_creation_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: aggregate.reasoning_tokens,
            total_cost_usd: None,
            cost_status: None,
            cost_source: None,
            cost_delta_kind: None,
            correction_state: None,
            first_event_at: aggregate.first_event_at,
            last_event_at: aggregate.last_event_at,
        };
        write_codex_rollup_fact_on_conn(conn, &fact, storage)?;
        for request_id in request_ids {
            let marked_at = aggregate.last_event_at.unwrap_or(0);
            let marker = AgentSessionCanonicalCoverageMarker {
                app_type: storage.app_type().to_string(),
                data_source: storage.coverage_source("codex_session").to_string(),
                request_id,
                canonical_session_id: Some(key.session_id.clone()),
                marked_at,
            };
            let sql = format!(
                "INSERT INTO {} (app_type, data_source, request_id, canonical_session_id, marked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(app_type, data_source, request_id) DO UPDATE SET
                     canonical_session_id = excluded.canonical_session_id,
                     marked_at = excluded.marked_at",
                storage.coverage_table()
            );
            conn.execute(
                &sql,
                rusqlite::params![
                    marker.app_type,
                    marker.data_source,
                    marker.request_id,
                    marker.canonical_session_id,
                    marker.marked_at
                ],
            )?;
        }
    }
    Ok(())
}

fn write_codex_rollup_fact_on_conn(
    conn: &Connection,
    fact: &NormalizedUsageRollupFact,
    storage: CodexStorage,
) -> Result<(), AppError> {
    if storage == CodexStorage::Published {
        return write_agent_session_usage_rollup_fact_on_conn(conn, fact);
    }
    let mut dao_fact: AgentSessionUsageRollupFact = fact.to_dao()?;
    dao_fact.app_type = storage.app_type().to_string();
    Database::upsert_agent_session_usage_rollup_fact_on_conn_into(
        conn,
        &dao_fact,
        storage.rollup_table(),
    )
}

/// Canonical facts are written atomically while parsing each raw batch.  A
/// later rebuild must not infer source fields from `proxy_request_logs`, whose
/// legacy integer columns cannot represent missing cache-read or cache-create.
fn rebuild_codex_normalized_rollups(
    _db: &Database,
    _storage: CodexStorage,
) -> Result<(), AppError> {
    Ok(())
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
        let mut canonical_observations: HashMap<CodexFactKey, CodexFactAccumulator> =
            HashMap::new();
        for (event, event_index) in batch {
            let request_id =
                format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{root_thread_id}:{event_index}");
            // A raw compatibility row is admitted only when this exact event
            // can also produce a canonical fact.  The legacy row shape cannot
            // carry source-field presence or a safe event time, so inserting
            // first would leave an unmarked/uncanonical row that later raw
            // retention could interpret incorrectly.
            let Some((fact_key, fact_observation)) =
                codex_fact_from_event(&request_id, root_thread_id, event)
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
                let aggregate = canonical_observations.entry(fact_key).or_default();
                merge_codex_fact_accumulator(aggregate, fact_observation);
                batch_imported = batch_imported.saturating_add(1);
            } else {
                batch_skipped = batch_skipped.saturating_add(1);
            }
        }
        persist_codex_facts_on_conn(&tx, canonical_observations, storage)?;
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

/// 插入单条 Codex 会话记录到 proxy_request_logs（自取锁的便捷包装，测试专用；
/// 生产路径走 [`insert_codex_session_entry_on_conn`] 以复用批量事务与定价缓存）
#[cfg(test)]
fn insert_codex_session_entry(
    db: &Database,
    request_id: &str,
    delta: &DeltaTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
    suspected_duplicates: &mut u32,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    insert_codex_session_entry_on_conn(
        &conn,
        request_id,
        delta,
        model,
        session_id,
        timestamp,
        suspected_duplicates,
        &mut HashMap::new(),
        CodexStorage::Published,
    )
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

    let sql = format!(
        "INSERT OR IGNORE INTO {} (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
        storage.session_log_table()
    );
    let inserted_rows = conn
        .prepare_cached(&sql)
        .and_then(|mut stmt| {
            stmt.execute(rusqlite::params![
                request_id,
                "_codex_session", // provider_id
                "codex",          // app_type
                model,
                model, // request_model = model
                delta.input,
                delta.output,
                delta.cached_input,
                0i64, // cache_creation_tokens: Codex 日志无此数据
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,                   // latency_ms
                Option::<i64>::None,    // first_token_ms
                200i64,                 // status_code
                Option::<String>::None, // error_message
                session_id.map(|s| s.to_string()),
                Some("codex_session"), // provider_type
                1i64,                  // is_streaming
                "1.0",                 // cost_multiplier
                created_at,
                "codex_session", // data_source
            ])
        })
        .map_err(|e| AppError::Database(format!("插入 Codex 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
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
    use rusqlite::Connection;
    use tempfile::tempdir;

    const PARENT_ID: &str = "00000000-0000-4000-8000-000000000001";
    const CHILD_A_ID: &str = "00000000-0000-4000-8000-000000000002";
    const CHILD_B_ID: &str = "00000000-0000-4000-8000-000000000003";

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
    fn native_thread_title_uses_db_then_index_without_reading_first_message() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("state_5.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, first_user_message TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                PARENT_ID,
                "Renamed native title",
                "prompt body must stay unread"
            ],
        )
        .unwrap();
        drop(conn);

        let index_path = temp.path().join("session_index.jsonl");
        fs::write(
            &index_path,
            format!("{{\"id\":\"{CHILD_A_ID}\",\"thread_name\":\"Index title\"}}\n"),
        )
        .unwrap();

        let db_titles = load_native_thread_titles_from_db(&db_path);
        assert_eq!(
            db_titles.get(PARENT_ID).map(String::as_str),
            Some("Renamed native title")
        );
        assert!(!db_titles
            .values()
            .any(|title| title == "prompt body must stay unread"));
        let index_titles = load_native_thread_titles_from_index(&index_path);
        assert_eq!(
            index_titles.get(CHILD_A_ID).map(String::as_str),
            Some("Index title")
        );
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

    fn token_count_missing_cache_at(input: u64, output: u64, timestamp: &str) -> serde_json::Value {
        let mut value = token_count_at(input, 0, output, timestamp);
        value["payload"]["info"]["total_token_usage"]
            .as_object_mut()
            .expect("total_token_usage must be an object")
            .remove("cached_input_tokens");
        value
    }

    fn token_count_without_source_components(timestamp: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": { "total_tokens": 99 }
                }
            }
        })
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

    fn token_count_with_last_missing_cache_at(
        total_input: u64,
        total_output: u64,
        last_input: u64,
        last_output: u64,
        limit_id: &str,
        timestamp: &str,
    ) -> serde_json::Value {
        let mut value = token_count_with_last_at(
            total_input,
            0,
            total_output,
            last_input,
            0,
            last_output,
            limit_id,
            timestamp,
        );
        value["payload"]["info"]["last_token_usage"]
            .as_object_mut()
            .expect("last_token_usage must be an object")
            .remove("cached_input_tokens");
        value
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
    fn test_delta_first_event() {
        let prev = None;
        let current = CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 17934);
        assert_eq!(delta.cached_input, 9600);
        assert_eq!(delta.output, 454);
        assert!(!delta.is_zero());
    }

    #[test]
    fn test_delta_subsequent_event() {
        let prev = Some(CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        });
        let current = CumulativeTokens {
            input: 36722,
            cached_input: 27904,
            output: 804,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 36722 - 17934);
        assert_eq!(delta.cached_input, 27904 - 9600);
        assert_eq!(delta.output, 804 - 454);
    }

    #[test]
    fn test_delta_zero_at_task_boundary() {
        let prev = Some(CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        });
        // task 边界：相同的累计值
        let current = CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        };
        let delta = compute_delta(&prev, &current);
        assert!(delta.is_zero());
    }

    #[test]
    fn test_delta_saturating_sub() {
        // 异常情况：当前值小于前值（不应发生，但需防护）
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 50,
            output: 30,
        });
        let current = CumulativeTokens {
            input: 80,
            cached_input: 40,
            output: 20,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 0);
        assert_eq!(delta.cached_input, 0);
        assert_eq!(delta.output, 0);
        assert!(delta.is_zero());
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
    fn test_cross_limit_snapshot_replay_is_not_double_counted() -> Result<(), AppError> {
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
    fn test_cross_source_replay_remains_adjacent_across_non_token_events() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
                turn_context_for_model_at("gpt-5.6-sol", "2026-07-10T03:00:03Z"),
                token_count_with_last_at(
                    1_000,
                    0,
                    10,
                    100,
                    0,
                    10,
                    "codex_bengalfox",
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
    fn test_same_source_repeat_is_deduped_after_another_source_advances() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
                token_count_with_last_at(
                    2_000,
                    0,
                    20,
                    100,
                    0,
                    10,
                    "codex_bengalfox",
                    "2026-07-10T03:00:03Z",
                ),
                // `codex` has not advanced since its X snapshot, so this is a
                // same-source replay even though another source was interleaved.
                token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:04Z"),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100, 100]);
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
    fn test_full_snapshot_dedupe_allows_counter_reset() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        let first =
            token_count_with_last_at(100, 50, 10, 100, 50, 10, "codex", "2026-07-10T03:00:02Z");
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                first.clone(),
                first,
                token_count_with_last_at(
                    200,
                    100,
                    20,
                    100,
                    50,
                    10,
                    "codex",
                    "2026-07-10T03:00:04Z",
                ),
                // A restarted counter may legitimately return to an older
                // total after another full snapshot has advanced the source.
                token_count_with_last_at(100, 50, 10, 50, 25, 5, "codex", "2026-07-10T03:00:05Z"),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100, 100, 50]);
        Ok(())
    }

    #[test]
    fn test_empty_last_usage_falls_back_to_total() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                serde_json::json!({
                    "timestamp": "2026-07-10T03:00:02Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "total_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 0,
                                "output_tokens": 10,
                                "reasoning_output_tokens": 0,
                                "total_tokens": 110
                            },
                            "last_token_usage": {}
                        }
                    }
                }),
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
    fn test_empty_total_does_not_enable_snapshot_deduplication() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        let event = |limit_id: &str, timestamp: &str| {
            serde_json::json!({
                "timestamp": timestamp,
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {},
                        "last_token_usage": {
                            "input_tokens": 100,
                            "cached_input_tokens": 0,
                            "output_tokens": 10,
                            "reasoning_output_tokens": 0,
                            "total_tokens": 110
                        }
                    },
                    "rate_limits": { "limit_id": limit_id }
                }
            })
        };
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                event("codex", "2026-07-10T03:00:02Z"),
                // Without a usable cumulative total, identical per-request
                // usage is not enough evidence that this is a replay.
                event("codex_bengalfox", "2026-07-10T03:00:03Z"),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100, 100]);
        Ok(())
    }

    #[test]
    fn test_total_fallback_uses_session_baseline_across_model_switch() -> Result<(), AppError> {
        let dir = tempdir().unwrap();
        let file = rollout_path(dir.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context_for_model_at("model-a", "2026-07-10T03:00:01Z"),
                token_count_at(100, 50, 10, "2026-07-10T03:00:02Z"),
                turn_context_for_model_at("model-b", "2026-07-10T03:00:03Z"),
                token_count_at(150, 75, 15, "2026-07-10T03:00:04Z"),
            ],
        );

        let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
        let deltas = parsed
            .token_events
            .iter()
            .filter(|event| !event.delta.is_zero())
            .map(|event| event.delta.input)
            .collect::<Vec<_>>();

        assert_eq!(deltas, vec![100, 50]);
        Ok(())
    }

    #[test]
    fn test_parse_cumulative_tokens_valid() {
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 17934,
            "cached_input_tokens": 9600,
            "output_tokens": 454,
            "reasoning_output_tokens": 233,
            "total_tokens": 18388
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.input, 17934);
        assert_eq!(tokens.cached_input, 9600);
        assert_eq!(tokens.output, 454);
    }

    #[test]
    fn test_parse_cumulative_tokens_null() {
        let json = serde_json::Value::Null;
        assert!(parse_cumulative_tokens(&json).is_none());
    }

    #[test]
    fn test_parse_cumulative_tokens_rejects_empty_object_but_accepts_explicit_zero() {
        assert!(parse_cumulative_tokens(&serde_json::json!({})).is_none());

        let tokens = parse_cumulative_tokens(&serde_json::json!({ "input_tokens": 0 }))
            .expect("an explicit zero is valid usage");
        assert_eq!(tokens.input, 0);
        assert_eq!(tokens.cached_input, 0);
        assert_eq!(tokens.output, 0);
    }

    #[test]
    fn test_parse_cumulative_tokens_alt_field_names() {
        // 某些版本可能使用 cache_read_input_tokens 而非 cached_input_tokens
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 1000,
            "cache_read_input_tokens": 500,
            "output_tokens": 200
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.cached_input, 500);
    }

    #[test]
    fn test_collect_codex_session_files_nonexistent() {
        let files = collect_codex_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
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
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn metadata_only_replay_publishes_without_rollup() -> Result<(), AppError> {
        let db = Database::memory()?;
        reset_codex_usage_and_mark_replaying(&db)?;

        // A scan with no parsed identity must not replace the published
        // generation, even when it has no explicit deferred/error marker.
        finish_codex_replay_if_ready(
            &db,
            &SessionSyncResult {
                files_scanned: 1,
                ..Default::default()
            },
        )?;
        {
            let conn = lock_conn!(db.conn);
            assert_eq!(codex_replay_state_on_conn(&conn)?, CODEX_REPLAYING);
        }

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO codex_replay_nodes (
                     app_type, session_id, root_session_id, node_kind,
                     relation_confidence, title, source_path, last_synced_at
                 ) VALUES ('codex_replay', 'metadata-only', 'metadata-only', 'root',
                           'explicit', 'Metadata only', '/codex/rollout.jsonl', 1)",
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
        let published_nodes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_nodes
             WHERE app_type = 'codex' AND session_id = 'metadata-only'",
            [],
            |row| row.get(0),
        )?;
        let published_rollups: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = 'metadata-only'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(published_nodes, 1);
        assert_eq!(published_rollups, 0);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn codex_proxy_reservation_is_immediate_and_distinct_per_event() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        let first_time = "2026-07-10T03:00:02Z";
        let second_time = "2026-07-10T03:00:03Z";
        {
            let conn = lock_conn!(db.conn);
            for request_id in ["proxy-match-a", "proxy-match-b"] {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        total_cost_usd, latency_ms, status_code, created_at, data_source
                    ) VALUES (?, 'openai', 'codex', 'gpt-5.6-sol', 'gpt-5.6-sol',
                              10, 2, 1, 0, '0.01', 100, 200, ?, 'proxy')",
                    rusqlite::params![
                        request_id,
                        DateTime::parse_from_rfc3339(first_time)
                            .expect("valid proxy timestamp")
                            .timestamp()
                    ],
                )?;
            }
        }
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                // Different cumulative totals make these two events distinct
                // snapshots, while each exact request delta matches both proxy
                // rows in the ten-minute dedup window.
                token_count_with_last_at(100, 10, 20, 10, 1, 2, "codex", first_time),
                token_count_with_last_at(200, 20, 40, 10, 1, 2, "codex", second_time),
            ],
        );

        let result = sync_test_file(&db, &file, &[&file])?;
        assert_eq!(result.imported, 2);
        let conn = lock_conn!(db.conn);
        let claimed = conn
            .prepare(
                "SELECT request_id FROM agent_session_canonical_coverage
                 WHERE app_type = 'codex' AND data_source = 'proxy'
                 ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            claimed,
            vec!["proxy-match-a".to_string(), "proxy-match-b".to_string()]
        );
        let (request_count, input_tokens): (i64, i64) = conn.query_row(
            "SELECT request_count, input_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!((request_count, input_tokens), (2, 20));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn codex_proxy_reservation_rolls_back_when_fact_write_fails() -> Result<(), AppError> {
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
                ) VALUES ('proxy-failing-fact', 'openai', 'codex', 'gpt-5.6-sol',
                          'gpt-5.6-sol', 10, 2, 1, 0, '0.01', 100, 200, ?, 'proxy')",
                [DateTime::parse_from_rfc3339(event_time)
                    .expect("valid proxy timestamp")
                    .timestamp()],
            )?;
            conn.execute_batch(
                "CREATE TRIGGER fail_codex_fact BEFORE INSERT ON agent_session_usage_rollups
                 WHEN NEW.app_type = 'codex'
                 BEGIN SELECT RAISE(ABORT, 'forced Codex fact failure'); END;",
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

        let error = sync_test_file(&db, &file, &[&file])
            .expect_err("fact failure must abort the batch transaction");
        assert!(error.to_string().contains("forced Codex fact failure"));
        let conn = lock_conn!(db.conn);
        let proxy_coverage: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'codex' AND data_source = 'proxy'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(proxy_coverage, 0);
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
    fn test_filtered_parent_events_use_subsequence_prefix_alignment() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:02Z"),
                token_count_at(300, 150, 30, "2026-07-10T03:00:03Z"),
                turn_context_at("2026-07-10T03:00:10Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:06Z"),
                token_count_at(300, 150, 30, "2026-07-10T03:00:07Z"),
                token_count_at(450, 220, 45, "2026-07-10T03:00:08Z"),
            ],
        );

        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!((result.imported, result.skipped), (1, 2));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_rollout_is_cached_once_across_fork_cutoffs() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:10Z"),
                turn_context_at("2026-07-10T03:00:20Z"),
            ],
        );

        let early = "2026-07-10T03:00:05Z".parse::<DateTime<Utc>>().unwrap();
        let late = "2026-07-10T03:00:15Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(parent_signatures_before(&parent, early).unwrap().len(), 1);
        let first_timeline =
            Arc::clone(&replay_caches().lock().unwrap().parent_timelines[&parent].timeline);
        assert_eq!(parent_signatures_before(&parent, late).unwrap().len(), 2);

        let caches = replay_caches().lock().unwrap();
        assert_eq!(caches.parent_timelines.len(), 1);
        assert!(Arc::ptr_eq(
            &first_timeline,
            &caches.parent_timelines[&parent].timeline
        ));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_rollout_cache_invalidates_after_append() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let cutoff = "2026-07-10T03:00:15Z".parse::<DateTime<Utc>>().unwrap();
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                turn_context_at("2026-07-10T03:00:20Z"),
            ],
        );
        assert_eq!(parent_signatures_before(&parent, cutoff).unwrap().len(), 1);

        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:10Z"),
                turn_context_at("2026-07-10T03:00:20Z"),
            ],
        );
        assert_eq!(parent_signatures_before(&parent, cutoff).unwrap().len(), 2);

        let caches = replay_caches().lock().unwrap();
        assert_eq!(caches.parent_timelines.len(), 1);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_rollout_content_error_cache_preserves_open_errors() {
        clear_codex_replay_caches();
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let cutoff = "2026-07-10T03:00:05Z".parse::<DateTime<Utc>>().unwrap();
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_without_timestamp(100, 50, 10),
                turn_context_at("2026-07-10T03:00:20Z"),
            ],
        );

        let first_error = parent_signatures_before(&parent, cutoff).unwrap_err();
        assert!(first_error.contains("token_count 缺少有效 timestamp"));
        let cached_timeline =
            || Arc::clone(&replay_caches().lock().unwrap().parent_timelines[&parent].timeline);
        let first_timeline = cached_timeline();

        let second_error = parent_signatures_before(&parent, cutoff).unwrap_err();
        assert_eq!(second_error, first_error);
        assert!(Arc::ptr_eq(&first_timeline, &cached_timeline()));

        fs::remove_file(&parent).unwrap();
        let open_error = parent_signatures_before(&parent, cutoff).unwrap_err();
        assert!(open_error.contains("无法打开父 rollout"));
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_rollout_nanosecond_cutoffs_are_exact() {
        clear_codex_replay_caches();
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:00.000000500Z"),
                turn_context_at("2026-07-10T03:00:00.000000900Z"),
            ],
        );

        let before = "2026-07-10T03:00:00.000000300Z"
            .parse::<DateTime<Utc>>()
            .unwrap();
        let after = "2026-07-10T03:00:00.000000700Z"
            .parse::<DateTime<Utc>>()
            .unwrap();
        assert!(parent_signatures_before(&parent, before)
            .unwrap()
            .is_empty());
        assert_eq!(parent_signatures_before(&parent, after).unwrap().len(), 1);
        assert_eq!(replay_caches().lock().unwrap().parent_timelines.len(), 1);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn test_parent_file_stamp_distinguishes_same_size_same_mtime_files() {
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let replacement = temp.path().join("replacement.jsonl");
        let values = [session_meta(PARENT_ID), token_count(100, 50, 10)];
        write_jsonl(&parent, &values);
        write_jsonl(&replacement, &values);
        let original_file = fs::File::open(&parent).unwrap();
        let original_metadata = original_file.metadata().unwrap();
        let replacement_file = fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap();
        replacement_file
            .set_times(fs::FileTimes::new().set_modified(original_metadata.modified().unwrap()))
            .unwrap();
        let original_stamp = ParentFileStamp::from_file(&original_file).unwrap();
        let replacement_stamp = ParentFileStamp::from_file(&replacement_file).unwrap();
        assert_eq!(
            (original_stamp.size, original_stamp.modified_nanos),
            (replacement_stamp.size, replacement_stamp.modified_nanos)
        );
        assert_ne!(original_stamp, replacement_stamp);
    }

    #[test]
    #[serial_test::serial]
    fn test_empty_fork_imports_no_parent_usage() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:02Z"),
                turn_context_at("2026-07-10T03:00:10Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
                token_count_at(100, 50, 10, "2026-07-10T03:00:06Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:07Z"),
                serde_json::json!({
                    "timestamp": "2026-07-10T03:00:08Z",
                    "type": "event_msg",
                    "payload": { "type": "thread_settings_applied" }
                }),
            ],
        );

        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!(
            (result.imported, result.skipped, result.deferred),
            (0, 2, false)
        );
        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_conflicting_explicit_parents_are_deferred() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &child,
            &[
                session_meta_at(
                    CHILD_A_ID,
                    Some(PARENT_ID),
                    Some(CHILD_B_ID),
                    "2026-07-10T03:00:05Z",
                ),
                token_count_at(100, 50, 10, "2026-07-10T03:00:06Z"),
            ],
        );

        let result = sync_test_file(&db, &child, &[&child])?;
        assert!(result.deferred);
        assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_future_signature_cannot_extend_replay_prefix() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:06Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:07Z"),
            ],
        );

        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!(
            (result.imported, result.skipped, result.deferred),
            (1, 0, false)
        );
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_closed_parent_before_fork_can_align_child_prefix() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
                token_count_at(200, 100, 20, "2026-07-10T03:00:06Z"),
            ],
        );
        let parent_file = fs::OpenOptions::new().write(true).open(&parent).unwrap();
        let cutoff = "2026-07-10T03:00:05Z".parse::<DateTime<Utc>>().unwrap();
        parent_file
            .set_times(fs::FileTimes::new().set_modified(
                SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs((cutoff.timestamp() - 1) as u64),
            ))
            .unwrap();

        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert_eq!((result.imported, result.deferred), (1, false));
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
    fn test_billable_file_without_meta_is_deferred_without_cursor() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(&child, &[turn_context(), token_count(100, 50, 10)]);

        let result = sync_test_file(&db, &child, &[&child])?;
        assert!(result.deferred);
        assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));

        std::thread::sleep(std::time::Duration::from_millis(2));
        write_jsonl(
            &child,
            &[
                turn_context(),
                token_count(100, 50, 10),
                session_meta_at(CHILD_A_ID, None, None, "2026-07-10T03:00:03Z"),
            ],
        );
        let recovered = sync_test_file(&db, &child, &[&child])?;
        assert_eq!((recovered.imported, recovered.deferred), (1, false));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_non_billable_file_without_meta_advances_cursor() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &child,
            &[
                turn_context(),
                token_count_at(0, 0, 0, "2026-07-10T03:00:02Z"),
            ],
        );

        let result = sync_test_file(&db, &child, &[&child])?;
        assert!(!result.deferred);
        assert_eq!(get_sync_state(&db, &child.to_string_lossy())?.1, 2);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_subagents_use_filename_thread_ids() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child_a = rollout_path(temp.path(), CHILD_A_ID);
        let child_b = rollout_path(temp.path(), CHILD_B_ID);
        write_jsonl(
            &child_a,
            &[
                session_meta(CHILD_A_ID),
                turn_context(),
                token_count(100, 50, 10),
            ],
        );
        write_jsonl(
            &child_b,
            &[
                session_meta(CHILD_B_ID),
                turn_context(),
                token_count(200, 100, 20),
            ],
        );

        assert_eq!(
            sync_test_file(&db, &child_a, &[&child_a, &child_b])?.imported,
            1
        );
        assert_eq!(
            sync_test_file(&db, &child_b, &[&child_a, &child_b])?.imported,
            1
        );

        let conn = lock_conn!(db.conn);
        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            vec![
                format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:1"),
                format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_B_ID}:1")
            ]
        );
        Ok(())
    }

    #[test]
    fn test_archived_log_inherits_cursor_and_only_imports_appended_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let sessions = temp.path().join("sessions");
        let archived = temp.path().join("archived_sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(&archived).unwrap();
        let source = rollout_path(&sessions, PARENT_ID);
        let archived_file = rollout_path(&archived, PARENT_ID);
        write_jsonl(
            &archived_file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count(100, 50, 10),
                token_count(200, 100, 20),
            ],
        );

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens,
                    total_cost_usd, latency_ms, status_code, session_id,
                    created_at, data_source
                ) VALUES ('codex_session:parent:2', '_codex_session', 'codex',
                          'gpt-5.6-sol', 'gpt-5.6-sol', 999, 99, 0, '0', 0,
                          200, 'parent', 1, 'codex_session')",
                [],
            )?;
        }
        let source_path = source.to_string_lossy().to_string();
        update_sync_state(&db, &source_path, 1, 3)?;

        assert_eq!(
            sync_test_file(&db, &archived_file, &[&archived_file])?.imported,
            1
        );
        assert_eq!(
            sync_test_file(&db, &archived_file, &[&archived_file])?.imported,
            0
        );

        let conn = lock_conn!(db.conn);
        let old_row_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id = 'codex_session:parent:2'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(old_row_count, 1);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs
             WHERE request_id = ?1",
            [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:2")],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (100, 50, 10));
        let canonical: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, cache_read_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = ?1",
            [PARENT_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(canonical, (Some(1), Some(100), Some(10), Some(50)));
        drop(conn);
        assert_eq!(get_sync_state(&db, &archived_file.to_string_lossy())?.1, 4);

        Ok(())
    }

    #[test]
    fn test_insert_codex_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "codex-proxy",
                    "openai",
                    "codex",
                    "gpt-5.4",
                    "gpt-5.4",
                    10,
                    2,
                    1,
                    7,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let delta = DeltaTokens {
            input: 10,
            cached_input: 1,
            output: 2,
        };
        let mut suspected_duplicates = 0;
        let inserted = insert_codex_session_entry(
            &db,
            "codex-session-dup",
            &delta,
            "gpt-5.4",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
            &mut suspected_duplicates,
        )?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

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
    fn test_codex_session_duplicate_is_observed_but_still_inserted() -> Result<(), AppError> {
        let db = Database::memory()?;
        let delta = DeltaTokens {
            input: 10,
            cached_input: 1,
            output: 2,
        };
        let mut suspected_duplicates = 0;
        assert!(insert_codex_session_entry(
            &db,
            "codex-session-a",
            &delta,
            "gpt-5.4",
            Some("session-a"),
            Some("1970-01-01T00:16:40Z"),
            &mut suspected_duplicates,
        )?);
        assert!(insert_codex_session_entry(
            &db,
            "codex-session-b",
            &delta,
            "gpt-5.4",
            Some("session-b"),
            Some("1970-01-01T00:16:45Z"),
            &mut suspected_duplicates,
        )?);
        assert_eq!(suspected_duplicates, 1);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn reset_codex_usage_only_removes_codex_rows_and_structural_cursors() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let wide_dir = temp.path();
        let current_codex = rollout_path(&wide_dir.join("sessions"), CHILD_A_ID);
        let legacy_codex =
            format!("C:\\old-codex\\archived_sessions\\rollout-old-{CHILD_B_ID}.jsonl");
        let gemini_cursor = wide_dir.join("gemini/sessions/session-123.json");
        let claude_cursor = wide_dir.join(format!("projects/rollout-{PARENT_ID}.jsonl"));

        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, cache_read_tokens, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES
                    ('codex-row', '_codex_session', 'codex', 'gpt', 1, 1, 0, 0, 200, 1, 'codex_session'),
                    ('gemini-row', '_gemini_session', 'gemini', 'gemini', 1, 1, 0, 0, 200, 1, 'gemini_session');
                 INSERT INTO usage_daily_rollups (date, app_type, provider_id, model)
                 VALUES
                    ('2026-07-10', 'codex', '_codex_session', 'gpt'),
                    ('2026-07-10', 'gemini', '_gemini_session', 'gemini');
                 INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('codex', 'codex-node', 'codex-node', 'root', 'explicit', 1);
                 INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, data_source, precision,
                    time_semantics, request_count_semantics,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens
                 ) VALUES ('2026-07-10', 'codex', 'codex-node', 'codex_session',
                           'request_exact', 'event_time', 'agent_call', 1, 1, 0, 1);",
            )?;
            conn.execute(
                "INSERT INTO agent_session_canonical_coverage
                    (app_type, data_source, request_id, canonical_session_id, marked_at)
                 VALUES ('codex', 'codex_session', 'codex-marker', 'codex-node', 1)",
                [],
            )?;
            for path in [
                current_codex.to_string_lossy().to_string(),
                legacy_codex,
                gemini_cursor.to_string_lossy().to_string(),
                claude_cursor.to_string_lossy().to_string(),
            ] {
                conn.execute(
                    "INSERT INTO session_log_sync
                     (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES (?1, 1, 1, 1)",
                    [path],
                )?;
            }

            reset_codex_usage_on_conn(&conn, wide_dir)?;
            let codex_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
                [],
                |row| row.get(0),
            )?;
            let gemini_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'gemini_session'",
                [],
                |row| row.get(0),
            )?;
            let codex_rollups: i64 = conn.query_row(
                "SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
                [],
                |row| row.get(0),
            )?;
            let codex_nodes: i64 = conn.query_row(
                "SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )?;
            let codex_session_rollups: i64 = conn.query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups WHERE app_type = 'codex'",
                [],
                |row| row.get(0),
            )?;
            let codex_coverage: i64 = conn.query_row(
                "SELECT COUNT(*) FROM agent_session_canonical_coverage
                 WHERE app_type = 'codex' AND data_source = 'codex_session'",
                [],
                |row| row.get(0),
            )?;
            let remaining_cursors: i64 =
                conn.query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| {
                    row.get(0)
                })?;
            assert_eq!((codex_rows, gemini_rows, codex_rollups), (0, 1, 0));
            assert_eq!((codex_nodes, codex_session_rollups), (0, 0));
            assert_eq!(codex_coverage, 0);
            assert_eq!(remaining_cursors, 2);
        }
        Ok(())
    }

    // ── 模型名归一化测试 ──

    #[test]
    fn test_normalize_codex_model_lowercase() {
        assert_eq!(normalize_codex_model("GLM-4.6"), "glm-4.6");
        assert_eq!(normalize_codex_model("DeepSeek-Chat"), "deepseek-chat");
        assert_eq!(normalize_codex_model("GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_prefix() {
        assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("azure/gpt-5.2-codex"),
            "gpt-5.2-codex"
        );
        assert_eq!(normalize_codex_model("OPENAI/GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_iso_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("gpt-5.4-pro-2026-03-05"),
            "gpt-5.4-pro"
        );
    }

    #[test]
    fn test_normalize_codex_model_strip_compact_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("claude-opus-4-6-20260206"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_normalize_codex_model_no_change() {
        assert_eq!(normalize_codex_model("gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(normalize_codex_model("o3"), "o3");
        assert_eq!(normalize_codex_model("deepseek-chat"), "deepseek-chat");
    }

    #[test]
    fn test_normalize_codex_model_combined() {
        // prefix + uppercase + ISO date
        assert_eq!(
            normalize_codex_model("openai/GPT-5.4-2026-03-05"),
            "gpt-5.4"
        );
        // prefix + compact date
        assert_eq!(normalize_codex_model("openai/gpt-5.4-20260305"), "gpt-5.4");
    }

    #[test]
    fn test_cached_clamped_to_input() {
        // cached > input 的异常场景应被 min() 钳制
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 0,
            output: 50,
        });
        let current = CumulativeTokens {
            input: 110,       // delta = 10
            cached_input: 80, // delta = 80（异常：大于 input delta）
            output: 60,
        };
        let delta = compute_delta(&prev, &current);
        // 钳制前：cached_input = 80, input = 10
        assert_eq!(delta.cached_input, 80);
        assert_eq!(delta.input, 10);
        // 实际钳制在调用侧：delta.cached_input.min(delta.input)
        let clamped = delta.cached_input.min(delta.input);
        assert_eq!(clamped, 10);
    }

    #[test]
    #[serial_test::serial]
    fn test_normalized_rollout_graph_preserves_each_thread_and_all_depths() -> Result<(), AppError>
    {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let root = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        let grandchild = rollout_path(temp.path(), CHILD_B_ID);
        write_jsonl(&root, &[session_meta(PARENT_ID)]);
        write_jsonl(
            &child,
            &[session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                Some(PARENT_ID),
                "2026-07-10T03:00:05Z",
            )],
        );
        write_jsonl(
            &grandchild,
            &[session_meta_at(
                CHILD_B_ID,
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:00:06Z",
            )],
        );

        persist_codex_nodes_for_files(&db, &[root.clone(), child.clone(), grandchild.clone()])?;

        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT session_id, parent_session_id, root_session_id, node_kind,
                        relation_confidence
                 FROM agent_session_nodes WHERE app_type = 'codex'
                 ORDER BY session_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows,
            vec![
                (
                    PARENT_ID.to_string(),
                    None,
                    PARENT_ID.to_string(),
                    "root".to_string(),
                    "explicit".to_string(),
                ),
                (
                    CHILD_A_ID.to_string(),
                    Some(PARENT_ID.to_string()),
                    PARENT_ID.to_string(),
                    "child".to_string(),
                    "explicit".to_string(),
                ),
                (
                    CHILD_B_ID.to_string(),
                    Some(CHILD_A_ID.to_string()),
                    PARENT_ID.to_string(),
                    "child".to_string(),
                    "explicit".to_string(),
                ),
            ]
        );
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_parent_confidence_cases_fail_closed_for_self_and_filename_mismatch(
    ) -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let self_file = rollout_path(temp.path(), CHILD_A_ID);
        let mismatch_file = rollout_path(temp.path(), CHILD_B_ID);
        write_jsonl(
            &self_file,
            &[session_meta_at(
                CHILD_A_ID,
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:00:05Z",
            )],
        );
        write_jsonl(&mismatch_file, &[session_meta(PARENT_ID)]);
        persist_codex_nodes_for_files(&db, &[self_file, mismatch_file])?;

        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT session_id, root_session_id, node_kind, relation_confidence
                 FROM agent_session_nodes WHERE app_type = 'codex'
                 ORDER BY session_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 2);
        for (session_id, root, kind, confidence) in rows {
            assert_eq!(session_id, root);
            assert_eq!(kind, "conflict");
            assert_eq!(confidence, "conflict");
        }
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_partial_fact_retains_known_components_and_coverage_marker() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context_for_model_at("fixture-model", "2026-07-10T03:00:01Z"),
                token_count_at(100, 10, 5, "2026-07-10T03:00:02Z"),
                token_count_at(140, 14, 7, "2026-07-10T03:00:03Z"),
            ],
        );
        let first = sync_test_file(&db, &file, &[&file])?;
        assert_eq!(first.imported, 2);
        assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
        let conn = lock_conn!(db.conn);
        let fact: (
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        ) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, total_cost_usd
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'codex' AND session_id = ?1",
            [PARENT_ID],
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
        assert_eq!(fact, (Some(140), Some(7), Some(14), None, None));
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_count, 2);
        let marker_ids = conn
            .prepare(
                "SELECT request_id, canonical_session_id
                 FROM agent_session_canonical_coverage
                 WHERE app_type = 'codex' AND data_source = 'codex_session'
                 ORDER BY request_id",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            marker_ids,
            vec![
                (
                    format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:1"),
                    Some(PARENT_ID.to_string()),
                ),
                (
                    format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:2"),
                    Some(PARENT_ID.to_string()),
                ),
            ]
        );
        drop(conn);
        rebuild_codex_normalized_rollups(&db, CodexStorage::Published)?;
        let conn = lock_conn!(db.conn);
        let rebuilt_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = ?1",
            [PARENT_ID],
            |row| row.get(0),
        )?;
        assert_eq!(rebuilt_count, 1);
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

    #[test]
    #[serial_test::serial]
    fn test_unknown_source_components_never_insert_unmarked_raw_or_fact() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_without_source_components("2026-07-10T03:00:02Z"),
            ],
        );

        let result = sync_test_file(&db, &file, &[&file])?;
        assert!(!result.deferred);
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
        assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 3);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_empty_source_version_replaces_migrated_codex_fact_key() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, provider_id, model, request_model,
                    pricing_model, data_source, precision, time_semantics,
                    request_count_semantics, request_count, input_tokens,
                    output_tokens, cache_read_tokens, cache_creation_tokens,
                    first_event_at, last_event_at
                 ) VALUES (
                    '2026-07-10', 'codex', ?1, '_codex_session', 'fixture-model',
                    'fixture-model', '', 'codex_session', 'session_exact',
                    'event_time', 'agent_call', 1, 1, 1, 0, NULL, 1, 1
                 )",
                [PARENT_ID],
            )?;
        }
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context_for_model_at("fixture-model", "2026-07-10T03:00:01Z"),
                token_count_at(10, 0, 2, "2026-07-10T03:00:02Z"),
            ],
        );
        assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = ?1
               AND model = 'fixture-model' AND source_version = ''",
            [PARENT_ID],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);
        let row: (Option<i64>, Option<i64>, Option<i64>, Option<String>) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, source_version
             FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = ?1
               AND model = 'fixture-model' AND source_version = ''",
            [PARENT_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, (Some(2), Some(11), Some(3), Some(String::new())));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_cache_read_zero_is_distinct_from_missing_and_cost_stays_unknown() -> Result<(), AppError>
    {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context_for_model_at("known-zero", "2026-07-10T03:00:01Z"),
                token_count_with_last_at(10, 0, 2, 10, 0, 2, "zero-cache", "2026-07-10T03:00:02Z"),
                turn_context_for_model_at("missing-cache", "2026-07-10T03:00:03Z"),
                token_count_with_last_missing_cache_at(
                    20,
                    4,
                    10,
                    2,
                    "missing-cache",
                    "2026-07-10T03:00:04Z",
                ),
            ],
        );
        assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 2);

        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT model, input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, total_cost_usd
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'codex' ORDER BY model",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                (
                    "known-zero".to_string(),
                    Some(10),
                    Some(2),
                    Some(0),
                    None,
                    None,
                ),
                (
                    "missing-cache".to_string(),
                    Some(10),
                    Some(2),
                    None,
                    None,
                    None,
                ),
            ]
        );
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_count, 2);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_cumulative_missing_cache_read_never_becomes_zero() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = rollout_path(temp.path(), PARENT_ID);
        write_jsonl(
            &file,
            &[
                session_meta(PARENT_ID),
                turn_context_for_model_at("cumulative-missing", "2026-07-10T03:00:01Z"),
                token_count_missing_cache_at(30, 3, "2026-07-10T03:00:02Z"),
                token_count_missing_cache_at(40, 4, "2026-07-10T03:00:03Z"),
            ],
        );
        assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 2);
        let conn = lock_conn!(db.conn);
        let row: (
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        ) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, total_cost_usd
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'codex' AND model = 'cumulative-missing'",
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
        assert_eq!(row, (Some(40), Some(4), None, None, None));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_child_usage_stays_on_own_thread_and_root_self_excludes_descendant(
    ) -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_at(100, 10, 5, "2026-07-10T03:00:01Z"),
                turn_context_at("2026-07-10T03:00:05Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(
                    CHILD_A_ID,
                    Some(PARENT_ID),
                    Some(PARENT_ID),
                    "2026-07-10T03:00:05Z",
                ),
                turn_context(),
                token_count_at(100, 10, 5, "2026-07-10T03:00:06Z"),
                token_count_at(130, 13, 7, "2026-07-10T03:00:07Z"),
            ],
        );
        assert_eq!(
            sync_test_file(&db, &parent, &[&parent, &child])?.imported,
            1
        );
        assert_eq!(sync_test_file(&db, &child, &[&parent, &child])?.imported, 1);
        persist_codex_nodes_for_files(&db, &[parent, child])?;

        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT session_id, input_tokens, output_tokens
                 FROM proxy_request_logs
                 WHERE app_type = 'codex' AND data_source = 'codex_session'
                 ORDER BY session_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                (PARENT_ID.to_string(), 100, 5),
                (CHILD_A_ID.to_string(), 30, 2),
            ]
        );
        let durable_rows = conn
            .prepare(
                "SELECT session_id, input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, total_cost_usd
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'codex' ORDER BY session_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            durable_rows,
            vec![
                (
                    PARENT_ID.to_string(),
                    Some(100),
                    Some(5),
                    Some(10),
                    None,
                    None,
                ),
                (
                    CHILD_A_ID.to_string(),
                    Some(30),
                    Some(2),
                    Some(3),
                    None,
                    None,
                ),
            ]
        );
        let marker_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_canonical_coverage
             WHERE app_type = 'codex' AND data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(marker_count, 2);
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn test_grandchild_usage_keeps_own_canonical_session_id() -> Result<(), AppError> {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let parent = rollout_path(temp.path(), PARENT_ID);
        let child = rollout_path(temp.path(), CHILD_A_ID);
        let grandchild = rollout_path(temp.path(), CHILD_B_ID);
        write_jsonl(
            &parent,
            &[
                session_meta(PARENT_ID),
                turn_context(),
                token_count_at(100, 10, 5, "2026-07-10T03:00:01Z"),
                turn_context_at("2026-07-10T03:00:05Z"),
            ],
        );
        write_jsonl(
            &child,
            &[
                session_meta_at(
                    CHILD_A_ID,
                    Some(PARENT_ID),
                    Some(PARENT_ID),
                    "2026-07-10T03:00:02Z",
                ),
                turn_context(),
                token_count_at(100, 10, 5, "2026-07-10T03:00:03Z"),
                token_count_at(130, 13, 7, "2026-07-10T03:00:04Z"),
                turn_context_at("2026-07-10T03:00:05Z"),
            ],
        );
        write_jsonl(
            &grandchild,
            &[
                session_meta_at(CHILD_B_ID, None, Some(CHILD_A_ID), "2026-07-10T03:00:05Z"),
                turn_context(),
                token_count_at(130, 13, 7, "2026-07-10T03:00:06Z"),
                token_count_at(150, 15, 9, "2026-07-10T03:00:07Z"),
            ],
        );
        assert_eq!(
            sync_test_file(&db, &parent, &[&parent, &child, &grandchild])?.imported,
            1
        );
        assert_eq!(
            sync_test_file(&db, &child, &[&parent, &child, &grandchild])?.imported,
            1
        );
        assert_eq!(
            sync_test_file(&db, &grandchild, &[&parent, &child, &grandchild])?.imported,
            1
        );

        let conn = lock_conn!(db.conn);
        let rows = conn
            .prepare(
                "SELECT session_id, request_count, input_tokens, output_tokens
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'codex' ORDER BY session_id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                (PARENT_ID.to_string(), Some(1), Some(100), Some(5)),
                (CHILD_A_ID.to_string(), Some(1), Some(30), Some(2)),
                (CHILD_B_ID.to_string(), Some(1), Some(20), Some(2)),
            ]
        );
        Ok(())
    }

    /// 真实语料回放验收 harness（仅手动运行，勿在 CI 跑）。
    ///
    /// 把真实 `~/.codex/sessions` 语料在内存库上做一次全量重导，输出计时与
    /// 结果快照。用于性能改动的行为等价验证：改动前后各跑一次，两侧
    /// `CODEX_REPLAY_OUT` 文件必须逐字节相同。
    ///
    /// ```bash
    /// CODEX_REPLAY_OUT=/tmp/replay.tsv \
    ///   cargo test --release replay_real_codex_corpus -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn replay_real_codex_corpus() -> Result<(), AppError> {
        let Some(real_home) = dirs::home_dir() else {
            eprintln!("[REPLAY] no home dir, skipping");
            return Ok(());
        };
        let real_sessions = real_home.join(".codex").join("sessions");
        if !real_sessions.is_dir() {
            eprintln!("[REPLAY] {} not found, skipping", real_sessions.display());
            return Ok(());
        }

        // 临时 HOME 里只放一个指向真实语料的只读 symlink，避免测试
        // 触碰真实 ~/.cc-switch / ~/.codex 下的任何其他内容。
        let temp = tempfile::tempdir().expect("create temp home");
        fs::create_dir_all(temp.path().join(".codex")).expect("mkdir .codex");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_sessions, temp.path().join(".codex").join("sessions"))
            .expect("symlink sessions");
        let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        clear_codex_replay_caches();
        // CODEX_REPLAY_DISK=1 时用临时 HOME 下的磁盘库：逐行 autocommit 的
        // 主要成本是磁盘 journal fsync，内存库测不出真实写库开销。
        let db = if std::env::var("CODEX_REPLAY_DISK").is_ok() {
            Database::init()?
        } else {
            Database::memory()?
        };
        let start = std::time::Instant::now();
        let result = sync_codex_usage(&db)?;
        let full_elapsed = start.elapsed();
        eprintln!(
            "[REPLAY] full reimport: imported={} skipped={} suspected_dup={} deferred={} files={} errors={} elapsed={:.2?}",
            result.imported,
            result.skipped,
            result.suspected_duplicates,
            result.deferred_files,
            result.files_scanned,
            result.errors.len(),
            full_elapsed
        );

        let start = std::time::Instant::now();
        let steady = sync_codex_usage(&db)?;
        eprintln!(
            "[REPLAY] steady pass: imported={} deferred={} elapsed={:.2?}",
            steady.imported,
            steady.deferred_files,
            start.elapsed()
        );

        if let Ok(out_path) = std::env::var("CODEX_REPLAY_OUT") {
            use std::io::Write;
            let conn = lock_conn!(db.conn);
            let mut stmt = conn
                .prepare(
                    "SELECT request_id, model, request_model, input_tokens, output_tokens,
                            cache_read_tokens, cache_creation_tokens,
                            input_cost_usd, output_cost_usd, cache_read_cost_usd,
                            cache_creation_cost_usd, total_cost_usd,
                            session_id, provider_id, provider_type, status_code,
                            is_streaming, cost_multiplier, created_at, data_source
                     FROM proxy_request_logs
                     WHERE data_source = 'codex_session'
                     ORDER BY request_id",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    let mut fields = Vec::with_capacity(20);
                    for idx in 0..20 {
                        fields.push(match row.get_ref(idx)? {
                            rusqlite::types::ValueRef::Null => "NULL".to_string(),
                            rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                            rusqlite::types::ValueRef::Real(v) => v.to_string(),
                            rusqlite::types::ValueRef::Text(v) => {
                                String::from_utf8_lossy(v).into_owned()
                            }
                            rusqlite::types::ValueRef::Blob(v) => format!("blob:{}", v.len()),
                        });
                    }
                    Ok(fields.join("\t"))
                })
                .map_err(|e| AppError::Database(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut out = fs::File::create(&out_path).expect("create replay out file");
            for line in &rows {
                writeln!(out, "{line}").expect("write replay row");
            }
            eprintln!("[REPLAY] wrote {} rows to {out_path}", rows.len());
        }

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        Ok(())
    }
}
