//! OpenCode 会话日志使用追踪
//!
//! 从 ~/.local/share/opencode/opencode.db (SQLite) 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.local/share/opencode/opencode.db
//!   → session 表获取所有会话
//!   → message 表获取 assistant 消息
//!   → 解析 data JSON 提取 tokens/cost/model
//!   → proxy_request_logs 表
//! ```

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{SessionNodeMetadata, SessionRelationClaim};
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::session_usage_pipeline::{
    publish_canonical_batch_on_conn, reserve_canonical_coverage_on_conn, CanonicalReplaceScope,
    CanonicalUsageBatch, RawUsageLogRow, UsagePublishTarget, UsageSourceSpec,
};
use crate::services::usage_stats::{
    find_matching_proxy_usage_log, find_model_pricing, DedupKey, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use chrono::{Local, TimeZone};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

/// 从 opencode message.data JSON 中提取的 token 和费用数据
#[derive(Debug, Clone)]
struct OpenCodeMessageData {
    input_tokens: u32,
    output_tokens: u32,
    reasoning_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    cost: f64,
    cost_is_explicit: bool,
    model_id: String,
    model_is_explicit: bool,
    timestamp_ms: i64,
    timestamp_is_explicit: bool,
    token_fields_complete: bool,
}

struct OpenCodeMessageQueryResult {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenCodeStorage {
    Json,
    Sqlite,
}

#[derive(Debug, Clone)]
struct OpenCodeSessionRecord {
    session_id: String,
    sync_watermark: i64,
    title: Option<String>,
    project_dir: Option<String>,
    created_at: Option<i64>,
    last_active_at: Option<i64>,
    source_path: String,
    storage: OpenCodeStorage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenCodeInsertOutcome {
    Inserted,
    RefreshedSource,
    ExistingSource,
    ProxyDuplicate { proxy_request_id: String },
    Rejected,
}

#[derive(Debug, Clone)]
struct CostResolution {
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
    total_cost: String,
    canonical_total_cost: Option<String>,
    pricing_model: String,
}

impl CostResolution {
    fn unknown() -> Self {
        // proxy_request_logs predates nullable costs, so its legacy NOT NULL
        // columns receive a compatibility zero.  The canonical rollup keeps
        // the honest `None` in canonical_total_cost instead of persisting that
        // compatibility value as a real cost.
        Self {
            input_cost: "0".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
            total_cost: "0".to_string(),
            canonical_total_cost: None,
            pricing_model: String::new(),
        }
    }
}

/// 同步 OpenCode 使用数据
pub fn sync_opencode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_opencode_db_path();
    let json_storage = db_path
        .parent()
        .map(|path| path.join("storage"))
        .unwrap_or_else(|| PathBuf::from("storage"));

    sync_opencode_usage_from_sources(db, &db_path, &json_storage)
}

/// Source-oriented entry point kept separate from the environment lookup so
/// focused tests can use anonymous temporary SQLite/JSON fixtures.  The
/// production wrapper above still follows the existing OPENCODE_DB/XDG path
/// resolution and never reads an arbitrary path supplied by callers.
pub(crate) fn sync_opencode_usage_from_sources(
    db: &Database,
    db_path: &Path,
    json_storage: &Path,
) -> Result<SessionSyncResult, AppError> {
    let sqlite_sessions = if db_path.exists() {
        let opencode_conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?;
        query_sqlite_session_records(&opencode_conn, db_path)?
    } else {
        Vec::new()
    };

    let json_sessions = query_json_session_records(json_storage);
    let sqlite_ids: HashSet<String> = sqlite_sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect();
    // Keep SQLite as the authoritative storage variant for a bare session ID,
    // exactly matching the Session Manager provider's arbitration contract.
    let sessions = arbitrate_storage_sessions(sqlite_sessions.clone(), json_sessions);

    let mut result = SessionSyncResult {
        files_scanned: if db_path.exists() { 1 } else { 0 },
        ..SessionSyncResult::default()
    };
    if json_storage.exists() {
        result.files_scanned = result.files_scanned.saturating_add(1);
    }
    if sessions.is_empty() {
        return Ok(result);
    }

    let sqlite_conn = if db_path.exists() {
        Some(
            rusqlite::Connection::open_with_flags(
                db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?,
        )
    } else {
        None
    };

    // The file-level watermark remains the old DB mtime + WAL mtime contract.
    // Session-level watermarks continue to gate rescans, while every accepted
    // session writes a replacement aggregate bucket so repeated syncs cannot
    // drift or let the final message overwrite earlier messages.
    let db_path_str = db_path.to_string_lossy().to_string();
    let file_modified = if db_path.exists() {
        let metadata = fs::metadata(db_path)
            .map_err(|e| AppError::Config(format!("无法读取 opencode.db 元数据: {e}")))?;
        let mut modified = metadata_modified_nanos(&metadata);
        let wal_path = db_path.with_extension("db-wal");
        if let Ok(wal_meta) = fs::metadata(&wal_path) {
            modified = modified.max(metadata_modified_nanos(&wal_meta));
        }
        Some(modified)
    } else {
        None
    };
    let (last_file_modified, _) = if db_path.exists() {
        get_sync_state(db, &db_path_str)?
    } else {
        (0, 0)
    };

    let mut has_sync_errors = false;
    let mut processed_sqlite = 0usize;
    let mut processed_json = 0usize;

    for session in &sessions {
        let source_key = match session.storage {
            OpenCodeStorage::Sqlite => format!("{db_path_str}:{}", session.session_id),
            OpenCodeStorage::Json => format!("{}:{}", json_storage.display(), session.session_id),
        };
        let (last_session_watermark, _) = get_sync_state(db, &source_key)?;

        // The provider's file arbitration must still be honored when the DB
        // file itself has not changed: a JSON copy with the same ID is never
        // allowed to sneak back in through a second storage path.
        if session.sync_watermark <= last_session_watermark {
            continue;
        }

        let query_result = match session.storage {
            OpenCodeStorage::Sqlite => sqlite_conn
                .as_ref()
                .ok_or_else(|| AppError::Database("SQLite source disappeared during sync".into()))
                .and_then(|conn| query_assistant_messages(conn, &session.session_id)),
            OpenCodeStorage::Json => {
                query_json_assistant_messages(json_storage, &session.session_id)
            }
        };

        let query_result = match query_result {
            Ok(value) => value,
            Err(error) => {
                let message = format!("OpenCode 会话消息查询失败 {}: {error}", session.session_id);
                log::warn!("[OPENCODE-SYNC] {message}");
                result.errors.push(message);
                has_sync_errors = true;
                continue;
            }
        };

        match persist_opencode_session(db, session, &query_result, &mut result) {
            Ok(session_has_incomplete_usage) => {
                if session_has_incomplete_usage {
                    // Keep the cursor behind an in-progress or incomplete
                    // usage row.  The next sync can observe its completed
                    // replacement instead of permanently losing the usage.
                    continue;
                }
                if let Err(error) = update_sync_state(db, &source_key, session.sync_watermark, 0) {
                    let message = format!(
                        "OpenCode 会话同步状态更新失败 {}: {error}",
                        session.session_id
                    );
                    log::warn!("[OPENCODE-SYNC] {message}");
                    result.errors.push(message);
                    has_sync_errors = true;
                } else {
                    match session.storage {
                        OpenCodeStorage::Sqlite => processed_sqlite += 1,
                        OpenCodeStorage::Json => processed_json += 1,
                    }
                }
            }
            Err(error) => {
                let message = format!("OpenCode 会话写入失败 {}: {error}", session.session_id);
                log::warn!("[OPENCODE-SYNC] {message}");
                result.errors.push(message);
                has_sync_errors = true;
            }
        }
    }

    if let (Some(modified), true) = (file_modified, !has_sync_errors) {
        if modified > last_file_modified {
            update_sync_state(db, &db_path_str, modified, 0)?;
        }
    }

    if result.imported > 0 {
        log::info!(
            "[OPENCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, SQLite {} 个会话, JSON {} 个会话",
            result.imported,
            result.skipped,
            processed_sqlite,
            processed_json
        );
    }

    // Keep this binding in the source-oriented function: it documents and
    // enforces that arbitration is based on the bare ID rather than path/cwd.
    debug_assert!(sqlite_ids.iter().all(|id| {
        sessions
            .iter()
            .any(|session| session.storage == OpenCodeStorage::Sqlite && &session.session_id == id)
    }));
    Ok(result)
}

fn persist_opencode_session(
    db: &Database,
    session: &OpenCodeSessionRecord,
    query_result: &OpenCodeMessageQueryResult,
    result: &mut SessionSyncResult,
) -> Result<bool, AppError> {
    let mut has_incomplete_usage = query_result.has_incomplete_usage;
    let mut claim = SessionRelationClaim::standalone("opencode", session.session_id.clone());
    claim.metadata = SessionNodeMetadata {
        title: session.title.clone(),
        project_dir: session.project_dir.clone(),
        source_path: Some(session.source_path.clone()),
        created_at: session.created_at,
        last_active_at: session.last_active_at,
        last_synced_at: session.sync_watermark,
    };
    // Only messages that are owned by this source may contribute to the
    // canonical rollup. A proxy match is represented by a coverage marker for
    // the proxy request itself, not by a second direct-source fact.
    let mut canonical_messages = Vec::new();
    let mut proxy_duplicate_request_ids = Vec::new();
    let mut claimed_proxy_request_ids = HashSet::new();
    for (message_id, msg_data) in &query_result.messages {
        let request_id = format!("opencode_session:{}:{message_id}", session.session_id);
        if !msg_data.token_fields_complete
            || !msg_data.model_is_explicit
            || !msg_data.timestamp_is_explicit
            || local_date_from_timestamp_ms(msg_data.timestamp_ms).is_none()
        {
            // Legacy proxy_request_logs stores source-derived fields as
            // non-null compatibility values.  Missing token components,
            // model IDs, or valid event timestamps must not be rewritten as
            // zeros, "unknown", or the current time, so defer this session
            // until a complete replacement message is available.
            has_incomplete_usage = true;
            result.skipped = result.skipped.saturating_add(1);
            continue;
        }
        match insert_opencode_message_with_claims(
            db,
            &request_id,
            msg_data,
            &session.session_id,
            &claimed_proxy_request_ids,
        ) {
            Ok(OpenCodeInsertOutcome::Inserted | OpenCodeInsertOutcome::RefreshedSource) => {
                result.imported = result.imported.saturating_add(1);
                canonical_messages.push((message_id.clone(), msg_data.clone()));
            }
            Ok(OpenCodeInsertOutcome::ExistingSource) => {
                result.skipped = result.skipped.saturating_add(1);
                canonical_messages.push((message_id.clone(), msg_data.clone()));
            }
            Ok(OpenCodeInsertOutcome::ProxyDuplicate { proxy_request_id }) => {
                result.skipped = result.skipped.saturating_add(1);
                if claimed_proxy_request_ids.insert(proxy_request_id.clone()) {
                    proxy_duplicate_request_ids.push(proxy_request_id);
                }
            }
            Ok(OpenCodeInsertOutcome::Rejected) => {
                result.skipped = result.skipped.saturating_add(1);
            }
            Err(error) => {
                let message = format!("OpenCode 消息插入失败 {request_id}: {error}");
                log::warn!("[OPENCODE-SYNC] {message}");
                result.errors.push(message);
                result.skipped = result.skipped.saturating_add(1);
                return Err(AppError::Database("OpenCode raw usage write failed".into()));
            }
        }
    }

    // The shared pipeline groups complete messages by every durable fact
    // dimension before it replaces that fact.  Incomplete source evidence
    // remains excluded rather than being fabricated as zero-valued usage.
    let mut canonical_batch = build_usage_batch(db, &session.session_id, &canonical_messages);
    if !has_incomplete_usage {
        canonical_batch.replace_scopes.push(CanonicalReplaceScope {
            app_type: "opencode".into(),
            session_id: session.session_id.clone(),
            data_source: "opencode_session".into(),
        });
    }
    canonical_batch.relation_claims.push(claim);
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("开启 OpenCode canonical 写入事务失败: {error}"))
    })?;
    let marked_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    publish_canonical_batch_on_conn(&tx, UsagePublishTarget::Published, canonical_batch)?;
    for request_id in proxy_duplicate_request_ids {
        reserve_canonical_coverage_on_conn(
            &tx,
            UsagePublishTarget::Published,
            "opencode",
            "proxy",
            &request_id,
            Some(&session.session_id),
            marked_at,
        )?;
    }
    tx.commit().map_err(|error| {
        AppError::Database(format!("提交 OpenCode canonical 写入事务失败: {error}"))
    })?;
    Ok(has_incomplete_usage)
}

