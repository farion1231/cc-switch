//! Import Qwen Code's per-request usage from its session logs.
//!
//! Every API call — including internal sub-agent calls that never become an
//! assistant record — is logged as a `system` record with
//! `subtype = "ui_telemetry"` and `systemPayload.uiEvent["event.name"] =
//! "qwen-code.api_response"`, carrying model, token counts, duration and TTFT.
//! Files written before that event existed fall back to assistant
//! `usageMetadata`. `input_token_count` includes cached tokens; rows are stored
//! as fresh input.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::{
    session_usage::{
        load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const API_RESPONSE: &str = "qwen-code.api_response";

fn timestamp_ms(value: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.timestamp_millis())
}

pub fn sync_qwen_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_projects(db, &crate::qwen_config::runtime_dir().join("projects"))
}

fn session_files(projects: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(projects) = fs::read_dir(projects) else {
        return files;
    };
    for project in projects.flatten() {
        let chats = project.path().join("chats");
        // Archived sessions keep their record ids, so receipts dedupe them.
        for dir in [chats.clone(), chats.join("archive")] {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                    files.push(path);
                }
            }
        }
    }
    files
}

fn sync_projects(db: &Database, projects: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    for path in session_files(projects) {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata_modified_nanos(&meta);
        let key = format!("qwen:{}", path.to_string_lossy());
        let cursor = cursors.get(&key);
        if cursor
            .is_some_and(|c| c.last_modified == modified && c.last_line_offset == meta.len() as i64)
        {
            continue;
        }
        // Append-only logs resume from the last complete line; a shorter file was
        // rewritten, so read it again (receipts keep requests from doubling).
        let offset = cursor
            .map(|c| c.last_line_offset)
            .filter(|offset| *offset >= 0 && *offset <= meta.len() as i64)
            .unwrap_or(0);
        match sync_file(db, &path, &key, modified, offset) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("Qwen Code session {}: {error}", path.display())),
        }
    }
    Ok(result)
}

struct Entry {
    request_id: String,
    session_id: String,
    model: String,
    usage: TokenUsage,
    created_ms: i64,
    latency_ms: i64,
    first_token_ms: Option<i64>,
    status: i64,
}

