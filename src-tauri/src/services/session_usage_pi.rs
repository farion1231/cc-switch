//! Pi coding-agent session usage importer.
//!
//! Pi records normalized token and cost data in its session JSONL files. This
//! importer keeps direct (non-proxy) Pi usage visible in the shared dashboard.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const APP_TYPE: &str = "pi";
const DATA_SOURCE: &str = "pi_session";
const PROVIDER_PLACEHOLDER: &str = "_pi_session";
const UNKNOWN_MODEL: &str = "unknown";
const MAX_USAGE_LABEL_BYTES: usize = 512;
const MIN_SQLITE_UNIX_MILLIS: i64 = -62_167_219_200_000;
const MAX_SQLITE_UNIX_MILLIS: i64 = 253_402_300_799_999;
const REVISION_TAIL_BYTES: u64 = 4096;
const REVISION_MARKER_SHIFT: u32 = 61;
const REVISION_COMPLETE_SHIFT: u32 = 60;
const REVISION_SIZE_SHIFT: u32 = 32;
const REVISION_MARKER: u64 = 0b101;
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

/// Import usage from every Pi session file discoverable by the session
/// browser's current root and layout rules.
pub fn sync_pi_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = crate::session_manager::providers::pi::session_files()
        .map_err(|error| AppError::Config(format!("无法发现 Pi 会话: {error}")))?;
    Ok(sync_pi_files(db, &files))
}