/// The OpenCode provider's storage arbitration is intentionally pure and
/// keyed only by the bare session ID.  SQLite rows come first; JSON rows with
/// the same ID are discarded even when their paths/directories differ.
fn arbitrate_storage_sessions(
    sqlite_sessions: Vec<OpenCodeSessionRecord>,
    json_sessions: Vec<OpenCodeSessionRecord>,
) -> Vec<OpenCodeSessionRecord> {
    let sqlite_ids: HashSet<String> = sqlite_sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect();
    sqlite_sessions
        .into_iter()
        .chain(
            json_sessions
                .into_iter()
                .filter(|session| !sqlite_ids.contains(&session.session_id)),
        )
        .collect()
}

fn query_sqlite_session_records(
    conn: &rusqlite::Connection,
    db_path: &Path,
) -> Result<Vec<OpenCodeSessionRecord>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.directory, s.time_created, s.time_updated,
                    MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated))
             FROM session s
             LEFT JOIN message m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY 6",
        )
        .map_err(|e| AppError::Database(format!("准备会话查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            let session_id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let directory: String = row.get(2)?;
            let created_at: i64 = row.get(3)?;
            let last_active_at: i64 = row.get(4)?;
            let sync_watermark: i64 = row.get(5)?;
            Ok(OpenCodeSessionRecord {
                session_id: session_id.clone(),
                sync_watermark,
                title: if title.trim().is_empty() {
                    None
                } else {
                    Some(title)
                },
                project_dir: if directory.trim().is_empty() {
                    None
                } else {
                    Some(directory)
                },
                created_at: Some(created_at),
                last_active_at: Some(last_active_at),
                source_path: format!("sqlite:{}:{session_id}", db_path.display()),
                storage: OpenCodeStorage::Sqlite,
            })
        })
        .map_err(|e| AppError::Database(format!("查询会话失败: {e}")))?;

    rows.map(|row| row.map_err(|e| AppError::Database(format!("读取会话行失败: {e}"))))
        .collect()
}

