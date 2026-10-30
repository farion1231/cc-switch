//! Session usage importer for agents that persist Pi's session JSONL format:
//! Pi (pi.dev) and Oh My Pi (omp.sh).
//!
//! They are separate products with separate session stores; only the on-disk
//! format is shared. This importer keeps direct (non-proxy) usage visible in
//! the shared dashboard, one app at a time: each keeps its own `app_type`,
//! `data_source`, placeholder provider and sync cursors (see [`Flavor`]).

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    estimated_latency_ms, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::find_model_pricing;
use rust_decimal::Decimal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const UNKNOWN_MODEL: &str = "unknown";
const MAX_USAGE_LABEL_BYTES: usize = 512;
const MIN_SQLITE_UNIX_MILLIS: i64 = -62_167_219_200_000;
const MAX_SQLITE_UNIX_MILLIS: i64 = 253_402_300_799_999;
const REVISION_TAIL_BYTES: u64 = 4096;
const REVISION_MARKER_SHIFT: u32 = 61;
const REVISION_COMPLETE_SHIFT: u32 = 60;
const REVISION_SIZE_SHIFT: u32 = 32;
// Keep the original cursor format so existing history stays incremental.
const REVISION_MARKER: u64 = 0b101;
// Earlier builds of the timing fix wrote this tag with the same cursor layout.
const COMPAT_REVISION_MARKER: u64 = 0b110;
const REVISION_SIZE_MASK: u64 = (1 << 28) - 1;
const PI_REQUEST_DEDUP_SQL: &str = "SELECT EXISTS(
         SELECT 1 FROM session_usage_dedup
         WHERE data_source = ?1 AND request_id = ?2
     )";
const PI_SEMANTIC_DEDUP_SQL: &str = "SELECT EXISTS(
         SELECT 1 FROM session_usage_dedup
         WHERE data_source = ?1 AND semantic_id = ?2
     )";
const PI_LEGACY_SEMANTIC_DEDUP_SQL: &str = "SELECT EXISTS(
         SELECT 1 FROM session_usage_dedup
         WHERE data_source = ?1 AND semantic_id = ?2 AND has_entry_id = 0
     )";

/// Everything that differs between the agents writing the same session format.
///
/// The identity fields carry the app's own names end to end: `app_type` is the
/// dashboard's filter value, `data_source` keys the dedup ledger and the sync
/// cursors, and `provider_placeholder` is the provider row shown for entries
/// that name none. Mixing them up would fold one agent's usage into another's,
/// so each app module declares exactly one flavor from its own constants.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Flavor {
    /// Dashboard app filter, also `proxy_request_logs.app_type`.
    pub(crate) app_type: &'static str,
    /// `proxy_request_logs.data_source` and `session_usage_dedup.data_source`.
    pub(crate) data_source: &'static str,
    /// Placeholder `provider_id` for rows the entry does not attribute.
    pub(crate) provider_placeholder: &'static str,
    /// Reserved `session_log_sync` row that stores this app's child cutoff,
    /// for apps whose parent sessions project nested session usage (Pi's
    /// pi-subagents). `None`: nested session files are the only record of
    /// their usage, so they always count.
    pub(crate) cutoff_key: Option<&'static str>,
    /// Prefix on the synthesized `request_id` and `semantic_id` hashes.
    pub(crate) id_prefix: &'static str,
    /// Hash domain separator. Pi's value is frozen: the dedup ledger outlives
    /// pruned detail rows, so changing it would re-import history.
    pub(crate) hash_tag: &'static str,
    /// Log and error label: the app name, as the user knows it.
    pub(crate) label: &'static str,
    /// Whether Oh My Pi's `model_usage` entries carry usage. They record calls
    /// made outside the conversation: auto-thinking judges, job widgets.
    pub(crate) model_usage_carrier: bool,
    /// Whether a fixed-width `{"type":"title"}` slot may precede the session
    /// header. Only Oh My Pi writes one.
    pub(crate) title_slot: bool,
    pub(crate) session_files: fn() -> Result<Vec<PathBuf>, String>,
}

pub(crate) const PI: Flavor = Flavor {
    app_type: "pi",
    data_source: "pi_session",
    provider_placeholder: "_pi_session",
    cutoff_key: Some("pi:child-cutoff"),
    id_prefix: "pi_session",
    hash_tag: "pi-session",
    label: "Pi",
    model_usage_carrier: false,
    title_slot: false,
    session_files: crate::session_manager::providers::pi::session_files,
};

#[derive(Debug, Clone, Copy, Default)]
struct PiCosts {
    input: Decimal,
    output: Decimal,
    cache_read: Decimal,
    cache_write: Decimal,
    total: Decimal,
}

impl PiCosts {
    fn reported(self) -> Option<(Decimal, Decimal, Decimal, Decimal, Decimal)> {
        let component_total = self.input + self.output + self.cache_read + self.cache_write;
        let total = if self.total > Decimal::ZERO {
            self.total
        } else {
            component_total
        };
        (total > Decimal::ZERO).then_some((
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            total,
        ))
    }
}

#[derive(Debug)]
struct PiUsageRecord {
    request_id: String,
    semantic_id: String,
    has_entry_id: bool,
    provider_id: String,
    model: String,
    request_model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    costs: PiCosts,
    latency_ms: i64,
    status_code: i64,
    error_message: Option<String>,
    created_at: i64,
    session_id: String,
}

#[derive(Debug)]
struct ParsedPiFile {
    records: Vec<PiUsageRecord>,
    last_complete_line: i64,
    incomplete_tail: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PiFileRevision {
    modified_nanos: i64,
    file_size: u64,
    tail_fingerprint: u32,
    complete: bool,
}

impl PiFileRevision {
    fn encoded(self) -> i64 {
        ((REVISION_MARKER << REVISION_MARKER_SHIFT)
            | (u64::from(self.complete) << REVISION_COMPLETE_SHIFT)
            | (self.file_size << REVISION_SIZE_SHIFT)
            | u64::from(self.tail_fingerprint)) as i64
    }
}

#[derive(Debug, Clone, Copy)]
struct PiSyncState {
    revision: PiFileRevision,
    last_line_offset: i64,
}

#[derive(Debug)]
struct PiRequestIdentity {
    request_id: String,
    semantic_id: String,
    has_entry_id: bool,
}

#[derive(Default)]
struct ChildScope {
    /// Canonical paths of every file in this round.
    imported: HashSet<PathBuf>,
    /// Extension-less session paths; a file below one of them is a nested subagent or fork session.
    stems: HashSet<PathBuf>,
    /// Unix seconds of this build's first import; `i64::MIN` when the flavor has no cutoff.
    cutoff: i64,
}

impl ChildScope {
    fn new(files: &[PathBuf], cutoff: i64) -> Self {
        Self {
            imported: files
                .iter()
                .filter_map(|file| fs::canonicalize(file).ok())
                .collect(),
            stems: files.iter().map(|file| file.with_extension("")).collect(),
            cutoff,
        }
    }

    fn is_nested(&self, file: &Path) -> bool {
        file.ancestors().skip(1).any(|dir| self.stems.contains(dir))
    }
}

/// Import usage from every Pi session file under the session browser's current
/// root and layout rules, including nested subagent sessions.
pub fn sync_pi_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_flavor(db, &PI)
}

pub(crate) fn sync_flavor(
    db: &Database,
    flavor: &'static Flavor,
) -> Result<SessionSyncResult, AppError> {
    let files = (flavor.session_files)()
        .map_err(|error| AppError::Config(format!("无法发现 {} 会话: {error}", flavor.label)))?;
    Ok(sync_pi_files(db, flavor, &files))
}

