//! Import Cline CLI per-request usage from `<id>.messages.json`.
//!
//! Each assistant message carries `metrics`; `inputTokens` includes cache reads
//! and writes, so rows are stored as fresh input. `metadata_json.usage` in
//! `sessions.db` is the sum of these and is never imported on top.
use crate::cline_session::{self, timestamp_ms, ClineSession};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::{
    session_usage::{
        estimated_latency_ms, load_sync_cursors, metadata_modified_nanos,
        update_sync_state_on_conn, SessionSyncResult,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use rust_decimal::Decimal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn sync_cline_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_sessions(db, &cline_session::sessions())
}

fn sync_sessions(db: &Database, sessions: &[ClineSession]) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    // Subagent sessions are hidden from the list but their requests are billed too.
    for session in sessions {
        let path = &session.messages_path;
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = metadata_modified_nanos(&meta);
        let len = meta.len() as i64;
        let key = format!("cline:v1:{}", path.to_string_lossy());
        if cursors.get(&key).is_some_and(|cursor| {
            cursor.last_modified == modified && cursor.last_line_offset == len
        }) {
            continue;
        }
        match sync_file(db, path, &session.id, &key, modified, len) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("Cline session {}: {error}", path.display())),
        }
    }
    Ok(result)
}

struct Pending {
    request: String,
    model: String,
    usage: TokenUsage,
    native_cost: Option<Decimal>,
    created_ms: i64,
    latency_ms: i64,
}

fn tokens(metrics: &Value, key: &str) -> u32 {
    metrics[key].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
}

fn parse(document: &Value, session: &str) -> Vec<Pending> {
    let mut pending = Vec::new();
    let mut previous_ts = None;
    for message in document["messages"].as_array().into_iter().flatten() {
        let ts = message.get("ts").and_then(timestamp_ms);
        let request_start = previous_ts;
        previous_ts = ts.or(previous_ts);
        if message["role"].as_str() != Some("assistant") {
            continue;
        }
        let Some(metrics) = message.get("metrics").filter(|m| m.is_object()) else {
            continue;
        };
        let Some(created_ms) = ts else {
            continue;
        };
        let input = tokens(metrics, "inputTokens");
        let cache_read = tokens(metrics, "cacheReadTokens");
        let cache_write = tokens(metrics, "cacheWriteTokens");
        let usage = TokenUsage {
            // Cline reports total input (fresh + cache read + cache write).
            input_tokens: input.saturating_sub(cache_read).saturating_sub(cache_write),
            output_tokens: tokens(metrics, "outputTokens"),
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_write,
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
        let identity = message["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{:x}", Sha256::digest(message.to_string().as_bytes())));
        let model = message
            .pointer("/modelInfo/id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or("unknown")
            .to_owned();
        // `cost` is absent when Cline has no price for the model.
        let native_cost = metrics["cost"]
            .as_f64()
            .filter(|cost| cost.is_finite() && *cost > 0.0)
            .and_then(Decimal::from_f64_retain)
            .map(|cost| cost.round_dp(12).normalize());
        pending.push(Pending {
            request: format!("cline:{session}:{identity}"),
            model,
            usage,
            native_cost,
            created_ms,
            latency_ms: request_start
                .and_then(|start| estimated_latency_ms(start, created_ms))
                .unwrap_or(0),
        });
    }
    pending
}

fn sync_file(
    db: &Database,
    path: &Path,
    session: &str,
    key: &str,
    modified: i64,
    len: i64,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let bytes = fs::read(path).map_err(|error| AppError::io(path, error))?;
    // Cline rewrites the whole file; a half-written copy is retried on the next sync.
    let Ok(document) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(result);
    };
    let pending = parse(&document, session);
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for entry in pending {
        // Permanent receipts prevent re-import after rollups prune the detail rows.
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('cline_session', ?1, ?1, 1)",
            [&entry.request],
        )? == 0
        {
            result.skipped += 1;
            continue;
        }
        let amounts = match entry.native_cost {
            Some(total) => [
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
                total,
            ]
            .map(|value| value.to_string()),
            None => costs(&tx, &entry.model, &entry.usage),
        };
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, '_cline_session', 'cline', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?15, NULL, 200, ?12, 'cline_session', 1, '1', ?13, 'cline_session', ?14)",
            rusqlite::params![
                entry.request,
                entry.model,
                entry.usage.input_tokens,
                entry.usage.output_tokens,
                entry.usage.cache_read_tokens,
                entry.usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                session,
                entry.created_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
                entry.latency_ms,
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, len)?;
    tx.commit()?;
    Ok(result)
}