fn query_json_session_records(storage: &Path) -> Vec<OpenCodeSessionRecord> {
    let session_dir = storage.join("session");
    if !session_dir.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_json_files(&session_dir, &mut files);
    let mut records: HashMap<String, OpenCodeSessionRecord> = HashMap::new();
    for path in files {
        let data = match fs::read_to_string(&path) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let value: serde_json::Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(session_id) = value.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        if session_id.trim().is_empty() {
            continue;
        }
        let title = value
            .get("title")
            .and_then(|title| title.as_str())
            .filter(|title| !title.trim().is_empty())
            .map(str::to_string);
        let project_dir = value
            .get("directory")
            .and_then(|directory| directory.as_str())
            .filter(|directory| !directory.trim().is_empty())
            .map(str::to_string);
        let created_at = value
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(parse_timestamp_value);
        let last_active_at = value
            .get("time")
            .and_then(|time| time.get("updated"))
            .and_then(parse_timestamp_value)
            .or(created_at);
        let mut sync_watermark = path_modified_nanos(&path);
        let message_dir = storage.join("message").join(session_id);
        sync_watermark = sync_watermark.max(latest_modified_nanos(&message_dir));
        let record = OpenCodeSessionRecord {
            session_id: session_id.to_string(),
            sync_watermark,
            title,
            project_dir,
            created_at,
            last_active_at,
            source_path: path.to_string_lossy().to_string(),
            storage: OpenCodeStorage::Json,
        };
        let replace = records
            .get(session_id)
            .map(|existing| existing.sync_watermark < record.sync_watermark)
            .unwrap_or(true);
        if replace {
            records.insert(session_id.to_string(), record);
        }
    }
    let mut records: Vec<_> = records.into_values().collect();
    records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    records
}

fn path_modified_nanos(path: &Path) -> i64 {
    fs::metadata(path)
        .ok()
        .map(|metadata| metadata_modified_nanos(&metadata))
        .unwrap_or(0)
}

fn latest_modified_nanos(path: &Path) -> i64 {
    if !path.exists() {
        return 0;
    }
    let mut latest = path_modified_nanos(path);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            latest = latest.max(if child.is_dir() {
                latest_modified_nanos(&child)
            } else {
                path_modified_nanos(&child)
            });
        }
    }
    latest
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

fn parse_timestamp_value(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
}

/// Query SQLite assistant rows and delegate all completion/field checks to a
/// shared parser used by the legacy JSON storage variant.
fn query_assistant_messages(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<OpenCodeMessageQueryResult, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created")
        .map_err(|e| AppError::Database(format!("准备消息查询失败: {e}")))?;
    let rows = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询消息失败: {e}")))?;
    let rows: Result<Vec<_>, _> = rows.collect();
    let rows = rows.map_err(|e| AppError::Database(format!("读取消息行失败: {e}")))?;
    Ok(parse_assistant_rows(rows))
}

fn query_json_assistant_messages(
    storage: &Path,
    session_id: &str,
) -> Result<OpenCodeMessageQueryResult, AppError> {
    let message_dir = storage.join("message").join(session_id);
    if !message_dir.is_dir() {
        return Ok(OpenCodeMessageQueryResult {
            messages: Vec::new(),
            has_incomplete_usage: false,
        });
    }
    let mut files = Vec::new();
    collect_json_files(&message_dir, &mut files);
    let mut rows = Vec::new();
    for path in files {
        let data = match fs::read_to_string(&path) {
            Ok(data) => data,
            Err(_) => continue,
        };
        let message_id = serde_json::from_str::<serde_json::Value>(&data)
            .ok()
            .and_then(|value| {
                value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .map(str::to_string)
            })
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_string)
            });
        if let Some(message_id) = message_id {
            rows.push((message_id, data));
        }
    }
    Ok(parse_assistant_rows(rows))
}