pub(crate) fn sync_pi_files(
    db: &Database,
    flavor: &'static Flavor,
    files: &[PathBuf],
) -> SessionSyncResult {
    let mut result = SessionSyncResult {
        files_scanned: files.len().min(u32::MAX as usize) as u32,
        ..Default::default()
    };

    // 游标预取失败必须中止本轮（不能当空表全量重导，见 load_sync_cursors 文档）
    let cursors = match crate::services::session_usage::load_sync_cursors(db) {
        Ok(cursors) => cursors,
        Err(error) => {
            result.errors.push(format!("预取同步游标失败: {error}"));
            return result;
        }
    };

    let cutoff = match child_cutoff(db, flavor, &cursors) {
        Ok(cutoff) => cutoff,
        Err(error) => {
            result
                .errors
                .push(format!("记录 {} 子会话起算时间失败: {error}", flavor.label));
            return result;
        }
    };
    let scope = ChildScope::new(files, cutoff);
    for file_path in files {
        match sync_single_pi_file(db, flavor, file_path, &cursors, &scope) {
            Ok(file_result) => result.merge(file_result),
            Err(error) => {
                let message = format!("{}: {error}", file_path.display());
                log::warn!("[{}-SYNC] 会话文件解析失败: {message}", flavor.label);
                result.errors.push(message);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[{}-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            flavor.label,
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }
    result
}

fn child_cutoff(
    db: &Database,
    flavor: &'static Flavor,
    cursors: &std::collections::HashMap<String, crate::services::session_usage::SyncCursor>,
) -> Result<i64, AppError> {
    let Some(cutoff_key) = flavor.cutoff_key else {
        return Ok(i64::MIN);
    };
    if let Some(cursor) = cursors.get(cutoff_key) {
        return Ok(cursor.last_synced_at);
    }
    let now = chrono::Utc::now().timestamp();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, 0, 0, ?2)",
        rusqlite::params![cutoff_key, now],
    )
    .map_err(|error| {
        AppError::Database(format!("写入 {} 子会话起算时间失败: {error}", flavor.label))
    })?;
    Ok(now)
}

fn sync_single_pi_file(
    db: &Database,
    flavor: &'static Flavor,
    file_path: &Path,
    cursors: &std::collections::HashMap<String, crate::services::session_usage::SyncCursor>,
    scope: &ChildScope,
) -> Result<SessionSyncResult, AppError> {
    let metadata = fs::symlink_metadata(file_path).map_err(|error| {
        AppError::Config(format!("无法读取 {} 会话文件元数据: {error}", flavor.label))
    })?;
    if !metadata.file_type().is_file()
        || file_path.extension().and_then(|value| value.to_str()) != Some("jsonl")
    {
        return Err(AppError::Config(format!(
            "{} 会话路径不是普通 JSONL 文件",
            flavor.label
        )));
    }
    if metadata.len() > crate::session_manager::providers::pi::MAX_SESSION_BYTES {
        return Err(AppError::Config(format!(
            "{} 会话文件超过 {} 字节安全上限",
            flavor.label,
            crate::session_manager::providers::pi::MAX_SESSION_BYTES
        )));
    }

    let file_path_string = file_path.to_string_lossy().to_string();
    let modified = metadata_modified_nanos(&metadata);
    let revision = pi_file_revision(flavor, file_path, &metadata, modified)?;
    let previous = decode_pi_sync_state(cursors.get(&file_path_string));
    if previous.is_some_and(|state| state.revision == revision) {
        return Ok(SessionSyncResult::default());
    }

    // A matching tail at the old EOF identifies Pi's normal append path, so
    // active sessions can seek straight to appended JSONL. Any mismatch is a
    // rewrite and must rescan from the header; the durable request ledger
    // makes that safe.
    let (start_after_line, start_at_byte) = match previous {
        Some(state)
            if state.revision.complete
                && revision.file_size > state.revision.file_size
                && pi_prefix_tail_matches(flavor, file_path, state.revision)? =>
        {
            (state.last_line_offset, Some(state.revision.file_size))
        }
        Some(_) | None => (0, None),
    };
    let parsed = parse_pi_file(
        flavor,
        file_path,
        start_after_line,
        start_at_byte,
        revision.file_size,
        modified,
        scope,
    )?;
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction().map_err(|error| {
        AppError::Database(format!("启动 {} 用量导入事务失败: {error}", flavor.label))
    })?;
    let mut result = SessionSyncResult::default();
    for record in &parsed.records {
        if insert_pi_record(&tx, flavor, record)? {
            result.imported = result.imported.saturating_add(1);
        } else {
            result.skipped = result.skipped.saturating_add(1);
        }
    }

    update_pi_sync_state_on_conn(
        &tx,
        flavor,
        &file_path_string,
        revision,
        parsed.last_complete_line,
    )?;
    tx.commit().map_err(|error| {
        AppError::Database(format!("提交 {} 用量导入事务失败: {error}", flavor.label))
    })?;
    if parsed.incomplete_tail {
        result.deferred_files = 1;
    }
    Ok(result)
}

/// 从批量预取的游标解码 Pi 同步状态（revision 编码在 `last_synced_at`）。
fn decode_pi_sync_state(
    cursor: Option<&crate::services::session_usage::SyncCursor>,
) -> Option<PiSyncState> {
    let cursor = cursor?;
    let last_line_offset = cursor.last_line_offset;
    let encoded_revision = cursor.last_synced_at as u64;
    let marker = encoded_revision >> REVISION_MARKER_SHIFT;
    if marker != REVISION_MARKER && marker != COMPAT_REVISION_MARKER {
        return None;
    }
    let file_size = (encoded_revision >> REVISION_SIZE_SHIFT) & REVISION_SIZE_MASK;
    if file_size > crate::session_manager::providers::pi::MAX_SESSION_BYTES
        || last_line_offset < 0
        || last_line_offset > crate::session_manager::providers::pi::MAX_TREE_ENTRIES as i64 + 1
    {
        return None;
    }
    Some(PiSyncState {
        revision: PiFileRevision {
            modified_nanos: cursor.last_modified,
            file_size,
            tail_fingerprint: encoded_revision as u32,
            complete: ((encoded_revision >> REVISION_COMPLETE_SHIFT) & 1) == 1,
        },
        last_line_offset,
    })
}

fn update_pi_sync_state_on_conn(
    conn: &rusqlite::Connection,
    flavor: &'static Flavor,
    file_path: &str,
    revision: PiFileRevision,
    last_line_offset: i64,
) -> Result<(), AppError> {
    update_sync_state_on_conn(conn, file_path, revision.modified_nanos, last_line_offset)?;
    // No schema expansion is needed for Pi's append proof. This tagged value
    // is private to Pi rows; no production consumer interprets last_synced_at.
    conn.execute(
        "UPDATE session_log_sync SET last_synced_at = ?2 WHERE file_path = ?1",
        rusqlite::params![file_path, revision.encoded()],
    )
    .map_err(|error| {
        AppError::Database(format!("更新 {} 会话同步状态失败: {error}", flavor.label))
    })?;
    Ok(())
}

fn pi_file_revision(
    flavor: &'static Flavor,
    file_path: &Path,
    metadata: &fs::Metadata,
    modified_nanos: i64,
) -> Result<PiFileRevision, AppError> {
    let tail_len = metadata.len().min(REVISION_TAIL_BYTES);
    let mut tail = vec![0; tail_len as usize];
    if tail_len > 0 {
        let mut file = File::open(file_path).map_err(|error| {
            AppError::Config(format!("无法打开 {} 会话文件: {error}", flavor.label))
        })?;
        file.seek(SeekFrom::Start(metadata.len() - tail_len))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| {
                AppError::Config(format!("无法读取 {} 会话文件尾部: {error}", flavor.label))
            })?;
    }

    let complete = tail.last() == Some(&b'\n');
    let tail_fingerprint = pi_tail_fingerprint(&tail);
    Ok(PiFileRevision {
        modified_nanos,
        file_size: metadata.len(),
        tail_fingerprint,
        complete,
    })
}

fn pi_prefix_tail_matches(
    flavor: &'static Flavor,
    file_path: &Path,
    previous: PiFileRevision,
) -> Result<bool, AppError> {
    let tail_len = previous.file_size.min(REVISION_TAIL_BYTES);
    let mut tail = vec![0; tail_len as usize];
    if tail_len > 0 {
        let mut file = File::open(file_path).map_err(|error| {
            AppError::Config(format!("无法打开 {} 会话文件: {error}", flavor.label))
        })?;
        file.seek(SeekFrom::Start(previous.file_size - tail_len))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| {
                AppError::Config(format!("无法校验 {} 会话追加边界: {error}", flavor.label))
            })?;
    }
    Ok(pi_tail_fingerprint(&tail) == previous.tail_fingerprint)
}

fn pi_tail_fingerprint(tail: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"pi-session-tail-v1");
    hash_field(&mut hasher, tail);
    let digest = hasher.finalize();
    u32::from_be_bytes(digest[..4].try_into().unwrap_or_default())
}