fn sync_pi_files(db: &Database, files: &[PathBuf]) -> SessionSyncResult {
    let mut result = SessionSyncResult {
        files_scanned: files.len().min(u32::MAX as usize) as u32,
        ..Default::default()
    };

    for file_path in files {
        match sync_single_pi_file(db, file_path) {
            Ok(file_result) => result.merge(file_result),
            Err(error) => {
                let message = format!("{}: {error}", file_path.display());
                log::warn!("[PI-SYNC] 会话文件解析失败: {message}");
                result.errors.push(message);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[PI-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }
    result
}

fn sync_single_pi_file(db: &Database, file_path: &Path) -> Result<SessionSyncResult, AppError> {
    let metadata = fs::symlink_metadata(file_path)
        .map_err(|error| AppError::Config(format!("无法读取 Pi 会话文件元数据: {error}")))?;
    if !metadata.file_type().is_file()
        || file_path.extension().and_then(|value| value.to_str()) != Some("jsonl")
    {
        return Err(AppError::Config(
            "Pi 会话路径不是普通 JSONL 文件".to_string(),
        ));
    }
    if metadata.len() > crate::session_manager::providers::pi::MAX_SESSION_BYTES {
        return Err(AppError::Config(format!(
            "Pi 会话文件超过 {} 字节安全上限",
            crate::session_manager::providers::pi::MAX_SESSION_BYTES
        )));
    }

    let file_path_string = file_path.to_string_lossy().to_string();
    let modified = metadata_modified_nanos(&metadata);
    let revision = pi_file_revision(file_path, &metadata, modified)?;
    let previous = get_pi_sync_state(db, &file_path_string)?;
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
                && pi_prefix_tail_matches(file_path, state.revision)? =>
        {
            (state.last_line_offset, Some(state.revision.file_size))
        }
        Some(_) | None => (0, None),
    };
    let parsed = parse_pi_file(
        file_path,
        start_after_line,
        start_at_byte,
        revision.file_size,
        modified,
    )?;
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 Pi 用量导入事务失败: {error}")))?;
    let mut result = SessionSyncResult::default();
    for record in &parsed.records {
        if insert_pi_record(&tx, record)? {
            result.imported = result.imported.saturating_add(1);
        } else {
            result.skipped = result.skipped.saturating_add(1);
        }
    }

    update_pi_sync_state_on_conn(&tx, &file_path_string, revision, parsed.last_complete_line)?;
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Pi 用量导入事务失败: {error}")))?;
    if parsed.incomplete_tail {
        result.deferred_files = 1;
    }
    Ok(result)
}

fn get_pi_sync_state(db: &Database, file_path: &str) -> Result<Option<PiSyncState>, AppError> {
    let conn = lock_conn!(db.conn);
    let row = conn
        .query_row(
            "SELECT last_modified, last_line_offset, last_synced_at
             FROM session_log_sync WHERE file_path = ?1",
            rusqlite::params![file_path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| AppError::Database(format!("读取 Pi 会话同步状态失败: {error}")))?;
    let Some((modified_nanos, last_line_offset, encoded_revision)) = row else {
        return Ok(None);
    };
    let encoded_revision = encoded_revision as u64;
    if encoded_revision >> REVISION_MARKER_SHIFT != REVISION_MARKER {
        return Ok(None);
    }
    let file_size = (encoded_revision >> REVISION_SIZE_SHIFT) & REVISION_SIZE_MASK;
    if file_size > crate::session_manager::providers::pi::MAX_SESSION_BYTES
        || last_line_offset < 0
        || last_line_offset > crate::session_manager::providers::pi::MAX_TREE_ENTRIES as i64 + 1
    {
        return Ok(None);
    }
    Ok(Some(PiSyncState {
        revision: PiFileRevision {
            modified_nanos,
            file_size,
            tail_fingerprint: encoded_revision as u32,
            complete: ((encoded_revision >> REVISION_COMPLETE_SHIFT) & 1) == 1,
        },
        last_line_offset,
    }))
}

fn update_pi_sync_state_on_conn(
    conn: &rusqlite::Connection,
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
    .map_err(|error| AppError::Database(format!("更新 Pi 会话同步状态失败: {error}")))?;
    Ok(())
}

fn pi_file_revision(
    file_path: &Path,
    metadata: &fs::Metadata,
    modified_nanos: i64,
) -> Result<PiFileRevision, AppError> {
    let tail_len = metadata.len().min(REVISION_TAIL_BYTES);
    let mut tail = vec![0; tail_len as usize];
    if tail_len > 0 {
        let mut file = File::open(file_path)
            .map_err(|error| AppError::Config(format!("无法打开 Pi 会话文件: {error}")))?;
        file.seek(SeekFrom::Start(metadata.len() - tail_len))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| AppError::Config(format!("无法读取 Pi 会话文件尾部: {error}")))?;
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

fn pi_prefix_tail_matches(file_path: &Path, previous: PiFileRevision) -> Result<bool, AppError> {
    let tail_len = previous.file_size.min(REVISION_TAIL_BYTES);
    let mut tail = vec![0; tail_len as usize];
    if tail_len > 0 {
        let mut file = File::open(file_path)
            .map_err(|error| AppError::Config(format!("无法打开 Pi 会话文件: {error}")))?;
        file.seek(SeekFrom::Start(previous.file_size - tail_len))
            .and_then(|_| file.read_exact(&mut tail))
            .map_err(|error| AppError::Config(format!("无法校验 Pi 会话追加边界: {error}")))?;
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
    file_path: &Path,
    start_after_line: i64,
    start_at_byte: Option<u64>,
    snapshot_size: u64,
    file_modified_nanos: i64,
) -> Result<ParsedPiFile, AppError> {
    let file = File::open(file_path)
        .map_err(|error| AppError::Config(format!("无法打开 Pi 会话文件: {error}")))?;
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    let mut line_number = 0i64;
    let mut bytes_read = 0u64;
    let mut session_id = None;
    let mut session_timestamp = None;
    let mut records = Vec::new();
    let mut incomplete_tail = false;

    loop {
        buffer.clear();
        let remaining = snapshot_size.saturating_sub(bytes_read);
        if remaining == 0 {
            break;
        }
        let read = Read::by_ref(&mut reader)
            .take(remaining)
            .read_line(&mut buffer)
            .map_err(|error| AppError::Config(format!("无法读取 Pi 会话文件: {error}")))?;
        if read == 0 {
            return Err(AppError::Config("Pi 会话文件在读取期间被截断".to_string()));
        }
        bytes_read = bytes_read.saturating_add(read as u64);
        if bytes_read > crate::session_manager::providers::pi::MAX_SESSION_BYTES {
            return Err(AppError::Config(
                "Pi 会话文件读取时超过安全上限".to_string(),
            ));
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
                "Pi 会话超过 {} 条 entry 安全上限",
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
            if value.get("type").and_then(Value::as_str) != Some("session") {
                return Err(AppError::Config(
                    "Pi 会话的首条有效 JSON 不是 session header".to_string(),
                ));
            }
            session_id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| crate::session_manager::providers::pi::is_valid_tree_id(id))
                .map(str::to_string);
            if session_id.is_none() {
                return Err(AppError::Config("Pi 会话 header 缺少 id".to_string()));
            }
            let header_timestamp_millis = value.get("timestamp").and_then(parse_timestamp_millis);
            session_timestamp = header_timestamp_millis.map(|timestamp| timestamp / 1000);
            if let Some(byte_offset) = start_at_byte.filter(|offset| *offset >= bytes_read) {
                reader.seek(SeekFrom::Start(byte_offset)).map_err(|error| {
                    AppError::Config(format!("无法定位 Pi 会话增量边界: {error}"))
                })?;
                bytes_read = byte_offset;
                line_number = start_after_line;
            }
            continue;
        }
        if let Some(record) = parse_usage_record(
            &value,
            session_id.as_deref().unwrap_or_default(),
            session_timestamp,
            file_modified_nanos / 1_000_000_000,
        ) {
            records.push(record);
        }
    }

    if session_id.is_none() && !incomplete_tail {
        return Err(AppError::Config("Pi 会话没有有效 header".to_string()));
    }
    Ok(ParsedPiFile {
        records,
        last_complete_line: line_number,
        incomplete_tail,
    })
}

fn parse_usage_record(
    entry: &Value,
    session_id: &str,
    session_timestamp: Option<i64>,
    file_timestamp: i64,
) -> Option<PiUsageRecord> {
    let entry_type = entry.get("type").and_then(Value::as_str)?;
    let (kind, usage_value, message) = match entry_type {
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
        _ => return None,
    };

    let event_timestamp_millis = entry
        .get("timestamp")
        .and_then(parse_timestamp_millis)
        .or_else(|| {
            message
                .and_then(|value| value.get("timestamp"))
                .and_then(parse_timestamp_millis)
        });
    let input_tokens = token_count(usage_value, "input");
    let output_tokens = token_count(usage_value, "output");
    let cache_read_tokens = token_count(usage_value, "cacheRead");
    let cache_write_tokens = token_count(usage_value, "cacheWrite");
    let costs = parse_costs(usage_value.get("cost"));
    let stop_reason = (kind == "assistant")
        .then(|| message.and_then(|value| nonempty_string(value.get("stopReason"))))
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

    let (provider_id, model, request_model) = if kind == "assistant" {
        let message = message?;
        let provider = bounded_label(message.get("provider"), PROVIDER_PLACEHOLDER);
        let requested = bounded_label(message.get("model"), UNKNOWN_MODEL);
        let actual = nonempty_string(message.get("responseModel"))
            .map(truncate_usage_label)
            .unwrap_or(&requested)
            .to_string();
        (provider, actual, requested)
    } else {
        (
            PROVIDER_PLACEHOLDER.to_string(),
            UNKNOWN_MODEL.to_string(),
            UNKNOWN_MODEL.to_string(),
        )
    };

    let created_at = event_timestamp_millis
        .map(|timestamp| timestamp / 1000)
        .or(session_timestamp)
        .unwrap_or(file_timestamp)
        .clamp(MIN_SQLITE_UNIX_MILLIS / 1000, MAX_SQLITE_UNIX_MILLIS / 1000);

    let (status_code, error_message) = if kind == "assistant" {
        match stop_reason {
            Some("error") | Some("aborted") => {
                let fallback = if stop_reason == Some("aborted") {
                    "Pi request aborted"
                } else {
                    "Pi request failed"
                };
                let error = message
                    .and_then(|value| nonempty_string(value.get("errorMessage")))
                    .unwrap_or(fallback)
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

    let identity = pi_request_identity(entry, kind, usage_value, message);

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
    entry: &Value,
    kind: &str,
    usage: &Value,
    message: Option<&Value>,
) -> PiRequestIdentity {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"pi-session-semantic-v1");
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
    let semantic_id = format!("pi_session_semantic:{:x}", hasher.finalize());
    let entry_id = nonempty_string(entry.get("id"));
    let request_id = if let Some(entry_id) = entry_id {
        let mut request_hasher = Sha256::new();
        hash_field(&mut request_hasher, b"pi-session-request-v3");
        hash_field(&mut request_hasher, kind.as_bytes());
        hash_field(&mut request_hasher, entry_id.as_bytes());
        if let Some(timestamp) = entry.get("timestamp") {
            hash_json(&mut request_hasher, timestamp);
        }
        format!("pi_session:{:x}", request_hasher.finalize())
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

fn insert_pi_record(conn: &rusqlite::Connection, record: &PiUsageRecord) -> Result<bool, AppError> {
    let request_seen: bool = conn
        .query_row(
            PI_REQUEST_DEDUP_SQL,
            rusqlite::params![DATA_SOURCE, record.request_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(format!("查询 Pi 用量去重账本失败: {error}")))?;
    let already_seen = request_seen
        || conn
            .query_row(
                if record.has_entry_id {
                    PI_LEGACY_SEMANTIC_DEDUP_SQL
                } else {
                    PI_SEMANTIC_DEDUP_SQL
                },
                rusqlite::params![DATA_SOURCE, record.semantic_id],
                |row| row.get(0),
            )
            .map_err(|error| AppError::Database(format!("查询 Pi 用量去重账本失败: {error}")))?;
    if already_seen {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            DATA_SOURCE,
            record.request_id,
            record.semantic_id,
            i64::from(record.has_entry_id),
        ],
    )
    .map_err(|error| AppError::Database(format!("写入 Pi 用量去重账本失败: {error}")))?;

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_write_tokens,
        model: Some(record.model.clone()),
        message_id: None,
    };
    let costs = record.costs.reported().or_else(|| {
        find_model_pricing(conn, &record.model).map(|pricing| {
            let calculated =
                CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::ONE);
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
            APP_TYPE,
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
            0i64,
            Option::<i64>::None,
            record.status_code,
            record.error_message,
            record.session_id,
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            record.created_at,
            DATA_SOURCE,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|error| AppError::Database(format!("插入 Pi 会话用量失败: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
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
                .query_map(rusqlite::params![DATA_SOURCE, "identity"], |row| {
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
        let result = sync_pi_files(&db, std::slice::from_ref(&path));
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
            provider.provider_id == PROVIDER_PLACEHOLDER && provider.provider_name == "Pi (Session)"
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
        let result = sync_pi_files(&db, std::slice::from_ref(&path));
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 2);
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

        let record = parse_usage_record(&entry, "session", None, 0).expect("usage record");
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

        let record =
            parse_usage_record(&entry, "session", Some(1_700_000_000), 0).expect("usage record");
        assert_eq!(record.created_at, 1_700_000_000);
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
        let result = sync_pi_files(&db, &[parent.clone(), fork.clone()]);
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 1);

        // A fork is also a complete recovery source when its parent file is
        // absent from a fresh database.
        let orphan_db = Database::memory()?;
        assert_eq!(
            sync_pi_files(&orphan_db, std::slice::from_ref(&fork)).imported,
            2
        );

        let second_pass = sync_pi_files(&db, &[parent, fork]);
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
            sync_pi_files(&db, std::slice::from_ref(&parent)).imported,
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
            sync_pi_files(&db, std::slice::from_ref(&parent)).imported,
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&fork)).imported, 1);

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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&fork)).imported, 0);
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 2);
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

        let parsed = parse_pi_file(&path, 0, None, snapshot_size, modified)?;
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
        let first = sync_pi_files(&db, std::slice::from_ref(&path));
        assert_eq!((first.imported, first.deferred_files), (1, 0));
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 0);
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 1);

        let before = assistant_line("before", "2023-11-14T22:13:21Z", 2);
        let after = assistant_line("after", "2023-11-14T22:13:23Z", 3);
        write_lines(&path, &[header, &before, &first, &after]);
        let rewritten = sync_pi_files(&db, std::slice::from_ref(&path));
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 0);
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 1);
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
            sync_pi_files(&db, std::slice::from_ref(&current)).imported,
            1
        );
        write_lines(&current, &[header, &eleven]);
        let correction = sync_pi_files(&db, std::slice::from_ref(&current));
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
            sync_pi_files(&legacy_db, std::slice::from_ref(&legacy)).imported,
            1
        );
        write_lines(&legacy, &[header, &reordered]);
        let reordered_result = sync_pi_files(&legacy_db, std::slice::from_ref(&legacy));
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
        let deferred = sync_pi_files(&db, std::slice::from_ref(&path));
        assert_eq!(deferred.imported, 0);
        assert_eq!(deferred.deferred_files, 1);
        let deferred_state =
            get_pi_sync_state(&db, path.to_string_lossy().as_ref())?.expect("deferred sync state");
        assert!(!deferred_state.revision.complete);
        let unchanged = sync_pi_files(&db, std::slice::from_ref(&path));
        assert_eq!((unchanged.imported, unchanged.deferred_files), (0, 0));

        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("append session");
            file.write_all(&first_bytes[split..]).expect("finish row");
            writeln!(file).expect("finish newline");
        }
        let completed = sync_pi_files(&db, std::slice::from_ref(&path));
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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 1);

        // Rewrite to fewer lines than the saved cursor. Already billed rows
        // remain, while the new record is found even with a preserved mtime.
        write_lines(&path, &[header, &third]);
        set_modified(&path, preserved_mtime);
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 1);

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
        assert_eq!(sync_pi_files(&db, std::slice::from_ref(&path)).imported, 1);
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
        let result = sync_pi_files(&db, std::slice::from_ref(&path));
        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("安全上限"));
        Ok(())
    }
}
