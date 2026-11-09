//! Import GitHub Copilot CLI request usage from `session.usage_record` events.
//!
//! Each record is one model call with exact counters. `inputTokens` includes cache reads, so
//! the fresh input comes from `copilotUsage.tokenDetails` (`input`) when present, otherwise
//! from `inputTokens - cacheReadTokens - cacheWriteTokens`. Older CLI versions do not write
//! these events; their sessions are not imported rather than estimated from totals.
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
    io::{BufRead, BufReader},
    path::Path,
};

pub fn sync_copilot_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_sessions(db, &crate::copilot_config::session_state_dir())
}

fn sync_sessions(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    let Ok(sessions) = fs::read_dir(root) else {
        return Ok(result);
    };
    for session in sessions.flatten() {
        let path = session.path().join("events.jsonl");
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = metadata_modified_nanos(&meta);
        let key = format!("copilot:v1:{}", path.to_string_lossy());
        if cursors.get(&key).is_some_and(|cursor| {
            cursor.last_modified == modified && cursor.last_line_offset == meta.len() as i64
        }) {
            continue;
        }
        match sync_file(
            db,
            &path,
            &session.file_name().to_string_lossy(),
            &key,
            modified,
        ) {
            Ok(part) => result.merge(part),
            Err(error) => result.errors.push(format!(
                "GitHub Copilot CLI session {}: {error}",
                path.display()
            )),
        }
    }
    Ok(result)
}

struct Record {
    request: String,
    model: String,
    usage: TokenUsage,
    created_ms: i64,
    latency_ms: i64,
    first_token_ms: Option<i64>,
}

fn record_of(session: &str, event: &Value) -> Option<Record> {
    if event["type"] != "session.usage_record" {
        return None;
    }
    let usage = &event["data"]["usage"];
    let count = |field: &str| usage[field].as_u64().unwrap_or(0);
    let detail = |kind: &str| {
        usage
            .pointer("/copilotUsage/tokenDetails")
            .and_then(Value::as_array)?
            .iter()
            .find(|item| item["tokenType"] == kind)?["tokenCount"]
            .as_u64()
    };
    let cache_read = detail("cache_read").unwrap_or_else(|| count("cacheReadTokens"));
    let cache_write = detail("cache_write").unwrap_or_else(|| count("cacheWriteTokens"));
    let input = detail("input").unwrap_or_else(|| {
        count("inputTokens")
            .saturating_sub(cache_read)
            .saturating_sub(cache_write)
    });
    let output = detail("output").unwrap_or_else(|| count("outputTokens"));
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    let clamp = |value: u64| value.min(u32::MAX as u64) as u32;
    let model = usage["model"]
        .as_str()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or("unknown")
        .to_string();
    let identity = event["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            usage
                .pointer("/accounting/sequence")
                .and_then(Value::as_u64)
                .map(|sequence| format!("seq{sequence}"))
        })?;
    let created_ms = event["timestamp"]
        .as_str()
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|time| time.timestamp_millis())?;
    let first_token_ms = usage["timeToFirstTokenMs"]
        .as_f64()
        .filter(|ms| *ms > 0.0)
        .map(|ms| ms.round() as i64);
    Some(Record {
        request: format!("copilot:{session}:{identity}"),
        usage: TokenUsage {
            input_tokens: clamp(input),
            output_tokens: clamp(output),
            cache_read_tokens: clamp(cache_read),
            cache_creation_tokens: clamp(cache_write),
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        },
        model,
        created_ms,
        latency_ms: usage["duration"].as_i64().unwrap_or(0).max(0),
        first_token_ms,
    })
}