fn parse_assistant_rows(rows: Vec<(String, String)>) -> OpenCodeMessageQueryResult {
    let mut messages = Vec::new();
    let mut has_incomplete_usage = false;
    let mut seen_message_ids = HashSet::new();
    for (message_id, data_json) in rows {
        if !seen_message_ids.insert(message_id.clone()) {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("role").and_then(|role| role.as_str()) != Some("assistant") {
            continue;
        }
        let completed = value
            .get("time")
            .and_then(|time| time.get("completed"))
            .filter(|completed| !completed.is_null());
        if completed.is_none() {
            has_incomplete_usage = true;
            continue;
        }
        if value.get("tokens").is_none() {
            // A completed assistant row without token evidence is still an
            // incomplete source row.  Keep the session cursor behind it so a
            // later replacement can be imported.
            has_incomplete_usage = true;
            continue;
        }
        match parse_message_data(&value) {
            Some(msg_data) => {
                if !msg_data.token_fields_complete
                    || !msg_data.model_is_explicit
                    || !msg_data.timestamp_is_explicit
                    || local_date_from_timestamp_ms(msg_data.timestamp_ms).is_none()
                {
                    has_incomplete_usage = true;
                }
                messages.push((message_id, msg_data));
            }
            None => {
                has_incomplete_usage = true;
            }
        }
    }
    messages.sort_by_key(|(_, message)| message.timestamp_ms);
    OpenCodeMessageQueryResult {
        messages,
        has_incomplete_usage,
    }
}

/// 解析 opencode message.data JSON 为结构化数据
fn parse_message_data(value: &serde_json::Value) -> Option<OpenCodeMessageData> {
    let tokens = value.get("tokens")?;

    let input = tokens.get("input").and_then(|value| value.as_u64());
    let output = tokens.get("output").and_then(|value| value.as_u64());
    let reasoning = tokens.get("reasoning").and_then(|value| value.as_u64());
    let input_tokens = input.unwrap_or(0).min(u32::MAX as u64) as u32;
    let output_tokens = output.unwrap_or(0).min(u32::MAX as u64) as u32;
    let reasoning_tokens = reasoning.unwrap_or(0).min(u32::MAX as u64) as u32;

    let cache_obj = tokens.get("cache");
    let cache_read = cache_obj
        .and_then(|cache| cache.get("read"))
        .and_then(|value| value.as_u64());
    let cache_write = cache_obj
        .and_then(|cache| cache.get("write"))
        .and_then(|value| value.as_u64());
    let cache_read_tokens = cache_read.unwrap_or(0).min(u32::MAX as u64) as u32;
    let cache_write_tokens = cache_write.unwrap_or(0).min(u32::MAX as u64) as u32;

    let explicit_cost = value.get("cost").and_then(|value| value.as_f64());
    let cost = explicit_cost.unwrap_or(0.0);
    let model = value.get("modelID").and_then(|value| value.as_str());
    let model_id = model
        .filter(|model| !model.trim().is_empty())
        .unwrap_or("unknown")
        .to_string();
    let timestamp = value
        .get("time")
        .and_then(|time| time.get("created"))
        .and_then(parse_timestamp_value);
    let timestamp_ms = timestamp.unwrap_or(0);

    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost,
        cost_is_explicit: explicit_cost.is_some_and(|value| value.is_finite()),
        model_id,
        model_is_explicit: model.is_some_and(|value| !value.trim().is_empty()),
        timestamp_ms,
        timestamp_is_explicit: timestamp.is_some(),
        token_fields_complete: input.is_some()
            && output.is_some()
            && reasoning.is_some()
            && cache_read.is_some()
            && cache_write.is_some(),
    })
}

fn output_tokens_with_reasoning(message: &OpenCodeMessageData) -> u32 {
    message
        .output_tokens
        .saturating_add(message.reasoning_tokens)
}

fn source_cost(message: &OpenCodeMessageData) -> Option<Decimal> {
    if message.cost_is_explicit && message.cost.is_finite() && message.cost > 0.0 {
        Decimal::from_str(&message.cost.to_string()).ok()
    } else {
        None
    }
}

fn resolve_message_cost(db: &Database, message: &OpenCodeMessageData) -> CostResolution {
    if let Some(source_cost) = source_cost(message) {
        return CostResolution {
            input_cost: "0".to_string(),
            output_cost: "0".to_string(),
            cache_read_cost: "0".to_string(),
            cache_creation_cost: "0".to_string(),
            total_cost: source_cost.to_string(),
            canonical_total_cost: Some(source_cost.to_string()),
            pricing_model: String::new(),
        };
    }

    let output_with_reasoning = output_tokens_with_reasoning(message);
    let usage = TokenUsage {
        input_tokens: message.input_tokens,
        output_tokens: output_with_reasoning,
        cache_read_tokens: message.cache_read_tokens,
        cache_creation_tokens: message.cache_write_tokens,
        model: Some(message.model_id.clone()),
        message_id: None,
    };
    let Ok(conn) = db.conn.lock() else {
        return CostResolution::unknown();
    };
    let Some(pricing) = find_model_pricing(&conn, &message.model_id) else {
        return CostResolution::unknown();
    };
    let cost = CostCalculator::calculate_for_app("opencode", &usage, &pricing, Decimal::from(1));
    CostResolution {
        input_cost: cost.input_cost.to_string(),
        output_cost: cost.output_cost.to_string(),
        cache_read_cost: cost.cache_read_cost.to_string(),
        cache_creation_cost: cost.cache_creation_cost.to_string(),
        total_cost: cost.total_cost.to_string(),
        canonical_total_cost: Some(cost.total_cost.to_string()),
        pricing_model: message.model_id.clone(),
    }
}

fn local_date_from_timestamp_ms(timestamp_ms: i64) -> Option<String> {
    if !timestamp_ms.is_positive() {
        return None;
    }
    Local
        .timestamp_opt(
            timestamp_ms / 1000,
            (timestamp_ms % 1000) as u32 * 1_000_000,
        )
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d").to_string())
}

fn build_usage_batch(
    db: &Database,
    session_id: &str,
    messages: &[(String, OpenCodeMessageData)],
) -> CanonicalUsageBatch {
    let mut batch = CanonicalUsageBatch::default();
    for (message_id, message) in messages {
        // A missing token component, model ID, or event time is unknown rather
        // than zero.  Do not fabricate either a raw compatibility row or a
        // durable canonical bucket from incomplete evidence.
        if !message.token_fields_complete
            || !message.model_is_explicit
            || !message.timestamp_is_explicit
        {
            continue;
        }
        let Some(date) = local_date_from_timestamp_ms(message.timestamp_ms) else {
            continue;
        };
        let cost = resolve_message_cost(db, message);
        let created_at = message.timestamp_ms / 1000;
        let source = UsageSourceSpec::new(
            "opencode",
            "_opencode_session",
            "opencode_session",
            crate::services::agent_session_usage::UsagePrecision::RequestExact,
            crate::services::agent_session_usage::TimeSemantics::EventTime,
            crate::services::agent_session_usage::RequestCountSemantics::AssistantMessage,
        );
        let mut fact = source.fact(
            date,
            session_id,
            message.model_id.clone(),
            message.model_id.clone(),
            cost.pricing_model,
        );
        fact.request_count = Some(1);
        fact.input_tokens = Some(i64::from(message.input_tokens));
        fact.output_tokens = Some(i64::from(output_tokens_with_reasoning(message)));
        fact.cache_read_tokens = Some(i64::from(message.cache_read_tokens));
        fact.cache_creation_tokens = Some(i64::from(message.cache_write_tokens));
        fact.total_cost_usd = cost.canonical_total_cost;
        fact.first_event_at = Some(created_at);
        fact.last_event_at = Some(created_at);
        batch.replace_observe(format!("opencode_session:{session_id}:{message_id}"), fact);
    }
    batch
}

