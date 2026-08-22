//! DSH coding-agent session usage importer.
//!
//! DSH persists each session as a zstd-compressed JSONL file under
//! `~/.dsh/sessions/<encoded-cwd>/<session-id>/session.jsonl.zstd`. Assistant
//! messages carry normalized Anthropic-style usage (fresh input tokens, no
//! cache-write bucket). This importer replays those files into the shared
//! dashboard; the durable `session_usage_dedup` ledger keyed by
//! `dsh:session-<id>:<seq>` makes full reparses idempotent and recognizes rows
//! written by earlier tooling with the same identity scheme.

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
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const APP_TYPE: &str = "dsh";
const DATA_SOURCE: &str = "dsh_session";
const PROVIDER_PLACEHOLDER: &str = "_dsh_session";
const UNKNOWN_MODEL: &str = "unknown";
const MAX_USAGE_LABEL_BYTES: usize = 512;
const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;
const MIN_SQLITE_UNIX_SECONDS: i64 = -62_167_219_200;
const MAX_SQLITE_UNIX_SECONDS: i64 = 253_402_300_799;
const REQUEST_ID_PREFIX: &str = "dsh:";
const DSH_REQUEST_DEDUP_SQL: &str = "SELECT EXISTS(
         SELECT 1 FROM session_usage_dedup
         WHERE data_source = ?1 AND request_id = ?2
     )";

#[derive(Debug)]
struct DshUsageRecord {
    request_id: String,
    provider_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    created_at: i64,
    session_id: String,
}

#[derive(Debug, Default)]
struct ParsedDshFile {
    session_id: Option<String>,
    records: Vec<DshUsageRecord>,
    incomplete_tail: bool,
}

/// Import usage from every DSH session log discoverable under the sessions
/// root. A missing root simply means there is nothing to import yet.
pub fn sync_dsh_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let root = dsh_sessions_root();
    Ok(sync_dsh_files(db, &dsh_session_files(&root)))
}

fn dsh_sessions_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("DSH_SESSIONS_DIR") {
        let trimmed = custom.to_string_lossy().trim().to_string();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    crate::config::get_home_dir().join(".dsh").join("sessions")
}

fn dsh_session_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(cwd_dirs) = fs::read_dir(root) else {
        return files;
    };
    for cwd_dir in cwd_dirs.flatten() {
        let Ok(session_dirs) = fs::read_dir(cwd_dir.path()) else {
            continue;
        };
        for session_dir in session_dirs.flatten() {
            let candidate = session_dir.path().join("session.jsonl.zstd");
            if candidate.is_file() {
                files.push(candidate);
            }
        }
    }
    files.sort();
    files
}

