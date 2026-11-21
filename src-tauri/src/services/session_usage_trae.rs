//! Import TRAE CLI usage from `token_usage_record` lines in its rollouts.
//! Each record carries one response's usage; `input_tokens` includes cached
//! input (OpenAI semantics), so it is normalized to fresh input here.
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
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

pub fn sync_trae_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_root(
        db,
        &crate::trae_config::data_dir().join("cli").join("sessions"),
    )
}

fn rollouts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rollouts(&path, depth + 1, out);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            out.push(path);
        }
    }
}

fn sync_root(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    let mut files = Vec::new();
    rollouts(root, 0, &mut files);
    for path in files {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata_modified_nanos(&metadata);
        let key = format!("trae:{}", path.to_string_lossy());
        if cursors.get(&key).is_some_and(|cursor| {
            cursor.last_modified == modified && cursor.last_line_offset == metadata.len() as i64
        }) {
            continue;
        }
        match sync_file(db, &path, &key, modified) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("TRAE CLI session {}: {error}", path.display())),
        }
    }
    Ok(result)
}

struct Pending {
    request: String,
    session: String,
    model: String,
    usage: TokenUsage,
    created_ms: i64,
}

fn sync_file(
    db: &Database,
    path: &Path,
    key: &str,
    modified: i64,
) -> Result<SessionSyncResult, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = 0i64;
    let mut session = String::new();
    let mut models: HashMap<String, String> = HashMap::new();
    let mut last_model = String::new();
    let mut pending = Vec::new();
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
        let payload = &record["payload"];
        match record["type"].as_str() {
            Some("session_meta") => {
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    session = id.into();
                }
            }
            Some("turn_context") => {
                if let Some(model) = payload.get("model").and_then(Value::as_str) {
                    last_model = model.into();
                    if let Some(turn) = payload.get("turn_id").and_then(Value::as_str) {
                        models.insert(turn.into(), model.into());
                    }
                }
            }
            Some("token_usage_record") => {
                let Some(response) = payload
                    .get("response_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    continue;
                };
                let usage = &payload["usage"];
                let tokens = |field| usage[field].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
                let cached = tokens("cached_input_tokens");
                let created = tokens("cache_creation_input_tokens");
                let usage = TokenUsage {
                    input_tokens: tokens("input_tokens")
                        .saturating_sub(cached.saturating_add(created)),
                    output_tokens: tokens("output_tokens"),
                    cache_read_tokens: cached,
                    cache_creation_tokens: created,
                    cache_creation_1h_tokens: 0,
                    model: None,
                    message_id: None,
                };
                if usage.input_tokens == 0
                    && usage.output_tokens == 0
                    && usage.cache_read_tokens == 0
                    && usage.cache_creation_tokens == 0
                {
                    continue;
                }
                let Some(created_ms) = record
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                    .map(|time| time.timestamp_millis())
                else {
                    continue;
                };
                let model = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .and_then(|turn| models.get(turn))
                    .cloned()
                    .unwrap_or_else(|| last_model.clone());
                let session_id = payload
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or(&session)
                    .to_string();
                pending.push(Pending {
                    request: format!("trae:{session_id}:{response}"),
                    session: session_id,
                    model: if model.is_empty() {
                        "unknown".into()
                    } else {
                        model
                    },
                    usage,
                    created_ms,
                });
            }
            _ => {}
        }
    }
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for mut entry in pending {
        // Permanent receipts prevent re-import after rollout rewrites or pruning.
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('trae_session', ?1, ?1, 1)",
            [&entry.request],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        entry.usage.model = Some(entry.model.clone());
        let amounts = costs(&tx, &entry.model, &entry.usage);
        tx.execute("INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
        ) VALUES (?1, '_trae_session', 'trae', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, NULL, 200, ?12, 'trae_session', 1, '1', ?13, 'trae_session', ?14)",
            rusqlite::params![entry.request, entry.model, entry.usage.input_tokens, entry.usage.output_tokens, entry.usage.cache_read_tokens, entry.usage.cache_creation_tokens,
                amounts[0], amounts[1], amounts[2], amounts[3], amounts[4], entry.session, entry.created_ms / 1000, INPUT_TOKEN_SEMANTICS_FRESH])?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app("trae", usage, &pricing, Decimal::ONE);
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

    fn rollout(dir: &Path) -> PathBuf {
        let path = dir.join("2026/10/23/rollout-a.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let usage = |response: &str, input: u64, cached: u64, output: u64| {
            json!({"timestamp":"2026-10-23T06:36:44.000Z","type":"token_usage_record","payload":{
                "session_id":"s1","turn_id":"t1","response_id":response,
                "usage":{"input_tokens":input,"cache_creation_input_tokens":0,"cached_input_tokens":cached,"output_tokens":output,"reasoning_output_tokens":0,"total_tokens":input+output}}})
        };
        let lines = [
            json!({"type":"session_meta","payload":{"id":"s1","cwd":"/work"}}),
            json!({"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5"}}),
            usage("resp_a", 1000, 0, 20),
            usage("resp_b", 1100, 900, 15),
        ];
        fs::write(
            &path,
            lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        path
    }

    #[test]
    fn trae_usage_imports_each_response_once_with_fresh_input() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        rollout(dir.path());
        let first = sync_root(&db, dir.path()).unwrap();
        assert_eq!(first.imported, 2);
        let rows: Vec<(String, String, i64, i64, i64)> = {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT request_id, model, input_tokens, cache_read_tokens, output_tokens FROM proxy_request_logs WHERE app_type = 'trae' ORDER BY request_id")
                .unwrap();
            stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };
        assert_eq!(
            rows,
            [
                ("trae:s1:resp_a".into(), "gpt-5".into(), 1000, 0, 20),
                ("trae:s1:resp_b".into(), "gpt-5".into(), 200, 900, 15)
            ]
        );
        // Unchanged files are skipped by cursor; rewritten files dedupe by receipt.
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 0);
        let path = rollout(dir.path());
        fs::write(&path, fs::read_to_string(&path).unwrap() + "\n").unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 0);
    }
}