fn insert_opencode_message_with_claims(
    db: &Database,
    request_id: &str,
    msg: &OpenCodeMessageData,
    session_id: &str,
    claimed_proxy_request_ids: &HashSet<String>,
) -> Result<OpenCodeInsertOutcome, AppError> {
    if !msg.token_fields_complete
        || !msg.model_is_explicit
        || !msg.timestamp_is_explicit
        || local_date_from_timestamp_ms(msg.timestamp_ms).is_none()
    {
        return Ok(OpenCodeInsertOutcome::Rejected);
    }

    let cost = resolve_message_cost(db, msg);
    let conn = lock_conn!(db.conn);

    let created_at = msg.timestamp_ms / 1000;

    // OpenCode 使用 Anthropic 风格：input 是新鲜输入，cache 单独计
    // output 包含 reasoning tokens（按输出计费）
    let output_with_reasoning = output_tokens_with_reasoning(msg);
    let mut raw = RawUsageLogRow::native_session(
        request_id,
        "_opencode_session",
        "opencode",
        msg.model_id.as_str(),
        Some(session_id),
        "opencode_session",
        created_at,
    );
    raw.pricing_model = (!cost.pricing_model.is_empty()).then(|| cost.pricing_model.clone());
    raw.input_tokens = i64::from(msg.input_tokens);
    raw.output_tokens = i64::from(output_with_reasoning);
    raw.cache_read_tokens = i64::from(msg.cache_read_tokens);
    raw.cache_creation_tokens = i64::from(msg.cache_write_tokens);
    raw.input_cost_usd = cost.input_cost;
    raw.output_cost_usd = cost.output_cost;
    raw.cache_read_cost_usd = cost.cache_read_cost;
    raw.cache_creation_cost_usd = cost.cache_creation_cost;
    raw.total_cost_usd = cost.total_cost;

    // A same-source row is eligible for canonical reconstruction and may be a
    // rewritten message. Refresh only the OpenCode-native row; a proxy-owned
    // request with the same ID is deliberately outside this update scope.
    let same_source_exists: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM proxy_request_logs
             WHERE request_id = ?1
               AND app_type = 'opencode'
               AND data_source = 'opencode_session'
         )",
        rusqlite::params![request_id],
        |row| row.get(0),
    )?;
    if same_source_exists {
        return Ok(if refresh_opencode_raw_on_conn(&conn, &raw)? {
            OpenCodeInsertOutcome::RefreshedSource
        } else {
            OpenCodeInsertOutcome::ExistingSource
        });
    }

    let dedup_key = DedupKey {
        app_type: "opencode",
        model: &msg.model_id,
        input_tokens: msg.input_tokens,
        output_tokens: output_with_reasoning,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_write_tokens,
        created_at,
    };
    // Prefer an already-marked proxy row for this session on rescans. The
    // normal matcher intentionally hides covered rows, but hiding it here
    // would make a rescan promote the same native message into a second
    // canonical source row after the first pass established proxy ownership.
    let proxy_request_id = find_covered_proxy_usage_log_for_session(
        &conn,
        &dedup_key,
        session_id,
        claimed_proxy_request_ids,
    )?
    .or(find_matching_proxy_usage_log_excluding_claimed(
        &conn,
        &dedup_key,
        claimed_proxy_request_ids,
    )?);
    if let Some(proxy_request_id) = proxy_request_id {
        // A single source batch can contain distinct native messages with the
        // same fingerprint. Once a matching proxy row has been claimed by an
        // earlier message in this batch, admit this later native row instead
        // of assigning the one proxy request twice.
        if !claimed_proxy_request_ids.contains(&proxy_request_id) {
            return Ok(OpenCodeInsertOutcome::ProxyDuplicate { proxy_request_id });
        }
    }

    let inserted = raw
        .insert_or_ignore_on_conn(&conn, UsagePublishTarget::Published)
        .map_err(|error| AppError::Database(format!("插入 OpenCode 会话日志失败: {error}")))?;

    Ok(if inserted {
        OpenCodeInsertOutcome::Inserted
    } else {
        OpenCodeInsertOutcome::Rejected
    })
}

fn refresh_opencode_raw_on_conn(
    conn: &rusqlite::Connection,
    raw: &RawUsageLogRow,
) -> Result<bool, AppError> {
    conn.execute(
        "UPDATE proxy_request_logs
         SET model = ?1, request_model = ?2, pricing_model = ?3,
             input_tokens = ?4, output_tokens = ?5,
             cache_read_tokens = ?6, cache_creation_tokens = ?7,
             input_cost_usd = ?8, output_cost_usd = ?9,
             cache_read_cost_usd = ?10, cache_creation_cost_usd = ?11,
             total_cost_usd = ?12, session_id = ?13, created_at = ?14
         WHERE request_id = ?15
           AND app_type = 'opencode' AND data_source = 'opencode_session'
           AND (
               model IS NOT ?1 OR request_model IS NOT ?2 OR pricing_model IS NOT ?3
               OR input_tokens IS NOT ?4 OR output_tokens IS NOT ?5
               OR cache_read_tokens IS NOT ?6 OR cache_creation_tokens IS NOT ?7
               OR input_cost_usd IS NOT ?8 OR output_cost_usd IS NOT ?9
               OR cache_read_cost_usd IS NOT ?10 OR cache_creation_cost_usd IS NOT ?11
               OR total_cost_usd IS NOT ?12 OR session_id IS NOT ?13
               OR created_at IS NOT ?14
           )",
        rusqlite::params![
            &raw.model,
            &raw.request_model,
            &raw.pricing_model,
            raw.input_tokens,
            raw.output_tokens,
            raw.cache_read_tokens,
            raw.cache_creation_tokens,
            &raw.input_cost_usd,
            &raw.output_cost_usd,
            &raw.cache_read_cost_usd,
            &raw.cache_creation_cost_usd,
            &raw.total_cost_usd,
            &raw.session_id,
            raw.created_at,
            &raw.request_id,
        ],
    )
    .map(|updated| updated > 0)
    .map_err(|error| AppError::Database(format!("更新 OpenCode raw compatibility 行失败: {error}")))
}