fn sync_file(
    db: &Database,
    path: &Path,
    session: &str,
    key: &str,
    modified: i64,
) -> Result<SessionSyncResult, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = 0i64;
    let mut records = Vec::new();
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        if size == 0 {
            break;
        }
        // A live writer may have flushed only part of its last record. Retry it next round.
        if buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        let Ok(event) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        if let Some(record) = record_of(session, &event) {
            records.push(record);
        }
    }
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for record in records {
        // Permanent receipts prevent re-import after log pruning.
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('copilot_session', ?1, ?1, 1)",
            [&record.request],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let amounts = costs(&tx, &record.model, &record.usage);
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
        ) VALUES (?1, '_copilot_session', 'copilot', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?15, ?16, 200, ?12, 'copilot_session', 1, '1', ?13, 'copilot_session', ?14)",
            rusqlite::params![
                record.request,
                record.model,
                record.usage.input_tokens,
                record.usage.output_tokens,
                record.usage.cache_read_tokens,
                record.usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                session,
                record.created_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
                record.latency_ms,
                record.first_token_ms
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

/// 五个金额（输入、输出、缓存读、缓存写、合计）；查不到价格的记 0，不代表免费。
fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app("copilot", usage, &pricing, Decimal::ONE);
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
    use serde_json::json;

    fn write_session(root: &Path, events: &[Value]) -> std::path::PathBuf {
        let dir = root.join("session-1");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        fs::write(
            &path,
            events
                .iter()
                .map(|event| format!("{event}\n"))
                .collect::<String>(),
        )
        .unwrap();
        path
    }

    fn usage_event(id: &str, details: bool) -> Value {
        let mut usage = json!({
            "model":"gpt-5-mini","inputTokens":25301,"outputTokens":85,
            "cacheReadTokens":7680,"cacheWriteTokens":0,"duration":2988,"timeToFirstTokenMs":2906.8
        });
        if details {
            usage["copilotUsage"] = json!({"tokenDetails":[
                {"tokenType":"input","tokenCount":17621},
                {"tokenType":"cache_read","tokenCount":7680},
                {"tokenType":"cache_write","tokenCount":0},
                {"tokenType":"output","tokenCount":85}
            ]});
        }
        json!({"type":"session.usage_record","id":id,"timestamp":"2026-10-10T09:08:03.054Z","data":{"usage":usage}})
    }

    #[test]
    fn copilot_usage_imports_fresh_input_once() {
        let db = Database::memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        write_session(
            root.path(),
            &[
                json!({"type":"user.message","timestamp":"2026-10-10T09:08:00.000Z","data":{"content":"hi"}}),
                usage_event("u1", true),
                usage_event("u2", false),
            ],
        );
        let result = sync_sessions(&db, root.path()).unwrap();
        assert_eq!(result.imported, 2);
        {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT input_tokens, cache_read_tokens, output_tokens, latency_ms, first_token_ms, app_type, input_token_semantics FROM proxy_request_logs ORDER BY request_id")
                .unwrap();
            type Row = (i64, i64, i64, i64, Option<i64>, String, i64);
            let rows: Vec<Row> = stmt
                .query_map([], |row| {
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
                .collect();
            for row in &rows {
                assert_eq!(
                    row,
                    &(
                        17621,
                        7680,
                        85,
                        2988,
                        Some(2907),
                        "copilot".to_string(),
                        INPUT_TOKEN_SEMANTICS_FRESH
                    )
                );
            }
        }
        // Unchanged files are skipped; rewritten files never import the same record twice.
        assert_eq!(sync_sessions(&db, root.path()).unwrap().imported, 0);
        write_session(
            root.path(),
            &[
                usage_event("u1", true),
                usage_event("u2", false),
                usage_event("u3", true),
            ],
        );
        let result = sync_sessions(&db, root.path()).unwrap();
        assert_eq!((result.imported, result.skipped), (1, 2));
    }

    /// 用真实 Copilot CLI 跑出的数据目录核对会话和用量解析：
    /// `CCS_COPILOT_SMOKE_HOME=<COPILOT_HOME> cargo test --lib real_copilot_home -- --ignored`
    #[test]
    #[ignore]
    fn real_copilot_home_sessions_and_usage_parse() {
        let home = std::path::PathBuf::from(
            std::env::var_os("CCS_COPILOT_SMOKE_HOME").expect("CCS_COPILOT_SMOKE_HOME"),
        );
        let root = home.join("session-state");
        let db = Database::memory().unwrap();
        let result = sync_sessions(&db, &root).unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.imported > 0);
        for entry in fs::read_dir(&root).unwrap().flatten() {
            let events = entry.path().join("events.jsonl");
            if !events.is_file() {
                continue;
            }
            let messages =
                crate::session_manager::providers::copilot::load_messages(&events).unwrap();
            assert!(messages.iter().any(|m| m.role == "user"));
            assert!(messages.iter().any(|m| m.role == "assistant"));
        }
    }

    #[test]
    fn copilot_usage_ignores_partial_tail_and_empty_records() {
        let db = Database::memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = write_session(
            root.path(),
            &[
                json!({"type":"session.usage_record","id":"zero","timestamp":"2026-10-10T09:08:03.054Z","data":{"usage":{"model":"m"}}}),
            ],
        );
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str(&usage_event("partial", true).to_string());
        fs::write(&path, text).unwrap();
        assert_eq!(sync_sessions(&db, root.path()).unwrap().imported, 0);
    }
}