fn costs(conn: &rusqlite::Connection, model: &str, usage: &TokenUsage) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app("cline", usage, &pricing, Decimal::ONE);
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
    use crate::cline_session::tests::write_fixture;
    use serde_json::json;
    use serial_test::serial;

    fn conversation() -> Value {
        json!([
            {"id":"u1","role":"user","content":"<user_input mode=\"act\">hi</user_input>","ts":1700000050000i64},
            {"id":"msg_priced","role":"assistant","content":[{"type":"text","text":"ok"}],"ts":1700000052000i64,
             "modelInfo":{"id":"claude-sonnet-4-5","provider":"anthropic"},
             "metrics":{"inputTokens":130,"outputTokens":7,"cacheReadTokens":20,"cacheWriteTokens":10,"cost":0.0004485}},
            {"id":"r1","role":"user","content":[{"type":"tool_result","tool_use_id":"c","content":"x"}],"ts":1700000053000i64},
            {"id":"msg_unpriced","role":"assistant","content":[{"type":"text","text":"done"}],"ts":1700000056000i64,
             "modelInfo":{"id":"fixture-model","provider":"custom"},
             "metrics":{"inputTokens":5000,"outputTokens":100,"cacheReadTokens":4000,"cacheWriteTokens":0}},
            {"id":"msg_nomodel","role":"assistant","content":[],"ts":1700000057000i64,
             "metrics":{"inputTokens":3,"outputTokens":1,"cacheReadTokens":0,"cacheWriteTokens":0}},
            {"id":"msg_empty","role":"assistant","content":[],"ts":1700000058000i64,
             "metrics":{"inputTokens":0,"outputTokens":0,"cacheReadTokens":0,"cacheWriteTokens":0}}
        ])
    }

    type Row = (String, String, i64, i64, i64, i64, String, String, i64, i64);

    fn rows(db: &Database) -> Vec<Row> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        total_cost_usd, session_id, latency_ms, input_token_semantics
                 FROM proxy_request_logs WHERE app_type = 'cline' ORDER BY created_at, request_id",
            )
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
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    #[test]
    fn cline_usage_imports_fresh_input_native_cost_and_price_table_fallback() {
        let db = Database::memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT OR REPLACE INTO model_pricing (model_id,display_name,input_cost_per_million,output_cost_per_million,cache_read_cost_per_million,cache_creation_cost_per_million) VALUES ('fixture-model','Fixture','1','2','0.1','0.5')", []).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db/sessions.db");
        let sessions_dir = dir.path().join("sessions");
        write_fixture(
            &db_path,
            &sessions_dir,
            &[
                ("s_cli", "cli", false, conversation()),
                (
                    "s_sub",
                    "cli",
                    true,
                    json!([conversation()[0], conversation()[1]]),
                ),
                ("s_vscode", "vscode", false, conversation()),
            ],
        );
        let sessions = cline_session::read_index(&db_path, &sessions_dir);
        let result = sync_sessions(&db, &sessions).unwrap();
        assert!(result.errors.is_empty());
        assert_eq!((result.imported, result.files_scanned), (4, 2));

        let rows = rows(&db);
        let find = |request: &str| rows.iter().find(|row| row.0 == request).unwrap().clone();
        let priced = find("cline:s_cli:msg_priced");
        // 130 total input - 20 cache read - 10 cache write = 100 fresh input.
        assert_eq!(
            (priced.1.as_str(), priced.2, priced.3, priced.4, priced.5),
            ("claude-sonnet-4-5", 100, 7, 20, 10)
        );
        assert_eq!(priced.6.parse::<Decimal>().unwrap(), Decimal::new(4485, 7));
        assert_eq!((priced.7.as_str(), priced.8, priced.9), ("s_cli", 2_000, 2));
        let unpriced = find("cline:s_cli:msg_unpriced");
        assert_eq!((unpriced.2, unpriced.4, unpriced.8), (1_000, 4_000, 3_000));
        // 1000 * 1 + 100 * 2 + 4000 * 0.1 = 1600 per million.
        assert_eq!(unpriced.6.parse::<Decimal>().unwrap(), Decimal::new(16, 4));
        assert_eq!(find("cline:s_cli:msg_nomodel").1, "unknown");
        // Subagent usage is imported under its own session id.
        assert_eq!(find("cline:s_sub:msg_priced").7, "s_sub");
        assert!(!rows.iter().any(|row| row.0.contains("msg_empty")));

        // Unchanged files are not rescanned; rewritten ones never double count.
        assert_eq!(sync_sessions(&db, &sessions).unwrap().files_scanned, 0);
        let path = sessions_dir.join("s_cli/s_cli.messages.json");
        fs::write(
            &path,
            json!({"version":1,"messages":conversation()}).to_string() + "\n",
        )
        .unwrap();
        let again = sync_sessions(&db, &sessions).unwrap();
        assert_eq!((again.imported, again.skipped), (0, 3));
        assert_eq!(db.rollup_and_prune(30).unwrap(), 4);
        db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM session_log_sync", [])
            .unwrap();
        assert_eq!(sync_sessions(&db, &sessions).unwrap().imported, 0);
    }

    #[test]
    fn cline_usage_retries_half_written_file() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.messages.json");
        fs::write(&path, "{\"messages\":[").unwrap();
        let result = sync_file(&db, &path, "s", "partial", 1, 13).unwrap();
        assert_eq!(result.imported, 0);
        let cursors: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(cursors, 0);
    }

    struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    #[serial]
    fn cline_usage_and_sessions_follow_cline_env_dirs() {
        let keys = [
            "CC_SWITCH_TEST_HOME",
            "CLINE_DIR",
            "CLINE_DATA_DIR",
            "CLINE_DB_DATA_DIR",
            "CLINE_SESSION_DATA_DIR",
        ];
        let _restore = RestoreEnv(
            keys.iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        );
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path().join("home"));
        std::env::set_var("CLINE_DIR", dir.path().join("cline"));
        std::env::set_var("CLINE_DATA_DIR", dir.path().join("data"));
        std::env::remove_var("CLINE_DB_DATA_DIR");
        std::env::remove_var("CLINE_SESSION_DATA_DIR");
        let data = dir.path().join("data");
        write_fixture(
            &data.join("db/sessions.db"),
            &data.join("sessions"),
            &[("s_env", "cli", false, conversation())],
        );

        let listed = crate::session_manager::providers::cline::scan_sessions();
        assert_eq!(listed.len(), 1);
        let resume = listed[0].resume_command.as_deref().unwrap();
        // The resume command carries the data dir in the platform's own syntax.
        #[cfg(windows)]
        assert!(resume.contains("$env:CLINE_DATA_DIR = "), "{resume}");
        #[cfg(not(windows))]
        assert!(resume.contains("CLINE_DATA_DIR="), "{resume}");
        assert!(resume.ends_with("cline --id s_env"), "{resume}");
        let messages = crate::session_manager::load_transcript(
            "cline",
            listed[0].source_path.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(messages.transcript.messages.len(), 4);
        assert!(crate::session_manager::delete_session(
            "cline",
            "s_env",
            listed[0].source_path.as_deref().unwrap()
        )
        .unwrap_err()
        .contains("cline history delete"));

        let db = Database::memory().unwrap();
        assert_eq!(sync_cline_usage(&db).unwrap().imported, 3);
    }
}