fn parse_pi_file(
    flavor: &'static Flavor,
    file_path: &Path,
    start_after_line: i64,
    start_at_byte: Option<u64>,
    snapshot_size: u64,
    file_modified_nanos: i64,
    scope: &ChildScope,
) -> Result<ParsedPiFile, AppError> {
    let file = File::open(file_path).map_err(|error| {
        AppError::Config(format!("无法打开 {} 会话文件: {error}", flavor.label))
    })?;
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    let mut line_number = 0i64;
    let mut bytes_read = 0u64;
    let mut session_id = None;
    let mut session_timestamp = None;
    let mut records = Vec::new();
    let mut incomplete_tail = false;
    let nested = scope.is_nested(file_path);

    loop {
        buffer.clear();
        let remaining = snapshot_size.saturating_sub(bytes_read);
        if remaining == 0 {
            break;
        }
        let read = Read::by_ref(&mut reader)
            .take(remaining)
            .read_line(&mut buffer)
            .map_err(|error| {
                AppError::Config(format!("无法读取 {} 会话文件: {error}", flavor.label))
            })?;
        if read == 0 {
            return Err(AppError::Config(format!(
                "{} 会话文件在读取期间被截断",
                flavor.label
            )));
        }
        bytes_read = bytes_read.saturating_add(read as u64);
        if bytes_read > crate::session_manager::providers::pi::MAX_SESSION_BYTES {
            return Err(AppError::Config(format!(
                "{} 会话文件读取时超过安全上限",
                flavor.label
            )));
        }
        let has_newline = buffer.ends_with('\n');
        let line = buffer.trim();
        let value = if line.is_empty() {
            None
        } else {
            match serde_json::from_str::<Value>(line) {
                Ok(value) => Some(value),
                Err(_) if !has_newline => {
                    incomplete_tail = true;
                    break;
                }
                Err(_) => None,
            }
        };
        line_number = line_number.saturating_add(1);
        if line_number > crate::session_manager::providers::pi::MAX_TREE_ENTRIES as i64 + 1 {
            return Err(AppError::Config(format!(
                "{} 会话超过 {} 条 entry 安全上限",
                flavor.label,
                crate::session_manager::providers::pi::MAX_TREE_ENTRIES
            )));
        }
        if session_id.is_some() && line_number <= start_after_line {
            continue;
        }
        let Some(value) = value else {
            if !has_newline {
                incomplete_tail = true;
                break;
            }
            continue;
        };

        if session_id.is_none() {
            let entry_type = value.get("type").and_then(Value::as_str);
            // Oh My Pi opens with a fixed-width `{"type":"title"}` slot before
            // the header. It carries no usage; skip it and keep looking.
            if flavor.title_slot && entry_type == Some("title") {
                continue;
            }
            if entry_type != Some("session") {
                return Err(AppError::Config(format!(
                    "{} 会话的首条有效 JSON 不是 session header",
                    flavor.label
                )));
            }
            session_id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| crate::session_manager::providers::pi::is_valid_tree_id(id))
                .map(str::to_string);
            if session_id.is_none() {
                return Err(AppError::Config(format!(
                    "{} 会话 header 缺少 id",
                    flavor.label
                )));
            }
            let header_timestamp_millis = value.get("timestamp").and_then(parse_timestamp_millis);
            session_timestamp = header_timestamp_millis.map(|timestamp| timestamp / 1000);
            if let Some(byte_offset) = start_at_byte.filter(|offset| *offset >= bytes_read) {
                reader.seek(SeekFrom::Start(byte_offset)).map_err(|error| {
                    AppError::Config(format!("无法定位 {} 会话增量边界: {error}", flavor.label))
                })?;
                bytes_read = byte_offset;
                line_number = start_after_line;
            }
            continue;
        }
        let Some(record) = parse_usage_record(
            flavor,
            &value,
            session_id.as_deref().unwrap_or_default(),
            session_timestamp,
            file_modified_nanos / 1_000_000_000,
        ) else {
            continue;
        };
        // Before the cutoff the parent's projections carry child usage; from it on, the child files do.
        let from_cutoff = record.created_at >= scope.cutoff;
        if (nested && !from_cutoff)
            || (from_cutoff && duplicates_imported_sessions(&value, &scope.imported))
        {
            continue;
        }
        records.push(record);
    }

    if session_id.is_none() && !incomplete_tail {
        return Err(AppError::Config(format!(
            "{} 会话没有有效 header",
            flavor.label
        )));
    }
    Ok(ParsedPiFile {
        records,
        last_complete_line: line_number,
        incomplete_tail,
    })
}

/// A pi-subagents projection sums the usage of the child sessions it links.
fn duplicates_imported_sessions(entry: &Value, imported_sessions: &HashSet<PathBuf>) -> bool {
    let links = projection_session_files(entry);
    !links.is_empty()
        && links
            .into_iter()
            .all(|link| fs::canonicalize(link).is_ok_and(|path| imported_sessions.contains(&path)))
}

fn projection_session_files(entry: &Value) -> Vec<&str> {
    let mut links = Vec::new();
    let message = entry.get("message").filter(|message| {
        entry.get("type").and_then(Value::as_str) == Some("message")
            && message.get("role").and_then(Value::as_str) == Some("toolResult")
            && message.get("usage").is_some()
    });
    if let Some(details) = message.and_then(|message| message.get("details")) {
        collect_session_files(details, &mut links);
    }
    links
}

fn collect_session_files<'a>(value: &'a Value, links: &mut Vec<&'a str>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match field {
                    Value::String(link) if key == "sessionFile" => links.push(link),
                    _ => collect_session_files(field, links),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_session_files(item, links);
            }
        }
        _ => {}
    }
}