fn count(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

fn usage_from(
    model: &str,
    input_total: u32,
    output: u32,
    thoughts: u32,
    cached: u32,
) -> TokenUsage {
    TokenUsage {
        input_tokens: input_total.saturating_sub(cached),
        output_tokens: output.saturating_add(thoughts),
        cache_read_tokens: cached,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        model: Some(model.into()),
        message_id: None,
    }
}

/// An `api_response` telemetry record.
fn telemetry_entry(record: &Value) -> Option<Entry> {
    if record.get("subtype").and_then(Value::as_str) != Some("ui_telemetry") {
        return None;
    }
    let event = record.pointer("/systemPayload/uiEvent")?;
    if event.get("event.name").and_then(Value::as_str) != Some(API_RESPONSE) {
        return None;
    }
    let model = event
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .unwrap_or("unknown");
    let usage = usage_from(
        model,
        count(event, "input_token_count"),
        count(event, "output_token_count"),
        count(event, "thoughts_token_count"),
        count(event, "cached_content_token_count"),
    );
    Some(Entry {
        request_id: String::new(),
        session_id: String::new(),
        model: model.into(),
        usage,
        created_ms: event
            .get("event.timestamp")
            .and_then(timestamp_ms)
            .or_else(|| record.get("timestamp").and_then(timestamp_ms))?,
        latency_ms: event
            .get("duration_ms")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        first_token_ms: event.get("ttft_ms").and_then(Value::as_i64),
        status: event
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(200),
    })
}

/// An assistant record's `usageMetadata` (older logs without telemetry).
fn assistant_entry(record: &Value) -> Option<Entry> {
    if record.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let usage = record.get("usageMetadata")?;
    let model = record
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .unwrap_or("unknown");
    Some(Entry {
        request_id: String::new(),
        session_id: String::new(),
        model: model.into(),
        usage: usage_from(
            model,
            count(usage, "promptTokenCount"),
            count(usage, "candidatesTokenCount"),
            count(usage, "thoughtsTokenCount"),
            count(usage, "cachedContentTokenCount"),
        ),
        created_ms: record.get("timestamp").and_then(timestamp_ms)?,
        latency_ms: 0,
        first_token_ms: None,
        status: 200,
    })
}

fn sync_file(
    db: &Database,
    path: &Path,
    key: &str,
    modified: i64,
    offset: i64,
) -> Result<SessionSyncResult, AppError> {
    let mut file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = offset;
    let mut telemetry = Vec::new();
    let mut assistant = Vec::new();
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        // A live writer may have flushed only part of its last record. Retry it next round.
        if size == 0 || buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        let Ok(record) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        let Some(uuid) = record.get("uuid").and_then(Value::as_str) else {
            continue;
        };
        let session = record
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let (list, entry) = match telemetry_entry(&record) {
            Some(entry) => (&mut telemetry, entry),
            None => match assistant_entry(&record) {
                Some(entry) => (&mut assistant, entry),
                None => continue,
            },
        };
        list.push(Entry {
            request_id: format!("qwen:{session}:{uuid}"),
            session_id: session,
            ..entry
        });
    }
    // Telemetry already covers every assistant turn; using both would double count.
    let has_telemetry = !telemetry.is_empty() || file_has_telemetry(path, offset);
    let entries = if has_telemetry { telemetry } else { assistant };

    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for entry in entries {
        let usage = &entry.usage;
        if usage.input_tokens == 0 && usage.output_tokens == 0 && usage.cache_read_tokens == 0 {
            continue;
        }
        // Permanent receipts prevent re-import after log rewrites or detail pruning.
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('qwen_session', ?1, ?1, 1)",
            [&entry.request_id],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let amounts = costs(&tx, &entry.model, usage);
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, '_qwen_session', 'qwen', ?2, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'qwen_session', 1, '1', ?15, 'qwen_session', ?16)",
            rusqlite::params![
                entry.request_id,
                entry.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                entry.latency_ms,
                entry.first_token_ms,
                entry.status,
                entry.session_id,
                entry.created_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

/// Whether the part of the file before `offset` already used telemetry records.
fn file_has_telemetry(path: &Path, offset: i64) -> bool {
    if offset == 0 {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file).take(offset as u64);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if line.contains(API_RESPONSE) {
            return true;
        }
        line.clear();
    }
    false
}

fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app("qwen", usage, &pricing, Decimal::ONE);
            [
                cost.input_cost,
                cost.output_cost,
                cost.cache_read_cost,
                cost.cache_creation_cost,
                cost.total_cost,
            ]
        })
        .unwrap_or([Decimal::ZERO; 5])
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn telemetry(uuid: &str, model: &str, input: u64, cached: u64, output: u64) -> String {
        serde_json::json!({
            "uuid": uuid, "sessionId": "s1", "timestamp": "2026-10-22T17:29:57.798Z",
            "type": "system", "subtype": "ui_telemetry",
            "systemPayload": {"uiEvent": {
                "event.name": API_RESPONSE, "event.timestamp": "2026-10-22T17:29:57.797Z",
                "model": model, "status_code": 200, "duration_ms": 296, "ttft_ms": 293,
                "input_token_count": input, "output_token_count": output,
                "cached_content_token_count": cached, "thoughts_token_count": 2
            }}
        })
        .to_string()
    }

    const ASSISTANT: &str = r#"{"uuid":"a1","sessionId":"s1","timestamp":"2026-10-22T17:29:57.798Z","type":"assistant","model":"m","message":{"role":"model","parts":[{"text":"hi"}]},"usageMetadata":{"promptTokenCount":1200,"candidatesTokenCount":34,"thoughtsTokenCount":0,"cachedContentTokenCount":1000}}"#;

    /// request_id, model, input, output, cache read, latency, first token
    type Row = (String, String, i64, i64, i64, i64, Option<i64>);

    fn rows(db: &Database) -> Vec<Row> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT request_id, model, input_tokens, output_tokens, cache_read_tokens, latency_ms, first_token_ms FROM proxy_request_logs WHERE app_type = 'qwen' ORDER BY request_id")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn session_file(root: &Path) -> PathBuf {
        let chats = root.join("-proj").join("chats");
        fs::create_dir_all(&chats).unwrap();
        chats.join("s1.jsonl")
    }

    #[test]
    fn qwen_usage_prefers_telemetry_and_resumes_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(
            &path,
            format!(
                "{}\n{ASSISTANT}\n{}\n",
                telemetry("t1", "m", 1200, 1000, 34),
                telemetry("t2", "sub", 300, 0, 10)
            ),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!(first.imported, 2);
        let imported = rows(&db);
        assert_eq!(
            imported[0],
            (
                "qwen:s1:t1".into(),
                "m".into(),
                200,
                36,
                1000,
                296,
                Some(293)
            )
        );
        assert_eq!(imported[1].1, "sub");

        // Unchanged file is skipped; appended telemetry is picked up once.
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{ASSISTANT}").unwrap();
        writeln!(file, "{}", telemetry("t3", "m", 50, 0, 5)).unwrap();
        // A partial trailing line waits for the writer to finish.
        write!(file, "{{\"uuid\":").unwrap();
        drop(file);
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        assert_eq!(rows(&db).len(), 3);
    }

    #[test]
    fn qwen_usage_falls_back_to_assistant_records_without_telemetry() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(&path, format!("{ASSISTANT}\n")).unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        let imported = rows(&db);
        assert_eq!(imported[0].0, "qwen:s1:a1");
        assert_eq!(
            (imported[0].2, imported[0].3, imported[0].4),
            (200, 34, 1000)
        );
        // Rewriting the file does not double count: receipts are permanent.
        fs::write(&path, format!("{ASSISTANT}\n")).unwrap();
        filetime_bump(&path);
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
    }

    fn filetime_bump(path: &Path) {
        let file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
    }
}