fn find_covered_proxy_usage_log_for_session(
    conn: &rusqlite::Connection,
    key: &DedupKey,
    session_id: &str,
    claimed_proxy_request_ids: &HashSet<String>,
) -> Result<Option<String>, AppError> {
    let coverage_table_exists: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_session_canonical_coverage'
         )",
        [],
        |row| row.get(0),
    )?;
    if !coverage_table_exists {
        return Ok(None);
    }
    let mut statement = conn.prepare(
        "SELECT l.request_id
         FROM proxy_request_logs l
         INNER JOIN agent_session_canonical_coverage coverage
           ON coverage.app_type = l.app_type
          AND coverage.data_source = 'proxy'
          AND coverage.request_id = l.request_id
          AND coverage.canonical_session_id = ?1
         WHERE COALESCE(l.data_source, 'proxy') = 'proxy'
           AND l.app_type = 'opencode'
           AND l.status_code >= 200
           AND l.status_code < 300
           AND l.input_tokens = ?2
           AND l.output_tokens = ?3
           AND l.cache_read_tokens = ?4
           AND (l.cache_creation_tokens = ?5 OR ?5 = 0)
           AND l.created_at BETWEEN ?6 - ?7 AND ?6 + ?7
           AND (
               LOWER(l.model) = LOWER(?8)
               OR LOWER(l.model) = 'unknown'
               OR LOWER(?8) = 'unknown'
           )
         ORDER BY ABS(l.created_at - ?6), l.request_id",
    )?;
    let rows = statement.query_map(
        rusqlite::params![
            session_id,
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.cache_creation_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
            key.model,
        ],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        let request_id = row?;
        if !claimed_proxy_request_ids.contains(&request_id) {
            return Ok(Some(request_id));
        }
    }
    Ok(None)
}