fn parse_usage_record(
    flavor: &'static Flavor,
    entry: &Value,
    session_id: &str,
    session_timestamp: Option<i64>,
    file_timestamp: i64,
) -> Option<PiUsageRecord> {
    let entry_type = entry.get("type").and_then(Value::as_str)?;
    // `carrier` holds the provider/model/stopReason/errorMessage fields: the
    // nested `message` for message entries, the entry itself for the carriers
    // Oh My Pi writes at top level.
    let (kind, usage_value, carrier) = match entry_type {
        "message" => {
            let message = entry.get("message")?;
            match message.get("role").and_then(Value::as_str) {
                Some("assistant") => ("assistant", message.get("usage")?, Some(message)),
                Some("toolResult") => ("tool_result", message.get("usage")?, Some(message)),
                _ => return None,
            }
        }
        "compaction" => ("compaction", entry.get("usage")?, None),
        "branch_summary" => ("branch_summary", entry.get("usage")?, None),
        // Oh My Pi records calls outside the conversation — auto-thinking
        // judges, job widgets, completion probes — here, with the request
        // metadata at the entry's top level.
        "model_usage" if flavor.model_usage_carrier => {
            ("model_usage", entry.get("usage")?, Some(entry))
        }
        _ => return None,
    };
    let attributed = matches!(kind, "assistant" | "model_usage");

    let event_timestamp_millis = entry
        .get("timestamp")
        .and_then(parse_timestamp_millis)
        .or_else(|| {
            carrier
                .and_then(|value| value.get("timestamp"))
                .and_then(parse_timestamp_millis)
        });
    let input_tokens = token_count(usage_value, "input");
    let output_tokens = token_count(usage_value, "output");
    let cache_read_tokens = token_count(usage_value, "cacheRead");
    let cache_write_tokens = token_count(usage_value, "cacheWrite");
    let costs = parse_costs(usage_value.get("cost"));
    let stop_reason = attributed
        .then(|| carrier.and_then(|value| nonempty_string(value.get("stopReason"))))
        .flatten();
    let failed = matches!(stop_reason, Some("error" | "aborted"));
    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
        && costs.reported().is_none()
        && !failed
    {
        return None;
    }

    let (provider_id, model, request_model) = if attributed {
        let carrier = carrier?;
        let provider = bounded_label(carrier.get("provider"), flavor.provider_placeholder);
        let requested = bounded_label(carrier.get("model"), UNKNOWN_MODEL);
        let actual = nonempty_string(carrier.get("responseModel"))
            .map(truncate_usage_label)
            .unwrap_or(&requested)
            .to_string();
        (provider, actual, requested)
    } else {
        (
            flavor.provider_placeholder.to_string(),
            UNKNOWN_MODEL.to_string(),
            UNKNOWN_MODEL.to_string(),
        )
    };

    let created_at = event_timestamp_millis
        .map(|timestamp| timestamp / 1000)
        .or(session_timestamp)
        .unwrap_or(file_timestamp)
        .clamp(MIN_SQLITE_UNIX_MILLIS / 1000, MAX_SQLITE_UNIX_MILLIS / 1000);

    let (status_code, error_message) = if attributed {
        match stop_reason {
            Some("error") | Some("aborted") => {
                let fallback = if stop_reason == Some("aborted") {
                    format!("{} request aborted", flavor.label)
                } else {
                    format!("{} request failed", flavor.label)
                };
                let error: String = carrier
                    .and_then(|value| nonempty_string(value.get("errorMessage")))
                    .unwrap_or(&fallback)
                    .chars()
                    .take(4096)
                    .collect();
                (
                    if stop_reason == Some("aborted") {
                        499
                    } else {
                        500
                    },
                    Some(error),
                )
            }
            _ => (200, None),
        }
    } else {
        (200, None)
    };

    let identity = pi_request_identity(flavor, entry, kind, usage_value, carrier);

    // Oh My Pi writes the measured request duration (`duration`, milliseconds,
    // first-token wait included) on the call itself. Pi initializes an assistant message's timestamp
    // before the request and appends the entry on message_end, so there the
    // difference between the two is an estimate that includes first-token
    // waiting; other usage carriers have no equivalent request-start timestamp.
    let latency_ms = if attributed {
        carrier
            .and_then(|value| value.get("duration"))
            .and_then(Value::as_f64)
            .filter(|duration| *duration > 0.0)
            .and_then(|duration| estimated_latency_ms(0, duration.round() as i64))
            .or_else(|| {
                let start = carrier?.get("timestamp").and_then(parse_timestamp_millis)?;
                let end = entry.get("timestamp").and_then(parse_timestamp_millis)?;
                estimated_latency_ms(start, end)
            })
            .unwrap_or(0)
    } else {
        0
    };

    Some(PiUsageRecord {
        request_id: identity.request_id,
        semantic_id: identity.semantic_id,
        has_entry_id: identity.has_entry_id,
        provider_id,
        model,
        request_model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        costs,
        latency_ms,
        status_code,
        error_message,
        created_at,
        session_id: session_id.to_string(),
    })
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn bounded_label(value: Option<&Value>, fallback: &str) -> String {
    truncate_usage_label(nonempty_string(value).unwrap_or(fallback)).to_string()
}

fn truncate_usage_label(value: &str) -> &str {
    if value.len() <= MAX_USAGE_LABEL_BYTES {
        return value;
    }
    let mut end = MAX_USAGE_LABEL_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn token_count(usage: &Value, key: &str) -> u32 {
    usage
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

fn parse_costs(value: Option<&Value>) -> PiCosts {
    let decimal = |key| {
        value
            .and_then(|cost| cost.get(key))
            .and_then(parse_decimal)
            .unwrap_or(Decimal::ZERO)
            .max(Decimal::ZERO)
    };
    PiCosts {
        input: decimal("input"),
        output: decimal("output"),
        cache_read: decimal("cacheRead"),
        cache_write: decimal("cacheWrite"),
        total: decimal("total"),
    }
}

fn parse_decimal(value: &Value) -> Option<Decimal> {
    let raw = match value {
        Value::Number(number) => number.to_string(),
        Value::String(value) => value.clone(),
        _ => return None,
    };
    Decimal::from_str(&raw)
        .or_else(|_| Decimal::from_scientific(&raw))
        .ok()
}

fn parse_timestamp_millis(value: &Value) -> Option<i64> {
    let timestamp = if let Some(timestamp) = value.as_i64() {
        if !(-100_000_000_000..=100_000_000_000).contains(&timestamp) {
            timestamp
        } else {
            timestamp.saturating_mul(1000)
        }
    } else {
        value
            .as_str()
            .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())?
            .timestamp_millis()
    };
    (MIN_SQLITE_UNIX_MILLIS..=MAX_SQLITE_UNIX_MILLIS)
        .contains(&timestamp)
        .then_some(timestamp)
}

fn pi_request_identity(
    flavor: &'static Flavor,
    entry: &Value,
    kind: &str,
    usage: &Value,
    message: Option<&Value>,
) -> PiRequestIdentity {
    let mut hasher = Sha256::new();
    // Pi's tag and version stay byte-identical: the ledger that makes the ids
    // unique is durable, so a new domain separator would re-import history.
    let semantic_tag = format!("{}-semantic-v1", flavor.hash_tag);
    hash_field(&mut hasher, semantic_tag.as_bytes());
    hash_field(&mut hasher, kind.as_bytes());

    for (label, value) in [
        (b"entry_timestamp".as_slice(), entry.get("timestamp")),
        (
            b"message_timestamp".as_slice(),
            message.and_then(|value| value.get("timestamp")),
        ),
    ] {
        if let Some(value) = value {
            hash_field(&mut hasher, label);
            hash_json(&mut hasher, value);
        }
    }
    if let Some(message) = message {
        for key in [
            "provider",
            "model",
            "responseModel",
            "responseId",
            "api",
            "toolCallId",
            "toolName",
            "stopReason",
            "errorMessage",
        ] {
            if let Some(value) = message.get(key) {
                hash_field(&mut hasher, key.as_bytes());
                hash_json(&mut hasher, value);
            }
        }
        if let Some(content) = message.get("content") {
            hash_field(&mut hasher, b"content");
            hash_json(&mut hasher, content);
        }
    } else if let Some(summary) = entry.get("summary") {
        hash_field(&mut hasher, b"summary");
        hash_json(&mut hasher, summary);
    }
    hash_field(&mut hasher, b"usage");
    hash_json(&mut hasher, usage);
    let semantic_id = format!("{}_semantic:{:x}", flavor.id_prefix, hasher.finalize());
    let entry_id = nonempty_string(entry.get("id"));
    let request_id = if let Some(entry_id) = entry_id {
        let request_tag = format!("{}-request-v3", flavor.hash_tag);
        let mut request_hasher = Sha256::new();
        hash_field(&mut request_hasher, request_tag.as_bytes());
        hash_field(&mut request_hasher, kind.as_bytes());
        hash_field(&mut request_hasher, entry_id.as_bytes());
        if let Some(timestamp) = entry.get("timestamp") {
            hash_json(&mut request_hasher, timestamp);
        }
        format!("{}:{:x}", flavor.id_prefix, request_hasher.finalize())
    } else {
        semantic_id.clone()
    };
    PiRequestIdentity {
        request_id,
        semantic_id,
        has_entry_id: entry_id.is_some(),
    }
}

fn hash_json(hasher: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash_field(hasher, b"null"),
        Value::Bool(value) => {
            hash_field(hasher, b"bool");
            hash_field(hasher, if *value { b"true" } else { b"false" });
        }
        Value::Number(value) => {
            hash_field(hasher, b"number");
            hash_field(hasher, value.to_string().as_bytes());
        }
        Value::String(value) => {
            hash_field(hasher, b"string");
            hash_field(hasher, value.as_bytes());
        }
        Value::Array(values) => {
            hash_field(hasher, b"array");
            hash_field(hasher, &(values.len() as u64).to_be_bytes());
            for value in values {
                hash_json(hasher, value);
            }
        }
        Value::Object(values) => {
            hash_field(hasher, b"object");
            hash_field(hasher, &(values.len() as u64).to_be_bytes());
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort_unstable();
            for key in keys {
                hash_field(hasher, key.as_bytes());
                hash_json(hasher, &values[key]);
            }
        }
    }
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn insert_pi_record(
    conn: &rusqlite::Connection,
    flavor: &'static Flavor,
    record: &PiUsageRecord,
) -> Result<bool, AppError> {
    let ledger_error = |action: &str, error: rusqlite::Error| {
        AppError::Database(format!(
            "{action} {} 用量去重账本失败: {error}",
            flavor.label
        ))
    };
    let request_seen: bool = conn
        .query_row(
            PI_REQUEST_DEDUP_SQL,
            rusqlite::params![flavor.data_source, record.request_id],
            |row| row.get(0),
        )
        .map_err(|error| ledger_error("查询", error))?;
    let already_seen = request_seen
        || conn
            .query_row(
                if record.has_entry_id {
                    PI_LEGACY_SEMANTIC_DEDUP_SQL
                } else {
                    PI_SEMANTIC_DEDUP_SQL
                },
                rusqlite::params![flavor.data_source, record.semantic_id],
                |row| row.get(0),
            )
            .map_err(|error| ledger_error("查询", error))?;
    if already_seen {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            flavor.data_source,
            record.request_id,
            record.semantic_id,
            i64::from(record.has_entry_id),
        ],
    )
    .map_err(|error| ledger_error("写入", error))?;

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_write_tokens,
        cache_creation_1h_tokens: 0,
        model: Some(record.model.clone()),
        message_id: None,
    };
    let costs = record.costs.reported().or_else(|| {
        find_model_pricing(conn, &record.model).map(|pricing| {
            let calculated =
                CostCalculator::calculate_for_app(flavor.app_type, &usage, &pricing, Decimal::ONE);
            (
                calculated.input_cost,
                calculated.output_cost,
                calculated.cache_read_cost,
                calculated.cache_creation_cost,
                calculated.total_cost,
            )
        })
    });
    let (input_cost, output_cost, cache_read_cost, cache_write_cost, total_cost) =
        costs.unwrap_or((
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        ));

    conn.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd,
            cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )",
        rusqlite::params![
            record.request_id,
            record.provider_id,
            flavor.app_type,
            record.model,
            record.request_model,
            record.model,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_write_tokens,
            INPUT_TOKEN_SEMANTICS_FRESH,
            input_cost.to_string(),
            output_cost.to_string(),
            cache_read_cost.to_string(),
            cache_write_cost.to_string(),
            total_cost.to_string(),
            record.latency_ms,
            Option::<i64>::None,
            record.status_code,
            record.error_message,
            record.session_id,
            Some(flavor.data_source),
            1i64,
            "1.0",
            record.created_at,
            flavor.data_source,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|error| AppError::Database(format!("插入 {} 会话用量失败: {error}", flavor.label)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::FileTimes;
    use std::io::Write;

    fn session_path(root: &Path, name: &str) -> PathBuf {
        root.join(format!("{name}.jsonl"))
    }

    fn write_lines(path: &Path, lines: &[&str]) {
        let mut file = File::create(path).expect("create session");
        for line in lines {
            writeln!(file, "{line}").expect("write session line");
        }
    }

    fn write_nested_lines(path: &Path, lines: &[&str]) {
        fs::create_dir_all(path.parent().expect("session parent")).expect("create session parent");
        write_lines(path, lines);
    }

    fn session_header(id: &str, parent: Option<&Path>) -> String {
        let mut header = json!({
            "type": "session",
            "version": 3,
            "id": id,
            "timestamp": "2023-11-14T22:13:20Z",
            "cwd": "/work",
        });
        if let Some(parent) = parent {
            header["parentSession"] = Value::from(parent.to_string_lossy());
        }
        header.to_string()
    }

    fn run_session(stem: &Path, run: &str) -> PathBuf {
        stem.join(run).join("run-0").join("session.jsonl")
    }

    fn projection_line(
        id: &str,
        timestamp: &str,
        tool_name: &str,
        details: Value,
        input: u32,
    ) -> String {
        json!({
            "type": "message",
            "id": id,
            "parentId": null,
            "timestamp": timestamp,
            "message": {
                "role": "toolResult",
                "toolCallId": format!("call-{id}"),
                "toolName": tool_name,
                "content": [{"type": "text", "text": "done"}],
                "details": details,
                "usage": {
                    "input": input,
                    "output": 2,
                    "cacheRead": 5,
                    "cacheWrite": 2,
                    "totalTokens": 999,
                    "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}
                }
            }
        })
        .to_string()
    }

    fn pi_rows_input_and_unknown(db: &Database) -> Result<(i64, i64, i64), AppError> {
        let conn = lock_conn!(db.conn);
        let totals = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(model = 'unknown'), 0)
             FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(totals)
    }

    fn set_child_cutoff(db: &Database, seconds: i64) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_synced_at)
             VALUES (?1, 0, 0, ?2)",
            rusqlite::params![PI.cutoff_key.expect("Pi cutoff key"), seconds],
        )?;
        Ok(())
    }

    fn assistant_line(id: &str, timestamp: &str, input: u32) -> String {
        format!(
            r#"{{"type":"message","id":"{id}","parentId":null,"timestamp":"{timestamp}","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"provider":"fixture-provider","model":"fixture-model","responseId":"reused-response-id","timestamp":1700000000000,"usage":{{"input":{input},"output":2,"cacheRead":5,"cacheWrite":2,"totalTokens":999,"cost":{{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}}},"stopReason":"stop"}}}}"#
        )
    }

    fn set_modified(path: &Path, modified: std::time::SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .expect("open session for timestamp restore")
            .set_times(FileTimes::new().set_modified(modified))
            .expect("restore session timestamp");
    }

    #[test]
    fn dedup_lookups_use_complete_identity_indexes() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        for (sql, expected) in [
            (PI_REQUEST_DEDUP_SQL, "(data_source=? AND request_id=?)"),
            (PI_SEMANTIC_DEDUP_SQL, "(data_source=? AND semantic_id=?)"),
            (
                PI_LEGACY_SEMANTIC_DEDUP_SQL,
                "(data_source=? AND semantic_id=? AND has_entry_id=?)",
            ),
        ] {
            let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let plan = statement
                .query_map(rusqlite::params![PI.data_source, "identity"], |row| {
                    row.get::<_, String>(3)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            assert!(
                plan.iter().any(|step| step.contains(expected)),
                "lookup does not constrain the complete identity {expected}: {plan:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn imports_all_pi_usage_carriers_with_source_semantics() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "all-carriers");
        write_lines(
            &path,
            &[
                r#"{"type":"session","version":3,"id":"session-a","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                r#"{"type":"message","id":"user","parentId":null,"timestamp":"2023-11-14T22:13:20Z","message":{"role":"user","content":"question"}}"#,
                r#"{"type":"message","id":"assistant","parentId":"user","timestamp":"2023-11-14T22:13:21Z","message":{"role":"assistant","content":[{"type":"text","text":"answer"}],"provider":"custom-pi","model":"requested-model","responseModel":"actual-model","responseId":"response-1","timestamp":1700000000000,"usage":{"input":10,"output":7,"cacheRead":5,"cacheWrite":2,"reasoning":3,"totalTokens":23,"cost":{"input":0.00001,"output":0.000014,"cacheRead":5E-7,"cacheWrite":0.000001,"total":0.0000255}},"stopReason":"stop"}}"#,
                r#"{"type":"message","id":"tool","parentId":"assistant","timestamp":"2023-11-14T22:13:22Z","message":{"role":"toolResult","toolCallId":"tool-1","toolName":"nested","content":[],"usage":{"input":3,"output":4,"cacheRead":1,"cacheWrite":1,"totalTokens":9,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}}}"#,
                r#"{"type":"compaction","id":"compact","parentId":"tool","timestamp":"2023-11-14T22:13:23Z","summary":"summary","usage":{"input":11,"output":12,"cacheRead":2,"cacheWrite":3,"totalTokens":28,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"details":{"retainedTail":[{"type":"message","message":{"role":"assistant","usage":{"input":999,"output":999,"cacheRead":999,"cacheWrite":999}}}]}}"#,
                r#"{"type":"branch_summary","id":"branch","parentId":"compact","timestamp":"2023-11-14T22:13:24Z","summary":"branch summary","usage":{"input":13,"output":14,"cacheRead":4,"cacheWrite":5,"totalTokens":36,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}}"#,
                r#"{"type":"message","id":"empty-error","parentId":"branch","timestamp":"2023-11-14T22:13:25Z","message":{"role":"assistant","provider":"custom-pi","model":"actual-model","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":"error"}}"#,
            ],
        );

        let db = Database::memory()?;
        let result = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(result.imported, 5);
        assert!(result.errors.is_empty());

        {
            let conn = lock_conn!(db.conn);
            let totals: (i64, i64, i64, i64, i64) = conn.query_row(
                "SELECT COUNT(*), SUM(input_tokens), SUM(output_tokens),
                        SUM(cache_read_tokens), SUM(cache_creation_tokens)
                 FROM proxy_request_logs WHERE data_source = 'pi_session'",
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
            assert_eq!(totals, (5, 37, 37, 12, 11));

            let assistant: (String, String, String, String, i64, i64, String) = conn.query_row(
                "SELECT provider_id, model, request_model, pricing_model, created_at,
                        input_token_semantics, total_cost_usd
                 FROM proxy_request_logs
                 WHERE provider_id = 'custom-pi' AND status_code = 200",
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
                    ))
                },
            )?;
            assert_eq!(assistant.0, "custom-pi");
            assert_eq!(assistant.1, "actual-model");
            assert_eq!(assistant.2, "requested-model");
            assert_eq!(assistant.3, "actual-model");
            assert_eq!(assistant.4, 1_700_000_001);
            assert_eq!(assistant.5, INPUT_TOKEN_SEMANTICS_FRESH);
            assert_eq!(
                Decimal::from_str(&assistant.6).expect("reported total"),
                Decimal::from_str("0.0000255").expect("expected total")
            );

            let empty_failure: (i64, i64, String) = conn.query_row(
                "SELECT status_code, input_tokens, error_message
                 FROM proxy_request_logs
                 WHERE provider_id = 'custom-pi' AND status_code = 500",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(empty_failure, (500, 0, "Pi request failed".to_string()));
        }

        let providers = db.get_provider_stats(None, None, Some("pi"), None, None)?;
        assert!(providers.iter().any(|provider| {
            provider.provider_id == PI.provider_placeholder
                && provider.provider_name == "Pi (Session)"
        }));
        Ok(())
    }

    #[test]
    fn zero_reported_cost_uses_fresh_input_pricing() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "pricing");
        let assistant = assistant_line("priced", "2023-11-14T22:13:21Z", 10);
        write_lines(
            &path,
            &[
                r#"{"type":"session","version":3,"id":"session-priced","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &assistant,
            ],
        );

        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO model_pricing (
                    model_id, display_name, input_cost_per_million,
                    output_cost_per_million, cache_read_cost_per_million,
                    cache_creation_cost_per_million
                 ) VALUES ('fixture-model', 'Fixture', '1', '2', '0.1', '0.5')",
                [],
            )?;
        }
        let result = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        // Pi input is already fresh; cache buckets are priced in addition to
        // all 10 input tokens rather than subtracted from them.
        assert_eq!(
            Decimal::from_str(&total).expect("calculated total"),
            Decimal::from_str("0.0000155").expect("expected total")
        );
        Ok(())
    }

    #[test]
    fn distinct_ids_keep_identical_usage_events_separate() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "same-payload");
        let first = assistant_line("first", "2023-11-14T22:13:21Z", 1);
        let second = assistant_line("second", "2023-11-14T22:13:21Z", 1);
        write_lines(
            &path,
            &[
                r#"{"type":"session","version":3,"id":"session-same","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &first,
                &second,
            ],
        );

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            2
        );
        Ok(())
    }

    #[test]
    fn bounds_untrusted_provider_and_model_labels_at_utf8_boundaries() {
        let mut entry: Value =
            serde_json::from_str(&assistant_line("bounded", "2023-11-14T22:13:21Z", 1))
                .expect("assistant entry");
        let oversized = "界".repeat(MAX_USAGE_LABEL_BYTES);
        let message = entry
            .get_mut("message")
            .and_then(Value::as_object_mut)
            .expect("message object");
        message.insert("provider".to_string(), Value::String(oversized.clone()));
        message.insert("model".to_string(), Value::String(oversized.clone()));
        message.insert("responseModel".to_string(), Value::String(oversized));

        let record = parse_usage_record(&PI, &entry, "session", None, 0).expect("usage record");
        for label in [record.provider_id, record.model, record.request_model] {
            assert!(label.len() <= MAX_USAGE_LABEL_BYTES);
            assert!(std::str::from_utf8(label.as_bytes()).is_ok());
        }
    }

    #[test]
    fn rejects_timestamps_outside_sqlite_date_range() {
        let mut entry: Value =
            serde_json::from_str(&assistant_line("time", "2023-11-14T22:13:21Z", 1))
                .expect("assistant entry");
        entry["timestamp"] = Value::from(i64::MIN);
        entry["message"]["timestamp"] = Value::from(i64::MAX);

        let record = parse_usage_record(&PI, &entry, "session", Some(1_700_000_000), 0)
            .expect("usage record");
        assert_eq!(record.created_at, 1_700_000_000);
    }

    #[test]
    fn estimates_assistant_latency_from_its_own_timestamps() {
        let mut entry: Value =
            serde_json::from_str(&assistant_line("timed", "2023-11-14T22:13:22.250Z", 1))
                .expect("assistant entry");
        for start in [
            Value::from(1_700_000_000_000i64),
            Value::from(1_700_000_000i64),
            Value::from("2023-11-14T22:13:20Z"),
        ] {
            entry["message"]["timestamp"] = start;
            for end in [
                Value::from("2023-11-14T22:13:22.250Z"),
                Value::from(1_700_000_002_250i64),
            ] {
                entry["timestamp"] = end;
                let record =
                    parse_usage_record(&PI, &entry, "session", None, 0).expect("usage record");
                assert_eq!(record.latency_ms, 2_250);
                assert_eq!(record.created_at, 1_700_000_002);
            }
        }
    }

    #[test]
    fn missing_or_implausible_timestamps_do_not_estimate_latency() {
        let mut entry: Value =
            serde_json::from_str(&assistant_line("untimed", "2023-11-14T22:13:22Z", 1))
                .expect("assistant entry");
        let start_ms = 1_700_000_000_000i64;
        for (start, end) in [
            (Value::Null, Value::from(start_ms + 2_000)),
            (Value::from(start_ms), Value::Null),
            (Value::from("invalid"), Value::from(start_ms + 2_000)),
            (Value::from(start_ms), Value::from("invalid")),
            (Value::from(start_ms), Value::from(start_ms)),
            (Value::from(start_ms), Value::from(start_ms - 1_000)),
            (Value::from(start_ms), Value::from(start_ms + 99)),
            (Value::from(start_ms), Value::from(start_ms + 3_600_001)),
            (Value::from(i64::MIN), Value::from(i64::MAX)),
        ] {
            entry["message"]["timestamp"] = start;
            entry["timestamp"] = end;
            // Header/mtime fallbacks are for created_at only, never latency.
            let record =
                parse_usage_record(&PI, &entry, "session", Some(1_700_000_000), 1_700_000_000)
                    .expect("usage record");
            assert_eq!(record.latency_ms, 0, "{entry}");
        }
    }

    #[test]
    fn non_assistant_usage_does_not_infer_request_timing() {
        let mut entry: Value =
            serde_json::from_str(&assistant_line("tool", "2023-11-14T22:13:22Z", 1))
                .expect("assistant entry");
        entry["message"]["role"] = Value::from("toolResult");
        let record = parse_usage_record(&PI, &entry, "session", None, 0).expect("tool usage");
        assert_eq!(record.latency_ms, 0);

        entry["usage"] = entry["message"]["usage"].clone();
        for kind in ["compaction", "branch_summary"] {
            entry["type"] = Value::from(kind);
            let record =
                parse_usage_record(&PI, &entry, "session", None, 0).expect("summary usage");
            assert_eq!(record.latency_ms, 0);
        }
    }

    #[test]
    fn imports_timing_on_initial_and_incremental_sync_for_speed_stats() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "timing");
        let first = assistant_line("first", "2023-11-14T22:13:30Z", 1)
            .replace(r#""output":2"#, r#""output":900"#);
        write_lines(
            &path,
            &[
                r#"{"type":"session","version":3,"id":"session-timing","timestamp":"2023-11-14T22:00:00Z","cwd":"/work"}"#,
                &first,
            ],
        );
        let db = Database::memory()?;
        let initial = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert!(initial.errors.is_empty());
        assert_eq!(initial.imported, 1);

        let second = assistant_line("second", "2023-11-14T22:14:20Z", 1)
            .replace("1700000000000", &1_700_000_040_000i64.to_string())
            .replace(r#""output":2"#, r#""output":600"#);
        {
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(file, "{second}").unwrap();
        }
        let appended = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert!(appended.errors.is_empty());
        assert_eq!((appended.imported, appended.skipped), (1, 0));
        {
            let conn = lock_conn!(db.conn);
            let mut statement = conn.prepare(
                "SELECT latency_ms, first_token_ms FROM proxy_request_logs ORDER BY created_at",
            )?;
            let timings = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(timings, vec![(10_000, None), (20_000, None)]);
        }
        let stats = db.get_provider_stats(None, None, Some(PI.app_type), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].est_speed_output_tokens, 1_500);
        assert_eq!(stats[0].est_speed_duration_ms, 30_000);
        assert_eq!(stats[0].speed_output_tokens, 0);
        assert_eq!(stats[0].speed_generation_ms, 0);
        Ok(())
    }

    #[test]
    fn compatible_cursors_stay_incremental_without_changing_history() -> Result<(), AppError> {
        for marker in [0b101u64, 0b110] {
            let temp = tempfile::tempdir().unwrap();
            let path = session_path(temp.path(), "compatible-cursor");
            let header = r#"{"type":"session","version":3,"id":"compatible-cursor"}"#;
            let first = assistant_line("untimed", "2023-11-14T22:13:30Z", 1);
            let second = assistant_line("timed", "2023-11-14T22:13:40Z", 2);
            write_lines(&path, &[header, &first, &second]);
            let db = Database::memory()?;
            assert_eq!(
                sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
                2
            );
            lock_conn!(db.conn).execute(
                "UPDATE proxy_request_logs SET latency_ms = CASE input_tokens WHEN 1 THEN 0 ELSE 1234 END,
                    total_cost_usd = '0.42'",
                [],
            )?;
            let cursors = crate::services::session_usage::load_sync_cursors(&db)?;
            let cursor = &cursors[path.to_string_lossy().as_ref()];
            let revision = (cursor.last_synced_at as u64 & ((1 << REVISION_MARKER_SHIFT) - 1))
                | (marker << REVISION_MARKER_SHIFT);
            lock_conn!(db.conn).execute(
                "UPDATE session_log_sync SET last_synced_at = ?1 WHERE file_path = ?2",
                rusqlite::params![revision as i64, path.to_string_lossy()],
            )?;

            let unchanged = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
            assert!(unchanged.errors.is_empty());
            assert_eq!((unchanged.imported, unchanged.skipped), (0, 0));

            let third = assistant_line("new", "2023-11-14T22:13:50Z", 3);
            {
                let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
                writeln!(file, "{third}").unwrap();
            }
            let appended = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
            assert!(appended.errors.is_empty());
            assert_eq!((appended.imported, appended.skipped), (1, 0));
            let cursors = crate::services::session_usage::load_sync_cursors(&db)?;
            let cursor = &cursors[path.to_string_lossy().as_ref()];
            assert_eq!(cursor.last_line_offset, 4);
            assert_eq!(cursor.last_synced_at as u64 >> REVISION_MARKER_SHIFT, 0b101);

            // A normal rewrite may revisit old requests, but deduplication must
            // leave their missing or previously recorded timing untouched.
            write_lines(&path, &[&format!("{header} "), &first, &second, &third]);
            let rewritten = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
            assert!(rewritten.errors.is_empty());
            assert_eq!((rewritten.imported, rewritten.skipped), (0, 3));
            let conn = lock_conn!(db.conn);
            let mut stmt = conn.prepare(
                "SELECT latency_ms, total_cost_usd FROM proxy_request_logs ORDER BY input_tokens",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(
                rows,
                vec![
                    (0, "0.42".into()),
                    (1234, "0.42".into()),
                    (30_000, "0".into())
                ]
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_entry_id_migration_preserves_existing_timing() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "legacy-timing");
        let header = r#"{"type":"session","version":1,"id":"session-legacy","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let migrated = assistant_line("generated", "2023-11-14T22:13:30Z", 1);
        let legacy = migrated.replace(r#""id":"generated","parentId":null,"#, "");
        write_lines(&path, &[header, &legacy]);
        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );
        lock_conn!(db.conn).execute("UPDATE proxy_request_logs SET latency_ms = 0", [])?;

        write_lines(
            &path,
            &[
                &header.replace(r#""version":1"#, r#""version":3"#),
                &migrated,
            ],
        );
        let migrated = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert!(migrated.errors.is_empty());
        assert_eq!((migrated.imported, migrated.skipped), (0, 1));
        let totals: (i64, i64) = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*), SUM(latency_ms) FROM proxy_request_logs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(totals, (1, 0));
        Ok(())
    }

    #[test]
    fn forked_history_is_not_imported_twice() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let parent = session_path(temp.path(), "parent");
        let fork = session_path(temp.path(), "fork");
        let first = assistant_line("first", "2023-11-14T22:13:21Z", 1);
        let second = assistant_line("second", "2023-11-14T22:13:23Z", 3);
        write_lines(
            &parent,
            &[
                r#"{"type":"session","version":3,"id":"session-parent","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &first,
            ],
        );
        write_lines(
            &fork,
            &[
                r#"{"type":"session","version":3,"id":"session-fork","timestamp":"2023-11-14T22:13:22Z","cwd":"/work","parentSession":"parent.jsonl"}"#,
                &first,
                &second,
            ],
        );

        let db = Database::memory()?;
        let result = sync_pi_files(&db, &PI, &[parent.clone(), fork.clone()]);
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 1);

        // A fork is also a complete recovery source when its parent file is
        // absent from a fresh database.
        let orphan_db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&orphan_db, &PI, std::slice::from_ref(&fork)).imported,
            2
        );

        let second_pass = sync_pi_files(&db, &PI, &[parent, fork]);
        assert_eq!(second_pass.imported, 0);
        let count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn rollup_then_legacy_migration_and_fork_do_not_reimport_history() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let parent = session_path(temp.path(), "legacy-parent");
        let fork = session_path(temp.path(), "migrated-fork");
        let migrated = assistant_line("generated", "2023-11-14T22:13:21Z", 1);
        let legacy = migrated.replace("\"id\":\"generated\",\"parentId\":null,", "");
        write_lines(
            &parent,
            &[
                r#"{"type":"session","version":1,"id":"session-legacy","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &legacy,
            ],
        );

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&parent)).imported,
            1
        );
        assert_eq!(db.rollup_and_prune(30)?, 1);

        // Pi migrates v1 in place by adding generated entry IDs while keeping
        // the same logical rows. The line cursor prevents those rows from
        // being treated as new after the rewrite.
        write_lines(
            &parent,
            &[
                r#"{"type":"session","version":3,"id":"session-legacy","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &migrated,
            ],
        );
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&parent)).imported,
            0
        );

        let new_entry = assistant_line("new-entry", "2023-11-14T22:13:23Z", 2);
        write_lines(
            &fork,
            &[
                r#"{"type":"session","version":3,"id":"session-fork","timestamp":"2023-11-14T22:13:22Z","cwd":"/work","parentSession":"legacy-parent.jsonl"}"#,
                &migrated,
                &new_entry,
            ],
        );
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&fork)).imported,
            1
        );

        let conn = lock_conn!(db.conn);
        let rolled_up: i64 = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0) FROM usage_daily_rollups
             WHERE app_type = 'pi'",
            [],
            |row| row.get(0),
        )?;
        let details: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((rolled_up, details), (1, 1));
        drop(conn);

        // Once the new fork request is rolled up too, a shorter repair forces
        // a full rescan. The durable ledger still rejects both old identities.
        assert_eq!(db.rollup_and_prune(30)?, 1);
        write_lines(
            &fork,
            &[
                r#"{"type":"session","version":3,"id":"session-fork","timestamp":"2023-11-14T22:13:22Z","cwd":"/work","parentSession":"legacy-parent.jsonl"}"#,
                &migrated,
            ],
        );
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&fork)).imported,
            0
        );
        let conn = lock_conn!(db.conn);
        let totals: (i64, i64) = conn.query_row(
            "SELECT
                (SELECT COALESCE(SUM(request_count), 0) FROM usage_daily_rollups
                 WHERE app_type = 'pi'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(totals, (2, 0));
        Ok(())
    }

    #[test]
    fn assistant_failures_keep_status_and_error_message() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "failures");
        let failed = assistant_line("failed", "2023-11-14T22:13:21Z", 1).replace(
            r#""stopReason":"stop""#,
            r#""stopReason":"error","errorMessage":"provider failed""#,
        );
        let aborted = assistant_line("aborted", "2023-11-14T22:13:22Z", 1)
            .replace(r#""stopReason":"stop""#, r#""stopReason":"aborted""#);
        write_lines(
            &path,
            &[
                r#"{"type":"session","version":3,"id":"session-failures","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#,
                &failed,
                &aborted,
            ],
        );

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            2
        );
        let conn = lock_conn!(db.conn);
        let statuses: Vec<(i64, String)> = conn
            .prepare(
                "SELECT status_code, error_message FROM proxy_request_logs
                 WHERE data_source = 'pi_session' ORDER BY status_code",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        assert_eq!(
            statuses,
            vec![
                (499, "Pi request aborted".to_string()),
                (500, "provider failed".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn parser_stops_at_the_captured_file_size() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "snapshot");
        let header = r#"{"type":"session","version":3,"id":"session-snapshot","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let first = assistant_line("first", "2023-11-14T22:13:21Z", 1);
        let second = assistant_line("second", "2023-11-14T22:13:22Z", 2);
        write_lines(&path, &[header, &first]);
        let metadata = fs::metadata(&path).expect("snapshot metadata");
        let snapshot_size = metadata.len();
        let modified = metadata_modified_nanos(&metadata);
        {
            let mut file = File::options()
                .append(true)
                .open(&path)
                .expect("append after snapshot");
            writeln!(file, "{second}").expect("append second record");
        }

        let parsed = parse_pi_file(
            &PI,
            &path,
            0,
            None,
            snapshot_size,
            modified,
            &ChildScope::default(),
        )?;
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.last_complete_line, 2);
        Ok(())
    }

    #[test]
    fn valid_unterminated_final_record_is_imported() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "unterminated");
        let header = r#"{"type":"session","version":3,"id":"session-unterminated","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let assistant = assistant_line("final", "2023-11-14T22:13:21Z", 1);
        let mut file = File::create(&path).expect("create session");
        writeln!(file, "{header}").expect("write header");
        file.write_all(assistant.as_bytes())
            .expect("write unterminated record");
        drop(file);

        let db = Database::memory()?;
        let first = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!((first.imported, first.deferred_files), (1, 0));
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            0
        );
        Ok(())
    }

    #[test]
    fn failed_append_prefix_proof_rescans_a_longer_rewrite() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "longer-rewrite");
        let header = r#"{"type":"session","version":3,"id":"session-rewrite","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let first = assistant_line("first", "2023-11-14T22:13:22Z", 1);
        write_lines(&path, &[header, &first]);

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );

        let before = assistant_line("before", "2023-11-14T22:13:21Z", 2);
        let after = assistant_line("after", "2023-11-14T22:13:23Z", 3);
        write_lines(&path, &[header, &before, &first, &after]);
        let rewritten = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!((rewritten.imported, rewritten.skipped), (2, 1));
        Ok(())
    }

    #[test]
    fn same_size_middle_rewrite_is_detected_by_mtime() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "middle-rewrite");
        let header = r#"{"type":"session","version":3,"id":"session-middle","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let zero = assistant_line("middle", "2023-11-14T22:13:21Z", 0)
            .replace(r#""output":2"#, r#""output":0"#)
            .replace(r#""cacheRead":5"#, r#""cacheRead":0"#)
            .replace(r#""cacheWrite":2"#, r#""cacheWrite":0"#);
        let one = zero.replacen(r#""input":0,"output":0"#, r#""input":1,"output":0"#, 1);
        let filler = serde_json::json!({
            "type": "message",
            "id": "filler",
            "timestamp": "2023-11-14T22:13:22Z",
            "message": {"role": "user", "content": "x".repeat(5000)}
        })
        .to_string();
        write_lines(&path, &[header, &zero, &filler]);
        let initial_size = fs::metadata(&path).expect("initial metadata").len();

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            0
        );
        let previous_mtime = fs::metadata(&path)
            .expect("previous metadata")
            .modified()
            .expect("previous mtime");

        write_lines(&path, &[header, &one, &filler]);
        assert_eq!(
            fs::metadata(&path).expect("rewrite metadata").len(),
            initial_size
        );
        set_modified(&path, previous_mtime + std::time::Duration::from_secs(2));
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );
        Ok(())
    }

    #[test]
    fn stable_entry_id_and_canonical_json_prevent_rewrite_duplicates() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let current = session_path(temp.path(), "current");
        let legacy = session_path(temp.path(), "legacy");
        let header = r#"{"type":"session","version":3,"id":"session-stable","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let ten = assistant_line("stable", "2023-11-14T22:13:21Z", 10);
        let eleven = assistant_line("stable", "2023-11-14T22:13:21Z", 11);
        write_lines(&current, &[header, &ten]);

        let db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&current)).imported,
            1
        );
        write_lines(&current, &[header, &eleven]);
        let correction = sync_pi_files(&db, &PI, std::slice::from_ref(&current));
        assert_eq!((correction.imported, correction.skipped), (0, 1));
        let current_count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(current_count, 1);

        let without_id = ten.replace(r#""id":"stable","parentId":null,"#, "");
        let reordered = without_id.replace(
            r#"{"type":"text","text":"ok"}"#,
            r#"{"text":"ok","type":"text"}"#,
        );
        write_lines(&legacy, &[header, &without_id]);
        let legacy_db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&legacy_db, &PI, std::slice::from_ref(&legacy)).imported,
            1
        );
        write_lines(&legacy, &[header, &reordered]);
        let reordered_result = sync_pi_files(&legacy_db, &PI, std::slice::from_ref(&legacy));
        assert_eq!(
            (reordered_result.imported, reordered_result.skipped),
            (0, 1)
        );

        let legacy_count: i64 = lock_conn!(legacy_db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(legacy_count, 1);
        Ok(())
    }

    #[test]
    fn incomplete_tail_and_truncated_rewrite_are_recovered() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "active");
        let header = r#"{"type":"session","version":3,"id":"session-active","timestamp":"2023-11-14T22:13:20Z","cwd":"/work"}"#;
        let first = assistant_line("first", "2023-11-14T22:13:21Z", 1);
        let first_bytes = first.as_bytes();
        let split = first_bytes.len() / 2;
        {
            let mut file = File::create(&path).expect("create partial session");
            writeln!(file, "{header}").expect("header");
            file.write_all(&first_bytes[..split]).expect("partial row");
        }

        let db = Database::memory()?;
        let deferred = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(deferred.imported, 0);
        assert_eq!(deferred.deferred_files, 1);
        let deferred_state = decode_pi_sync_state(
            crate::services::session_usage::load_sync_cursors(&db)
                .unwrap()
                .get(path.to_string_lossy().as_ref()),
        )
        .expect("deferred sync state");
        assert!(!deferred_state.revision.complete);
        let unchanged = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!((unchanged.imported, unchanged.deferred_files), (0, 0));

        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("append session");
            file.write_all(&first_bytes[split..]).expect("finish row");
            writeln!(file).expect("finish newline");
        }
        let completed = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(completed.imported, 1);
        let preserved_mtime = fs::metadata(&path)
            .expect("session metadata")
            .modified()
            .expect("session mtime");

        let second = assistant_line("second", "2023-11-14T22:13:22Z", 2);
        let third = assistant_line("third", "2023-11-14T22:13:23Z", 3);
        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("append complete row");
            writeln!(file, "{second}").expect("second row");
        }
        set_modified(&path, preserved_mtime);
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );

        // Rewrite to fewer lines than the saved cursor. Already billed rows
        // remain, while the new record is found even with a preserved mtime.
        write_lines(&path, &[header, &third]);
        set_modified(&path, preserved_mtime);
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );

        // A same-size, same-mtime repair is also detected by the bounded tail
        // fingerprint and rescanned from the header.
        let fifth = assistant_line("fifth", "2023-11-14T22:13:24Z", 4);
        let repaired_size = fs::metadata(&path).expect("repair metadata").len();
        write_lines(&path, &[header, &fifth]);
        assert_eq!(
            fs::metadata(&path).expect("replacement metadata").len(),
            repaired_size
        );
        set_modified(&path, preserved_mtime);
        assert_eq!(
            sync_pi_files(&db, &PI, std::slice::from_ref(&path)).imported,
            1
        );
        let count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'pi_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 4);
        Ok(())
    }

    #[test]
    fn oversized_file_is_reported_instead_of_silently_skipped() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_path(temp.path(), "oversized");
        File::create(&path)
            .expect("create sparse session")
            .set_len(crate::session_manager::providers::pi::MAX_SESSION_BYTES + 1)
            .expect("size sparse session");

        let db = Database::memory()?;
        let result = sync_pi_files(&db, &PI, std::slice::from_ref(&path));
        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("安全上限"));
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn subagent_usage_is_counted_once_from_the_child_sessions() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let agent_dir = temp.path().join("agent");
        let _agent = crate::pi_config::test_support::TestAgentDir::at(&agent_dir);
        let project = agent_dir.join("sessions").join("project");
        let parent = session_path(&project, "parent");
        let native_fork = session_path(&project, "native-fork");
        let stem = parent.with_extension("");
        let child_a = run_session(&stem, "run-a");
        let grandchild = run_session(&child_a.with_extension(""), "run-g");
        let child_b = run_session(&stem, "run-b");
        let child_c = run_session(&stem, "run-c");
        let fork = stem.join("forks").join("fork.jsonl");
        let parent_call = assistant_line("p1", "2023-11-14T22:13:21Z", 100);
        let projection_a = projection_line(
            "t-a",
            "2023-11-14T22:13:40Z",
            "subagent",
            json!({"results": [{"sessionFile": child_a.to_string_lossy()}]}),
            10,
        );

        write_nested_lines(
            &parent,
            &[
                &session_header("session-parent", None),
                &parent_call,
                &projection_a,
                &projection_line(
                    "t-b",
                    "2023-11-14T22:13:41Z",
                    "bg_wait",
                    json!({"completions": [{"results": [{"sessionFile": child_b.to_string_lossy()}]}]}),
                    20,
                ),
                r#"{"type":"custom_message","id":"notice","parentId":null,"timestamp":"2023-11-14T22:13:50Z","customType":"subagent-notify","content":"run-c finished","display":true}"#,
                &projection_line(
                    "t-f",
                    "2023-11-14T22:13:55Z",
                    "subagent",
                    json!({"results": [{"sessionFile": fork.to_string_lossy()}]}),
                    80,
                ),
            ],
        );
        write_nested_lines(
            &child_a,
            &[
                &session_header("session-a", Some(&parent)),
                &assistant_line("a1", "2023-11-14T22:13:25Z", 10),
                &projection_line(
                    "t-g",
                    "2023-11-14T22:13:27Z",
                    "subagent",
                    json!({"results": [{"sessionFile": grandchild.to_string_lossy()}]}),
                    7,
                ),
            ],
        );
        write_nested_lines(
            &grandchild,
            &[
                &session_header("session-g", Some(&child_a)),
                &assistant_line("g1", "2023-11-14T22:13:26Z", 7),
            ],
        );
        write_nested_lines(
            &child_b,
            &[
                &session_header("session-b", Some(&parent)),
                &assistant_line("b1", "2023-11-14T22:13:32Z", 20),
            ],
        );
        write_nested_lines(
            &child_c,
            &[
                &session_header("session-c", Some(&parent)),
                &assistant_line("c1", "2023-11-14T22:13:37Z", 40),
            ],
        );
        write_nested_lines(
            &fork,
            &[
                &session_header("session-fork", Some(&parent)),
                &parent_call,
                &projection_a,
                &assistant_line("f1", "2023-11-14T22:13:47Z", 80),
            ],
        );
        write_nested_lines(
            &native_fork,
            &[
                &session_header("session-native-fork", Some(&parent)),
                &parent_call,
                &projection_a,
            ],
        );

        let db = Database::memory()?;
        set_child_cutoff(&db, 1_700_000_000)?;
        let result = sync_pi_usage(&db)?;
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(pi_rows_input_and_unknown(&db)?, (6, 257, 0));

        let again = sync_pi_usage(&db)?;
        assert!(again.errors.is_empty(), "{:?}", again.errors);
        assert_eq!(again.imported, 0);
        Ok(())
    }

    #[test]
    fn usage_before_the_cutoff_stays_as_projections_and_later_usage_comes_from_the_children(
    ) -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let parent = session_path(temp.path(), "parent");
        let stem = parent.with_extension("");
        let child_a = run_session(&stem, "run-a");
        let child_c = run_session(&stem, "run-c");
        let parent_call = assistant_line("p1", "2023-11-14T22:13:21Z", 100);
        let projection_a = projection_line(
            "t-a",
            "2023-11-14T22:13:30Z",
            "subagent",
            json!({"results": [{"sessionFile": child_a.to_string_lossy()}]}),
            10,
        );
        write_lines(
            &parent,
            &[
                &session_header("session-parent", None),
                &parent_call,
                &projection_a,
            ],
        );
        write_nested_lines(
            &child_a,
            &[
                &session_header("session-a", Some(&parent)),
                &assistant_line("a1", "2023-11-14T22:13:25Z", 10),
                &assistant_line("a2", "2023-11-14T22:14:00Z", 5),
            ],
        );
        write_nested_lines(
            &child_c,
            &[
                &session_header("session-c", Some(&parent)),
                &assistant_line("c1", "2023-11-14T22:13:35Z", 40),
            ],
        );

        let db = Database::memory()?;
        set_child_cutoff(&db, 1_700_000_020)?;
        let files = [parent.clone(), child_a.clone(), child_c.clone()];
        let first = sync_pi_files(&db, &PI, &files);
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert_eq!(first.imported, 3);
        assert_eq!(pi_rows_input_and_unknown(&db)?, (3, 115, 1));

        write_lines(
            &parent,
            &[
                &session_header("session-parent", None),
                &parent_call,
                &projection_a,
                &projection_line(
                    "t-a2",
                    "2023-11-14T22:14:05Z",
                    "subagent",
                    json!({"results": [{"sessionFile": child_a.to_string_lossy()}]}),
                    15,
                ),
            ],
        );
        let second = sync_pi_files(&db, &PI, &files);
        assert!(second.errors.is_empty(), "{:?}", second.errors);
        assert_eq!((second.imported, second.skipped), (0, 0));
        assert_eq!(pi_rows_input_and_unknown(&db)?, (3, 115, 1));

        lock_conn!(db.conn).execute(
            "DELETE FROM session_log_sync WHERE file_path = ?1",
            [parent.to_string_lossy().to_string()],
        )?;
        let rescan = sync_pi_files(&db, &PI, &files);
        assert!(rescan.errors.is_empty(), "{:?}", rescan.errors);
        assert_eq!((rescan.imported, rescan.skipped), (0, 2));
        assert_eq!(pi_rows_input_and_unknown(&db)?, (3, 115, 1));
        Ok(())
    }

    #[test]
    fn projections_of_children_outside_the_imported_files_are_still_counted() -> Result<(), AppError>
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let parent = session_path(temp.path(), "parent");
        let inside = run_session(&parent.with_extension(""), "run-a");
        let relocated = session_path(&temp.path().join("moved"), "session");
        write_nested_lines(&inside, &[&session_header("session-a", Some(&parent))]);
        write_nested_lines(&relocated, &[&session_header("session-m", Some(&parent))]);
        write_lines(
            &parent,
            &[
                &session_header("session-parent", None),
                &projection_line(
                    "t-in",
                    "2023-11-14T22:13:30Z",
                    "subagent",
                    json!({"results": [{"sessionFile": inside.to_string_lossy()}]}),
                    10,
                ),
                &projection_line(
                    "t-out",
                    "2023-11-14T22:13:31Z",
                    "subagent",
                    json!({"results": [{"sessionFile": relocated.to_string_lossy()}]}),
                    20,
                ),
                &projection_line(
                    "t-mixed",
                    "2023-11-14T22:13:32Z",
                    "subagent",
                    json!({"results": [
                        {"sessionFile": inside.to_string_lossy()},
                        {"sessionFile": relocated.to_string_lossy()}
                    ]}),
                    30,
                ),
            ],
        );

        let db = Database::memory()?;
        set_child_cutoff(&db, 1_700_000_000)?;
        let result = sync_pi_files(&db, &PI, &[parent.clone(), inside.clone()]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(pi_rows_input_and_unknown(&db)?, (2, 50, 2));
        Ok(())
    }

    #[test]
    fn the_first_import_records_the_cutoff_once() -> Result<(), AppError> {
        let recorded = |db: &Database| -> Result<i64, AppError> {
            let conn = lock_conn!(db.conn);
            Ok(conn.query_row(
                "SELECT last_synced_at FROM session_log_sync WHERE file_path = ?1",
                [PI.cutoff_key.expect("Pi cutoff key")],
                |row| row.get(0),
            )?)
        };
        let db = Database::memory()?;
        let before = chrono::Utc::now().timestamp();
        let first = sync_pi_files(&db, &PI, &[]);
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert!(recorded(&db)? >= before);

        set_child_cutoff(&db, 42)?;
        let second = sync_pi_files(&db, &PI, &[]);
        assert!(second.errors.is_empty(), "{:?}", second.errors);
        assert_eq!(recorded(&db)?, 42);
        Ok(())
    }
}