fn sync_dsh_files(db: &Database, files: &[PathBuf]) -> SessionSyncResult {
    let mut result = SessionSyncResult {
        files_scanned: files.len().min(u32::MAX as usize) as u32,
        ..Default::default()
    };

    for file_path in files {
        match sync_single_dsh_file(db, file_path) {
            Ok(file_result) => result.merge(file_result),
            Err(error) => {
                let message = format!("{}: {error}", file_path.display());
                log::warn!("[DSH-SYNC] 会话文件解析失败: {message}");
                result.errors.push(message);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[DSH-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }
    result
}

fn sync_single_dsh_file(db: &Database, file_path: &Path) -> Result<SessionSyncResult, AppError> {
    let metadata = fs::symlink_metadata(file_path)
        .map_err(|error| AppError::Config(format!("无法读取 DSH 会话文件元数据: {error}")))?;
    if !metadata.file_type().is_file() {
        return Err(AppError::Config("DSH 会话路径不是普通文件".to_string()));
    }
    let modified = metadata_modified_nanos(&metadata);
    let file_path_string = file_path.to_string_lossy().to_string();
    if dsh_file_unchanged(db, &file_path_string, modified)? {
        return Ok(SessionSyncResult::default());
    }

    let parsed = parse_dsh_file(file_path, modified / 1_000_000_000)?;
    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 DSH 用量导入事务失败: {error}")))?;
    let mut result = SessionSyncResult::default();
    for record in &parsed.records {
        if insert_dsh_record(&tx, record)? {
            result.imported = result.imported.saturating_add(1);
        } else {
            result.skipped = result.skipped.saturating_add(1);
        }
    }

    update_sync_state_on_conn(
        &tx,
        &file_path_string,
        modified,
        parsed.records.len().min(i64::MAX as usize) as i64,
    )?;
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 DSH 用量导入事务失败: {error}")))?;
    if parsed.incomplete_tail {
        result.deferred_files = 1;
    }
    Ok(result)
}

/// DSH rewrites its zstd logs in full, so a file whose mtime has not advanced
/// since the last pass cannot contain new events.
fn dsh_file_unchanged(db: &Database, file_path: &str, modified: i64) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    let last_modified: Option<i64> = conn
        .query_row(
            "SELECT last_modified FROM session_log_sync WHERE file_path = ?1",
            rusqlite::params![file_path],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| AppError::Database(format!("读取 DSH 会话同步状态失败: {error}")))?;
    Ok(last_modified.is_some_and(|last| modified <= last))
}

fn parse_dsh_file(file_path: &Path, file_modified_seconds: i64) -> Result<ParsedDshFile, AppError> {
    let file = File::open(file_path)
        .map_err(|error| AppError::Config(format!("无法打开 DSH 会话文件: {error}")))?;
    let decoder = zstd::stream::read::Decoder::new(file)
        .map_err(|error| AppError::Config(format!("无法解压 DSH 会话文件: {error}")))?;
    let mut reader = BufReader::new(decoder);
    let mut buffer = String::new();
    let mut parsed = ParsedDshFile::default();
    let mut decompressed_bytes: u64 = 0;

    loop {
        buffer.clear();
        let read = reader
            .read_line(&mut buffer)
            .map_err(|error| AppError::Config(format!("无法读取 DSH 会话文件: {error}")))?;
        if read == 0 {
            break;
        }
        decompressed_bytes = decompressed_bytes.saturating_add(read as u64);
        if decompressed_bytes > MAX_SESSION_BYTES {
            return Err(AppError::Config(format!(
                "DSH 会话解压后超过 {} 字节安全上限",
                MAX_SESSION_BYTES
            )));
        }
        let has_newline = buffer.ends_with('\n');
        let line = buffer.trim();
        if line.is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) if !has_newline => {
                // A torn final line means DSH is mid-rewrite; keep the events
                // parsed so far and retry the tail on the next pass.
                parsed.incomplete_tail = true;
                break;
            }
            Err(_) => continue,
        };

        if parsed.session_id.is_none() {
            if value.get("type").and_then(Value::as_str) != Some("session") {
                return Err(AppError::Config(
                    "DSH 会话的首条有效 JSON 不是 session header".to_string(),
                ));
            }
            let session_id = value
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(truncate_usage_label)
                .ok_or_else(|| AppError::Config("DSH 会话 header 缺少 id".to_string()))?;
            parsed.session_id = Some(session_id.to_string());
            continue;
        }

        if value.get("type").and_then(Value::as_str) != Some("assistant/message") {
            continue;
        }
        let Some(data) = value.get("data").filter(|data| data.get("usage").is_some()) else {
            continue;
        };
        let Some(record) = parse_usage_record(
            data,
            value.get("seq").and_then(Value::as_i64),
            value.get("time").and_then(Value::as_i64),
            parsed.session_id.as_deref().unwrap_or_default(),
            file_modified_seconds,
        ) else {
            continue;
        };
        parsed.records.push(record);
    }

    if parsed.session_id.is_none() && !parsed.incomplete_tail {
        return Err(AppError::Config("DSH 会话没有有效 header".to_string()));
    }
    Ok(parsed)
}

