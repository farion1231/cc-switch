//! Import CodeBuddy Code's per-request usage from its session transcripts.
//!
//! Each model request (one `providerData.stepSeq` of an assistant turn) reports its
//! usage once, on the record that ends the step: the assistant `message`, or the
//! first `function_call` when the step only calls tools. `rawUsage` is the
//! provider's OpenAI-style usage: `prompt_tokens` is the whole input including cache
//! hits (`prompt_cache_hit_tokens + prompt_cache_miss_tokens == prompt_tokens`) and
//! `completion_tokens` already includes the thinking tokens. Rows are stored as
//! fresh input (`prompt_tokens - cache read - cache write`) with the cache counts
//! kept separately, so cost uses the normal Anthropic-style formula.
//!
//! Main transcripts live in `<config>/projects/<project>/<session>.jsonl`; teammate
//! and sub-agent transcripts in `<project>/<session>/subagents/*.jsonl`.
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
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Token counts of one CodeBuddy request. `input` excludes cache reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

fn number(value: &Value, pointer: &str) -> u64 {
    value.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

/// Usage of the request a record ends (`providerData.rawUsage`, falling back to the
/// normalized `providerData.usage`). `None` when the record carries no usage.
pub(crate) fn token_counts(provider_data: &Value) -> Option<TokenCounts> {
    if let Some(raw) = provider_data.get("rawUsage").filter(|v| v.is_object()) {
        let prompt = number(raw, "/prompt_tokens");
        let cache_read = [
            "/prompt_cache_hit_tokens",
            "/cache_read_input_tokens",
            "/cached_tokens",
            "/prompt_tokens_details/cached_tokens",
        ]
        .iter()
        .map(|pointer| number(raw, pointer))
        .max()
        .unwrap_or(0);
        let cache_write = number(raw, "/prompt_cache_write_tokens")
            .max(number(raw, "/cache_creation_input_tokens"));
        let reasoning = number(raw, "/completion_thinking_tokens")
            .max(number(raw, "/completion_tokens_details/reasoning_tokens"));
        return Some(TokenCounts {
            input: prompt.saturating_sub(cache_read.saturating_add(cache_write)),
            output: number(raw, "/completion_tokens"),
            cache_read,
            cache_write,
            reasoning,
        });
    }
    let usage = provider_data.get("usage").filter(|v| v.is_object())?;
    let prompt = number(usage, "/inputTokens");
    let cache_read = usage
        .get("inputTokensDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|detail| number(detail, "/cached_tokens"))
        .sum::<u64>();
    let reasoning = usage
        .get("outputTokensDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|detail| number(detail, "/reasoning_tokens"))
        .sum::<u64>();
    Some(TokenCounts {
        input: prompt.saturating_sub(cache_read),
        output: number(usage, "/outputTokens"),
        cache_read,
        cache_write: 0,
        reasoning,
    })
}

/// The model id the request used. Custom models are recorded as
/// `custom-local:<id>` in `requestModelId`; the prefix is dropped so the id matches
/// the pricing table and the model the user configured.
pub(crate) fn request_model(provider_data: &Value) -> Option<String> {
    ["requestModelId", "model"]
        .iter()
        .find_map(|key| provider_data.get(*key).and_then(Value::as_str))
        .map(|model| strip_custom_prefix(model.trim()))
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

/// `custom-local:deepseek-flash` → `deepseek-flash`; other ids are returned as is.
pub(crate) fn strip_custom_prefix(model: &str) -> &str {
    match model.split_once(':') {
        Some((prefix, id)) if prefix.starts_with("custom-") && !id.trim().is_empty() => id.trim(),
        _ => model,
    }
}

pub fn sync_codebuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_projects(
        db,
        &crate::session_manager::providers::codebuddy::projects_dir(),
    )
}

fn usage_files(projects: &Path) -> Vec<PathBuf> {
    let mut files = crate::session_manager::providers::codebuddy::session_files(projects);
    let subagents: Vec<PathBuf> = files
        .iter()
        .filter_map(|main| {
            let stem = main.file_stem()?;
            Some(main.with_file_name(stem).join("subagents"))
        })
        .flat_map(|dir| {
            fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("jsonl"))
                .collect::<Vec<_>>()
        })
        .collect();
    files.extend(subagents);
    files
}

fn sync_projects(db: &Database, projects: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    for path in usage_files(projects) {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata_modified_nanos(&meta);
        let key = format!("codebuddy:{}", path.to_string_lossy());
        let cursor = cursors.get(&key);
        if cursor
            .is_some_and(|c| c.last_modified == modified && c.last_line_offset == meta.len() as i64)
        {
            continue;
        }
        // Transcripts are append-only: resume after the last complete line. A shorter
        // file was rewritten, so read it again (receipts keep requests from doubling).
        let offset = cursor
            .map(|c| c.last_line_offset)
            .filter(|offset| *offset >= 0 && *offset <= meta.len() as i64)
            .unwrap_or(0);
        match sync_file(db, &path, &key, modified, offset) {
            Ok(part) => result.merge(part),
            Err(error) => result.errors.push(format!(
                "CodeBuddy Code session {}: {error}",
                path.display()
            )),
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
}

fn entry(record: &Value) -> Option<Entry> {
    match record.get("type").and_then(Value::as_str) {
        Some("message") if record.get("role").and_then(Value::as_str) == Some("assistant") => {}
        Some("function_call") => {}
        _ => return None,
    }
    let provider = record.get("providerData")?;
    let counts = token_counts(provider)?;
    if counts.input == 0 && counts.output == 0 && counts.cache_read == 0 && counts.cache_write == 0
    {
        return None;
    }
    let session = record
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    // One request per (messageId, stepSeq); fall back to the record id.
    let request = match (
        provider.get("messageId").and_then(Value::as_str),
        provider.get("stepSeq").and_then(Value::as_i64),
    ) {
        (Some(message), Some(step)) => format!("codebuddy:{session}:{message}:{step}"),
        _ => format!(
            "codebuddy:{session}:{}",
            record.get("id").and_then(Value::as_str)?
        ),
    };
    let model = request_model(provider).unwrap_or_else(|| "unknown".into());
    let to_u32 = |n: u64| n.min(u32::MAX as u64) as u32;
    Some(Entry {
        request_id: request,
        session_id: session,
        usage: TokenUsage {
            input_tokens: to_u32(counts.input),
            output_tokens: to_u32(counts.output),
            cache_read_tokens: to_u32(counts.cache_read),
            cache_creation_tokens: to_u32(counts.cache_write),
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        },
        model,
        created_ms: record.get("timestamp").and_then(Value::as_i64)?,
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
    let mut entries = Vec::new();
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        // A live writer may have flushed only part of its last record; retry next round.
        if size == 0 || buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        if let Some(entry) = serde_json::from_slice::<Value>(&buffer)
            .ok()
            .as_ref()
            .and_then(entry)
        {
            entries.push(entry);
        }
    }

    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for entry in entries {
        // Permanent receipts prevent re-import after log rewrites or detail pruning.
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('codebuddy_session', ?1, ?1, 1)",
            [&entry.request_id],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let usage = &entry.usage;
        let amounts = costs(&tx, &entry.model, usage);
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, '_codebuddy_session', 'codebuddy', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, NULL, 200, ?12, 'codebuddy_session', 1, '1', ?13, 'codebuddy_session', ?14)",
            rusqlite::params![
                entry.request_id,
                entry.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
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

fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost =
                CostCalculator::calculate_for_app("codebuddy", usage, &pricing, Decimal::ONE);
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
    use std::io::Write;

    fn call(id: &str, message: &str, step: i64, prompt: u64, hit: u64, output: u64) -> String {
        json!({
            "id": id, "sessionId": "s1", "timestamp": 1791900000000_i64, "type": "function_call",
            "callId": format!("call-{id}"), "name": "Bash", "arguments": "{}",
            "providerData": {"messageId": message, "stepSeq": step, "model": "hy4-preview",
                "requestModelId": "hy4-preview",
                "rawUsage": {"prompt_tokens": prompt, "completion_tokens": output,
                    "prompt_cache_hit_tokens": hit, "prompt_cache_miss_tokens": prompt - hit,
                    "completion_thinking_tokens": 3}}
        })
        .to_string()
    }

    fn assistant(id: &str, step: i64) -> String {
        json!({
            "id": id, "sessionId": "s1", "timestamp": 1791900001000_i64, "type": "message",
            "role": "assistant", "content": [{"type": "output_text", "text": "done"}],
            "providerData": {"messageId": "m1", "stepSeq": step, "model": "glm-5.3",
                "usage": {"requests": 1, "inputTokens": 500, "outputTokens": 7,
                    "inputTokensDetails": [{"cached_tokens": 400}]}}
        })
        .to_string()
    }

    const OTHER: &str = r#"{"id":"r","sessionId":"s1","timestamp":1791900000000,"type":"reasoning","rawContent":[],"providerData":{"messageId":"m1","stepSeq":1}}
{"id":"o","sessionId":"s1","timestamp":1791900000000,"type":"function_call_result","callId":"x","status":"completed","output":{"type":"text","text":"ok"}}
{"id":"c0","sessionId":"s1","timestamp":1791900000000,"type":"function_call","callId":"y","name":"Read","arguments":"{}","providerData":{"messageId":"m1","stepSeq":1}}"#;

    fn rows(db: &Database) -> Vec<(String, String, i64, i64, i64, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT request_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens FROM proxy_request_logs WHERE app_type = 'codebuddy' ORDER BY created_at, request_id")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn session_file(root: &Path) -> PathBuf {
        let project = root.join("Users-me-proj");
        fs::create_dir_all(&project).unwrap();
        project.join("s1.jsonl")
    }

    #[test]
    fn codebuddy_usage_counts_each_request_once_and_resumes_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(
            &path,
            format!(
                "{OTHER}\n{}\n{}\n",
                call("c1", "m1", 1, 1200, 1000, 40),
                assistant("a1", 2)
            ),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!(first.imported, 2);
        assert_eq!(
            rows(&db),
            vec![
                (
                    "codebuddy:s1:m1:1".into(),
                    "hy4-preview".into(),
                    200,
                    40,
                    1000,
                    0
                ),
                ("codebuddy:s1:m1:2".into(), "glm-5.3".into(), 100, 7, 400, 0),
            ]
        );

        // Unchanged file is skipped; appended requests are picked up once.
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", call("c2", "m2", 1, 300, 0, 5)).unwrap();
        // A partial trailing line waits for the writer to finish.
        write!(file, "{{\"id\":").unwrap();
        drop(file);
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        assert_eq!(rows(&db).len(), 3);
    }

    #[test]
    fn codebuddy_usage_reads_subagents_and_never_double_counts_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(&path, format!("{}\n", call("c1", "m1", 1, 100, 0, 1))).unwrap();
        let subagents = path.with_file_name("s1").join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(
            subagents.join("agent-1.jsonl"),
            format!("{}\n", call("c9", "m9", 1, 50, 10, 2)),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 2);
        // Rewriting the file resets the cursor, but receipts are permanent.
        fs::write(&path, format!("{}\n", call("c1", "m1", 1, 100, 0, 1))).unwrap();
        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        let again = sync_projects(&db, dir.path()).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(rows(&db).len(), 2);
    }

    #[test]
    fn codebuddy_token_counts_separate_cache_from_fresh_input() {
        let counts = token_counts(&json!({"rawUsage": {
            "prompt_tokens": 1000, "completion_tokens": 50,
            "cache_read_input_tokens": 600, "cache_creation_input_tokens": 100,
            "completion_tokens_details": {"reasoning_tokens": 20}
        }}))
        .unwrap();
        assert_eq!(
            counts,
            TokenCounts {
                input: 300,
                output: 50,
                cache_read: 600,
                cache_write: 100,
                reasoning: 20
            }
        );
        assert!(token_counts(&json!({"model": "x"})).is_none());
    }

    #[test]
    fn codebuddy_usage_drops_the_custom_model_prefix() {
        let data =
            json!({"model": "deepseek-flash", "requestModelId": "custom-local:deepseek-flash"});
        assert_eq!(request_model(&data).as_deref(), Some("deepseek-flash"));
        assert_eq!(strip_custom_prefix("hy4-preview"), "hy4-preview");
        assert_eq!(strip_custom_prefix("custom-local:"), "custom-local:");
    }
}