/// The shared matcher returns the closest proxy row, which is enough for a
/// single event but cannot express the current batch's already-claimed IDs.
/// When a batch has claimed one fingerprint, enumerate the same candidate set
/// and skip those IDs so the next native message can claim the next proxy row.
fn find_matching_proxy_usage_log_excluding_claimed(
    conn: &rusqlite::Connection,
    key: &DedupKey,
    claimed_proxy_request_ids: &HashSet<String>,
) -> Result<Option<String>, AppError> {
    if claimed_proxy_request_ids.is_empty() {
        return find_matching_proxy_usage_log(conn, key);
    }

    let coverage_table_exists: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_session_canonical_coverage'
         )",
        [],
        |row| row.get(0),
    )?;
    let coverage_filter = if coverage_table_exists {
        "AND NOT EXISTS (
               SELECT 1 FROM agent_session_canonical_coverage coverage
               WHERE coverage.app_type = l.app_type
                 AND coverage.data_source = 'proxy'
                 AND coverage.request_id = l.request_id
           )"
    } else {
        ""
    };
    let sql = format!(
        "SELECT l.request_id
         FROM proxy_request_logs l
         WHERE COALESCE(l.data_source, 'proxy') = 'proxy'
           AND l.app_type = 'opencode'
           AND l.status_code >= 200
           AND l.status_code < 300
           AND l.input_tokens = ?1
           AND l.output_tokens = ?2
           AND l.cache_read_tokens = ?3
           AND (l.cache_creation_tokens = ?4 OR ?4 = 0)
           AND l.created_at BETWEEN ?5 - ?6 AND ?5 + ?6
           AND (
               LOWER(l.model) = LOWER(?7)
               OR LOWER(l.model) = 'unknown'
               OR LOWER(?7) = 'unknown'
           )
           {coverage_filter}
         ORDER BY ABS(l.created_at - ?5), l.request_id"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        rusqlite::params![
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.cache_creation_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
            key.model,
        ],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        let request_id = row?;
        if !claimed_proxy_request_ids.contains(&request_id) {
            return Ok(Some(request_id));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_message_data_full() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "cost": 0.0023113,
            "tokens": {
                "total": 56554,
                "input": 3272,
                "output": 383,
                "reasoning": 419,
                "cache": {
                    "write": 0,
                    "read": 52480
                }
            },
            "modelID": "deepseek-v4-pro",
            "providerID": "deepseek",
            "time": {
                "created": 1779755333700i64,
                "completed": 1779755350639i64
            }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 3272);
        assert_eq!(data.output_tokens, 383);
        assert_eq!(data.reasoning_tokens, 419);
        assert_eq!(data.cache_read_tokens, 52480);
        assert_eq!(data.cache_write_tokens, 0);
        assert!((data.cost - 0.0023113).abs() < 1e-10);
        assert!(data.cost_is_explicit);
        assert!(data.model_is_explicit);
        assert!(data.timestamp_is_explicit);
        assert!(data.token_fields_complete);
        assert_eq!(data.model_id, "deepseek-v4-pro");
        assert_eq!(data.timestamp_ms, 1779755333700);
    }

    #[test]
    fn test_parse_message_data_keeps_explicit_zero_tokens() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": {
                "input": 0,
                "output": 0,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "modelID": "test"
        });
        let data = parse_message_data(&json).expect("explicit zero components are known");
        assert_eq!(data.input_tokens, 0);
        assert_eq!(data.output_tokens, 0);
        assert_eq!(data.reasoning_tokens, 0);
        assert!(data.token_fields_complete);
    }

    #[test]
    fn test_query_assistant_messages_skips_incomplete() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();

        let done = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "created": 1, "completed": 2 }
        })
        .to_string();
        let in_progress = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 500, "output": 0 },
            "modelID": "m",
            "time": { "created": 3 }
        })
        .to_string();

        conn.execute(
            "INSERT INTO message VALUES ('done', 's1', 1, ?1), ('wip', 's1', 2, ?2)",
            rusqlite::params![done, in_progress],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, "s1").unwrap();
        // 只返回已完成（带 time.completed）的消息，半截的被跳过
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].0, "done");
        assert!(result.has_incomplete_usage);
    }

    fn session_fixture(id: &str, storage: OpenCodeStorage) -> OpenCodeSessionRecord {
        OpenCodeSessionRecord {
            session_id: id.to_string(),
            sync_watermark: 10,
            title: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: format!("{storage:?}:{id}"),
            storage,
        }
    }

    fn completed_message(
        message_id: &str,
        input: u64,
        output: u64,
        reasoning: u64,
        cache_read: u64,
        cache_write: u64,
        cost: Option<f64>,
        created_ms: i64,
    ) -> (String, OpenCodeMessageData) {
        let mut value = serde_json::json!({
            "role": "assistant",
            "tokens": {
                "input": input,
                "output": output,
                "reasoning": reasoning,
                "cache": { "read": cache_read, "write": cache_write }
            },
            "modelID": "fixture-model",
            "time": { "created": created_ms, "completed": created_ms + 1 }
        });
        if let Some(cost) = cost {
            value["cost"] = serde_json::json!(cost);
        }
        (
            message_id.to_string(),
            parse_message_data(&value).expect("complete fixture message"),
        )
    }

    fn insert_matching_proxy_row(db: &Database, request_id: &str, message: &OpenCodeMessageData) {
        let conn = db.conn.lock().expect("lock proxy fixture database");
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, latency_ms, status_code, created_at
             ) VALUES (?1, 'proxy-provider', 'opencode', ?2, ?3, ?4, ?5, ?6, 0, 200, ?7)",
            rusqlite::params![
                request_id,
                message.model_id,
                message.input_tokens,
                output_tokens_with_reasoning(message),
                message.cache_read_tokens,
                message.cache_write_tokens,
                message.timestamp_ms / 1000,
            ],
        )
        .expect("insert matching proxy fixture");
    }

    #[test]
    fn one_proxy_and_two_identical_native_messages_leave_one_native_owned() {
        let db = Database::memory().expect("memory database");
        let first = completed_message("native-1", 10, 5, 2, 3, 4, Some(0.1), 1_800_000_000_000);
        let second = completed_message("native-2", 10, 5, 2, 3, 4, Some(0.1), 1_800_000_000_000);
        insert_matching_proxy_row(&db, "proxy-opencode-1", &first.1);

        let session = session_fixture("ses_one_proxy", OpenCodeStorage::Sqlite);
        let query_result = OpenCodeMessageQueryResult {
            messages: vec![first, second],
            has_incomplete_usage: false,
        };
        let mut result = SessionSyncResult::default();
        persist_opencode_session(&db, &session, &query_result, &mut result)
            .expect("persist one matching proxy");

        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 1);
        let conn = db.conn.lock().expect("lock one-proxy database");
        let counts: (i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs),
                    (SELECT request_count FROM agent_session_usage_rollups
                     WHERE app_type = 'opencode' AND session_id = 'ses_one_proxy'),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage
                     WHERE app_type = 'opencode' AND data_source = 'proxy'),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage
                     WHERE app_type = 'opencode' AND data_source = 'opencode_session'),
                    (SELECT input_tokens FROM agent_session_usage_rollups
                     WHERE app_type = 'opencode' AND session_id = 'ses_one_proxy')",
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
            )
            .expect("one-proxy counts");
        assert_eq!(counts, (2, 1, 1, 1, 10));
    }

    #[test]
    fn complete_session_rescan_replaces_obsolete_canonical_buckets_and_raw_row() {
        let db = Database::memory().expect("memory database");
        let session = session_fixture("ses_rewrite", OpenCodeStorage::Sqlite);
        let original = completed_message(
            "rewritten-message",
            10,
            5,
            2,
            3,
            4,
            Some(0.1),
            1_800_000_000_000,
        );
        persist_opencode_session(
            &db,
            &session,
            &OpenCodeMessageQueryResult {
                messages: vec![original],
                has_incomplete_usage: false,
            },
            &mut SessionSyncResult::default(),
        )
        .expect("persist original message");

        persist_opencode_session(
            &db,
            &session,
            &OpenCodeMessageQueryResult {
                messages: Vec::new(),
                has_incomplete_usage: true,
            },
            &mut SessionSyncResult::default(),
        )
        .expect("preserve baseline during incomplete scan");
        let conn = db.conn.lock().expect("lock incomplete scan database");
        let retained: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups
                 WHERE app_type = 'opencode' AND session_id = 'ses_rewrite'",
                [],
                |row| row.get(0),
            )
            .expect("retained rollup count");
        assert_eq!(retained, 1);
        drop(conn);

        let mut rewritten = completed_message(
            "rewritten-message",
            20,
            6,
            1,
            4,
            5,
            Some(0.2),
            1_800_086_400_000,
        );
        rewritten.1.model_id = "rewritten-model".into();
        let expected_date =
            local_date_from_timestamp_ms(rewritten.1.timestamp_ms).expect("rewritten local date");
        let mut rewritten_result = SessionSyncResult::default();
        persist_opencode_session(
            &db,
            &session,
            &OpenCodeMessageQueryResult {
                messages: vec![rewritten.clone()],
                has_incomplete_usage: false,
            },
            &mut rewritten_result,
        )
        .expect("persist rewritten message");
        assert_eq!(rewritten_result.imported, 1);
        let conn = db.conn.lock().expect("lock rewritten database");
        let rewritten_rows: Vec<(String, String, i64)> = conn
            .prepare(
                "SELECT date, model, input_tokens
                 FROM agent_session_usage_rollups
                 WHERE app_type = 'opencode' AND session_id = 'ses_rewrite'",
            )
            .expect("prepare rewritten rollups")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query rewritten rollups")
            .collect::<Result<_, _>>()
            .expect("collect rewritten rollups");
        assert_eq!(
            rewritten_rows,
            vec![(expected_date, "rewritten-model".into(), 20)]
        );
        let raw: (i64, String, String, i64, i64, i64, i64, String, i64) = conn
            .query_row(
                "SELECT COUNT(*), model, request_model, input_tokens, output_tokens,
                        cache_read_tokens, cache_creation_tokens, total_cost_usd, created_at
                 FROM proxy_request_logs
                 WHERE request_id = 'opencode_session:ses_rewrite:rewritten-message'",
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
                        row.get(8)?,
                    ))
                },
            )
            .expect("rewritten raw compatibility row");
        assert_eq!(
            raw,
            (
                1,
                "rewritten-model".into(),
                "rewritten-model".into(),
                20,
                7,
                4,
                5,
                "0.2".into(),
                1_800_086_400,
            )
        );
        drop(conn);

        let mut repeated_result = SessionSyncResult::default();
        persist_opencode_session(
            &db,
            &session,
            &OpenCodeMessageQueryResult {
                messages: vec![rewritten],
                has_incomplete_usage: false,
            },
            &mut repeated_result,
        )
        .expect("repeat rewritten message");
        assert_eq!(repeated_result.imported, 0);
        assert_eq!(repeated_result.skipped, 1);
        let conn = db.conn.lock().expect("lock repeated rewrite database");
        let stable_counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs
                     WHERE request_id = 'opencode_session:ses_rewrite:rewritten-message'),
                    (SELECT COUNT(*) FROM agent_session_usage_rollups
                     WHERE app_type = 'opencode' AND session_id = 'ses_rewrite'),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage
                     WHERE app_type = 'opencode' AND data_source = 'opencode_session'
                       AND request_id = 'opencode_session:ses_rewrite:rewritten-message')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("stable rewritten counts");
        assert_eq!(stable_counts, (1, 1, 1));
        drop(conn);

        persist_opencode_session(
            &db,
            &session,
            &OpenCodeMessageQueryResult {
                messages: Vec::new(),
                has_incomplete_usage: false,
            },
            &mut SessionSyncResult::default(),
        )
        .expect("persist completed deletion");
        let conn = db.conn.lock().expect("lock deleted database");
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups
                 WHERE app_type = 'opencode' AND session_id = 'ses_rewrite'",
                [],
                |row| row.get(0),
            )
            .expect("remaining rollup count");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn sqlite_storage_wins_over_json_for_same_bare_session_id() {
        let selected = arbitrate_storage_sessions(
            vec![session_fixture("ses_same", OpenCodeStorage::Sqlite)],
            vec![session_fixture("ses_same", OpenCodeStorage::Json)],
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].session_id, "ses_same");
        assert_eq!(selected[0].storage, OpenCodeStorage::Sqlite);
    }

    #[test]
    fn json_completed_and_in_progress_messages_are_parsed_from_anonymous_fixture() {
        let temp = tempdir().expect("tempdir");
        let message_dir = temp.path().join("message").join("ses_json");
        std::fs::create_dir_all(&message_dir).expect("message directory");
        std::fs::write(
            message_dir.join("m_done.json"),
            serde_json::json!({
                "id": "m_done",
                "role": "assistant",
                "tokens": {
                    "input": 11,
                    "output": 2,
                    "reasoning": 1,
                    "cache": { "read": 3, "write": 4 }
                },
                "modelID": "fixture-model",
                "time": { "created": 1_800_000_000_000_i64, "completed": 1 }
            })
            .to_string(),
        )
        .expect("completed fixture");
        std::fs::write(
            message_dir.join("m_wip.json"),
            serde_json::json!({
                "id": "m_wip",
                "role": "assistant",
                "tokens": {
                    "input": 99,
                    "output": 0,
                    "reasoning": 0,
                    "cache": { "read": 0, "write": 0 }
                },
                "modelID": "fixture-model",
                "time": { "created": 1_800_000_000_100_i64 }
            })
            .to_string(),
        )
        .expect("in-progress fixture");

        let result = query_json_assistant_messages(temp.path(), "ses_json").expect("parse JSON");
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].0, "m_done");
        assert!(result.has_incomplete_usage);
    }

    #[test]
    fn partial_tokens_are_skipped_and_cursor_is_retained_until_complete() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("opencode.db");
        let sqlite = rusqlite::Connection::open(&db_path).expect("sqlite fixture");
        sqlite
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    directory TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    time_updated INTEGER NOT NULL
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    time_updated INTEGER NOT NULL,
                    data TEXT NOT NULL
                );",
            )
            .expect("schema");

        let partial_message = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 10, "output": 2, "reasoning": 1 },
            "cost": 0.1,
            "modelID": "fixture-model",
            "time": { "created": 1_800_000_000_000_i64, "completed": 1 }
        })
        .to_string();
        sqlite
            .execute(
                "INSERT INTO session VALUES ('ses_partial', 'Partial', '/fixture', ?1, ?2)",
                rusqlite::params![1_800_000_000_000_i64, 200_i64],
            )
            .expect("session row");
        sqlite
            .execute(
                "INSERT INTO message VALUES ('m_partial', 'ses_partial', ?1, ?2, ?3)",
                rusqlite::params![1_800_000_000_000_i64, 200_i64, partial_message],
            )
            .expect("partial message row");
        drop(sqlite);

        let app_db = Database::memory().expect("app database");
        let json_storage = temp.path().join("storage");
        let first = sync_opencode_usage_from_sources(&app_db, &db_path, &json_storage)
            .expect("partial sync");
        assert_eq!(first.imported, 0);
        assert!(first.errors.is_empty());
        let conn = app_db
            .conn
            .lock()
            .expect("lock anonymous OpenCode database");
        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs),
                    (SELECT COUNT(*) FROM agent_session_usage_rollups),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("partial counts");
        assert_eq!(counts, (0, 0, 0));
        drop(conn);

        let source_key = format!("{}:ses_partial", db_path.to_string_lossy());
        assert_eq!(get_sync_state(&app_db, &source_key).unwrap().0, 0);

        let complete_message = serde_json::json!({
            "role": "assistant",
            "tokens": {
                "input": 10,
                "output": 2,
                "reasoning": 1,
                "cache": { "read": 3, "write": 4 }
            },
            "cost": 0.1,
            "modelID": "fixture-model",
            "time": { "created": 1_800_000_000_000_i64, "completed": 1 }
        })
        .to_string();
        let sqlite = rusqlite::Connection::open(&db_path).expect("sqlite replacement");
        sqlite
            .execute(
                "UPDATE message SET time_updated = ?1, data = ?2 WHERE id = 'm_partial'",
                rusqlite::params![400_i64, complete_message],
            )
            .expect("complete message replacement");
        sqlite
            .execute(
                "UPDATE session SET time_updated = ?1 WHERE id = 'ses_partial'",
                rusqlite::params![400_i64],
            )
            .expect("session watermark replacement");
        drop(sqlite);

        let second = sync_opencode_usage_from_sources(&app_db, &db_path, &json_storage)
            .expect("complete replacement sync");
        assert_eq!(second.imported, 1);
        assert!(second.errors.is_empty());
        let conn = app_db
            .conn
            .lock()
            .expect("lock anonymous OpenCode database");
        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs),
                    (SELECT COUNT(*) FROM agent_session_usage_rollups),
                    (SELECT COUNT(*) FROM agent_session_canonical_coverage)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("complete counts");
        assert_eq!(counts, (1, 1, 1));
        drop(conn);
        assert_eq!(get_sync_state(&app_db, &source_key).unwrap().0, 400);
    }
}