fn parse_usage_record(
    data: &Value,
    seq: Option<i64>,
    time_millis: Option<i64>,
    session_id: &str,
    file_modified_seconds: i64,
) -> Option<DshUsageRecord> {
    let seq = seq?;
    let usage = data.get("usage")?;
    let input_tokens = token_count(usage, "inputTokens");
    let output_tokens = token_count(usage, "outputTokens");
    let cache_read_tokens = token_count(usage, "cacheReadTokens");

    let source = data.pointer("/message/source");
    let provider_id = source
        .and_then(|source| source.get("provider"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(truncate_usage_label)
        .unwrap_or(PROVIDER_PLACEHOLDER)
        .to_string();
    let model = source
        .and_then(|source| source.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(truncate_usage_label)
        .unwrap_or(UNKNOWN_MODEL)
        .to_string();

    // DSH reports epoch milliseconds; `time` is absent only for synthetic or
    // hand-edited logs, where the file mtime is the best available estimate.
    let created_at = time_millis
        .map(|time| time.div_euclid(1000))
        .unwrap_or(file_modified_seconds)
        .clamp(MIN_SQLITE_UNIX_SECONDS, MAX_SQLITE_UNIX_SECONDS);

    Some(DshUsageRecord {
        request_id: format!("{REQUEST_ID_PREFIX}{session_id}:{seq}"),
        provider_id,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        created_at,
        session_id: session_id.to_string(),
    })
}

fn token_count(usage: &Value, key: &str) -> u32 {
    usage
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
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

fn insert_dsh_record(
    conn: &rusqlite::Connection,
    record: &DshUsageRecord,
) -> Result<bool, AppError> {
    let already_seen: bool = conn
        .query_row(
            DSH_REQUEST_DEDUP_SQL,
            rusqlite::params![DATA_SOURCE, record.request_id],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(format!("查询 DSH 用量去重账本失败: {error}")))?;
    if already_seen {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR IGNORE INTO session_usage_dedup
         (data_source, request_id, semantic_id, has_entry_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![DATA_SOURCE, record.request_id, record.request_id, 1i64],
    )
    .map_err(|error| AppError::Database(format!("写入 DSH 用量去重账本失败: {error}")))?;

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: 0,
        model: Some(record.model.clone()),
        message_id: None,
    };
    let costs = find_model_pricing(conn, &record.model).map(|pricing| {
        let calculated =
            CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::ONE);
        (
            calculated.input_cost,
            calculated.output_cost,
            calculated.cache_read_cost,
            calculated.cache_creation_cost,
            calculated.total_cost,
        )
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
            record.model,
            record.model,
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            0i64,
            INPUT_TOKEN_SEMANTICS_FRESH,
            input_cost.to_string(),
            output_cost.to_string(),
            cache_read_cost.to_string(),
            cache_write_cost.to_string(),
            total_cost.to_string(),
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            record.session_id,
            Some(DATA_SOURCE),
            0i64,
            "1.0",
            record.created_at,
            DATA_SOURCE,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|error| AppError::Database(format!("插入 DSH 会话用量失败: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_zstd_session(path: &Path, lines: &[String]) {
        let jsonl = lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        let compressed = zstd::stream::encode_all(jsonl.as_bytes(), 0).expect("encode session");
        fs::write(path, compressed).expect("write session");
    }

    fn session_header(id: &str) -> String {
        format!(
            r#"{{"type":"session","version":0,"id":"{id}","createdAt":1786678151825,"cwd":"/work"}}"#
        )
    }

    fn assistant_line(seq: i64, time: i64, input: u32, output: u32, cache_read: u32) -> String {
        format!(
            r#"{{"type":"assistant/message","seq":{seq},"time":{time},"data":{{"turn":1,"step":1,"message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"source":{{"kind":"model","provider":"stepfun-plan","model":"step-3.7-flash"}}}},"usage":{{"inputTokens":{input},"outputTokens":{output},"cacheReadTokens":{cache_read}}}}}}}"#
        )
    }

    fn session_file(root: &Path, encoded_cwd: &str, session_id: &str) -> PathBuf {
        let path = root
            .join(encoded_cwd)
            .join(session_id)
            .join("session.jsonl.zstd");
        fs::create_dir_all(path.parent().expect("session dir")).expect("create session dir");
        path
    }

    #[test]
    fn dedup_lookup_uses_complete_identity_index() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {DSH_REQUEST_DEDUP_SQL}"))?;
        let plan = statement
            .query_map(rusqlite::params![DATA_SOURCE, "identity"], |row| {
                row.get::<_, String>(3)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(
            plan.iter()
                .any(|step| step.contains("(data_source=? AND request_id=?)")),
            "lookup does not constrain the complete identity: {plan:?}"
        );
        Ok(())
    }

    #[test]
    fn imports_usage_events_with_fresh_semantics_and_legacy_request_ids() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_file(temp.path(), "--work--", "session-abc-123");
        write_zstd_session(
            &path,
            &[
                session_header("session-abc-123"),
                r#"{"type":"permission/preset","seq":0,"time":1786678151831,"data":{"preset":"workspace-write"}}"#.to_string(),
                assistant_line(7, 1786678145825, 7965, 139, 256),
                assistant_line(8, 1786678148978, 1534, 187, 8128),
                r#"{"type":"assistant/message","seq":9,"time":1786678149999,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[]}}}"#.to_string(),
            ],
        );

        let db = Database::memory()?;
        let result = sync_dsh_files(&db, std::slice::from_ref(&path));
        assert_eq!(result.imported, 2);
        assert!(result.errors.is_empty());

        {
            let conn = lock_conn!(db.conn);
            let identity: (String, String, String, String, String, String, String) = conn
                .query_row(
                    "SELECT request_id, provider_id, model, app_type, session_id,
                            provider_type, data_source
                     FROM proxy_request_logs WHERE request_id = 'dsh:session-abc-123:7'",
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
            assert_eq!(identity.0, "dsh:session-abc-123:7");
            assert_eq!(identity.1, "stepfun-plan");
            assert_eq!(identity.2, "step-3.7-flash");
            assert_eq!(identity.3, "dsh");
            assert_eq!(identity.4, "session-abc-123");
            assert_eq!(identity.5, "dsh_session");
            assert_eq!(identity.6, "dsh_session");

            let counters: (i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, input_token_semantics, created_at,
                        is_streaming
                 FROM proxy_request_logs WHERE request_id = 'dsh:session-abc-123:7'",
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
            assert_eq!((counters.0, counters.1, counters.2), (7965, 139, 256));
            assert_eq!(counters.3, 0);
            assert_eq!(counters.4, INPUT_TOKEN_SEMANTICS_FRESH);
            assert_eq!(counters.5, 1_786_678_145);
            assert_eq!(counters.6, 0);

            let dedup: (String, String, i64) = conn.query_row(
                "SELECT request_id, semantic_id, has_entry_id FROM session_usage_dedup
                 WHERE data_source = 'dsh_session' ORDER BY request_id",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(
                dedup,
                (
                    "dsh:session-abc-123:7".to_string(),
                    "dsh:session-abc-123:7".to_string(),
                    1
                )
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_backfill_ledger_rows_are_recognized_and_not_reimported() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_file(temp.path(), "--work--", "session-legacy-42");
        write_zstd_session(
            &path,
            &[
                session_header("session-legacy-42"),
                assistant_line(70473, 1786776034999, 6, 1107, 148864),
                assistant_line(70480, 1786776041000, 12, 34, 56),
            ],
        );

        // Rows written by the pre-upstream backfill tool use the same
        // `dsh:session-<id>:<seq>` identity; replaying must add nothing.
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for request_id in ["dsh:session-legacy-42:70473", "dsh:session-legacy-42:70480"] {
                conn.execute(
                    "INSERT OR IGNORE INTO session_usage_dedup
                     (data_source, request_id, semantic_id, has_entry_id)
                     VALUES ('dsh_session', ?1, ?1, 1)",
                    rusqlite::params![request_id],
                )?;
            }
        }
        let result = sync_dsh_files(&db, std::slice::from_ref(&path));
        assert_eq!((result.imported, result.skipped), (0, 2));
        let count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'dsh_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    fn missing_provider_falls_back_to_named_placeholder() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_file(temp.path(), "--work--", "session-anon-1");
        let event = r#"{"type":"assistant/message","seq":3,"time":1786678145825,"data":{"turn":1,"step":1,"message":{"role":"assistant","content":[]},"usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":2}}}"#;
        write_zstd_session(
            &path,
            &[session_header("session-anon-1"), event.to_string()],
        );

        let db = Database::memory()?;
        assert_eq!(sync_dsh_files(&db, std::slice::from_ref(&path)).imported, 1);

        let conn = lock_conn!(db.conn);
        let (provider_id, model): (String, String) = conn.query_row(
            "SELECT provider_id, model FROM proxy_request_logs
             WHERE data_source = 'dsh_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(provider_id, PROVIDER_PLACEHOLDER);
        assert_eq!(model, UNKNOWN_MODEL);
        drop(conn);

        let providers = db.get_provider_stats(None, None, Some(APP_TYPE), None, None)?;
        assert!(providers.iter().any(|provider| {
            provider.provider_id == PROVIDER_PLACEHOLDER
                && provider.provider_name == "DSH (Session)"
        }));
        Ok(())
    }

    #[test]
    fn rewritten_file_reimports_only_new_events() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = session_file(temp.path(), "--work--", "session-grow-9");
        write_zstd_session(
            &path,
            &[
                session_header("session-grow-9"),
                assistant_line(1, 1786678145825, 100, 10, 1000),
            ],
        );

        let db = Database::memory()?;
        assert_eq!(sync_dsh_files(&db, std::slice::from_ref(&path)).imported, 1);
        // Unchanged mtime: the file is skipped entirely.
        let unchanged = sync_dsh_files(&db, std::slice::from_ref(&path));
        assert_eq!((unchanged.imported, unchanged.skipped), (0, 0));

        write_zstd_session(
            &path,
            &[
                session_header("session-grow-9"),
                assistant_line(1, 1786678145825, 100, 10, 1000),
                assistant_line(2, 1786678148978, 200, 20, 2000),
            ],
        );
        // Same-size rewrites can keep the mtime; force a bump to model DSH's
        // full-rewrite behaviour and rely on the ledger for the old event.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open session for timestamp bump")
            .set_times(std::fs::FileTimes::new().set_modified(later))
            .expect("bump mtime");
        let grown = sync_dsh_files(&db, std::slice::from_ref(&path));
        assert_eq!((grown.imported, grown.skipped), (1, 1));

        let count: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'dsh_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }
}
