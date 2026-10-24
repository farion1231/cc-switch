//! Import Kimi Code's committed request usage. Tokens are already normalized to fresh input.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::kimi_session::{record_belongs_to, timestamp_ms, RequestIndex, MAIN_AGENT};
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::{
    session_usage::{
        load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

pub fn sync_kimi_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_sessions(db, &crate::kimi_config::data_dir().join("sessions"))
}

fn sync_sessions(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    let Ok(workspaces) = fs::read_dir(root) else {
        return Ok(result);
    };
    for workspace in workspaces.flatten() {
        let Ok(sessions) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            // Kimi also keeps a standalone directory for some child agents. Those requests
            // are already counted from their parent's agents/agent-N journals, and the
            // session browser hides such directories for the same reason.
            if is_child_session(&session.path()) {
                continue;
            }
            // One wire journal per agent: agents/main for the main agent, one
            // agents/agent-N per sub-agent instance. Both belong to the same session.
            let Ok(agents) = fs::read_dir(session.path().join("agents")) else {
                continue;
            };
            let name = session.file_name();
            let session_name = name.to_string_lossy();
            for agent in agents.flatten() {
                let path = agent.path().join("wire.jsonl");
                if !path.is_file() {
                    continue;
                }
                let agent_name = agent.file_name();
                let agent_id = agent_name.to_string_lossy();
                let modified = fs::metadata(&path)
                    .map(|meta| metadata_modified_nanos(&meta))
                    .unwrap_or(0);
                let key = format!("kimi:{}", path.to_string_lossy());
                if cursors.get(&key).is_some_and(|cursor| {
                    cursor.last_modified == modified
                        && cursor.last_line_offset
                            == fs::metadata(&path)
                                .map(|meta| meta.len() as i64)
                                .unwrap_or(-1)
                }) {
                    continue;
                }
                match sync_file(db, &path, &session_name, &key, modified, &agent_id) {
                    Ok(part) => result.merge(part),
                    Err(error) => result
                        .errors
                        .push(format!("Kimi Code session {}: {error}", path.display())),
                }
            }
        }
    }
    Ok(result)
}

/// A missing or unreadable `state.json` keeps the session eligible: losing historical usage
/// because metadata is absent would be worse than scanning a directory twice.
fn is_child_session(dir: &Path) -> bool {
    [dir.join("state.json"), dir.join("session-meta/state.json")]
        .into_iter()
        .find_map(|path| {
            fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        })
        .and_then(|state| {
            state
                .pointer("/custom/session_kind")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|kind| kind == "child")
}

fn sync_file(
    db: &Database,
    path: &Path,
    session: &str,
    key: &str,
    modified: i64,
    agent: &str,
) -> Result<SessionSyncResult, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = 0i64;
    let mut requests = RequestIndex::for_agent(agent);
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
        if size == 0 {
            break;
        }
        // A live writer may have flushed only part of its last record. Retry it next round.
        if buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        let Ok(record) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        // A parent journal can mirror a sub-agent's committed message, and vice versa. Only
        // the journal that owns the record may import it, or the same request would be
        // counted twice under two different request ids.
        if !record_belongs_to(&record, agent) {
            continue;
        }
        requests.observe(&record);
        if !matches!(
            record["type"].as_str(),
            Some("agent.message.appended" | "human.agent.message.appended")
        ) || record
            .pointer("/message/message/role")
            .and_then(Value::as_str)
            != Some("assistant")
        {
            continue;
        }
        let meta = &record["message"]["meta"];
        let Some(usage) = meta.get("usage") else {
            continue;
        };
        let Some(created) = timestamp_ms(&record["time"]) else {
            continue;
        };
        let tokens = |field| usage[field].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
        let usage = TokenUsage {
            input_tokens: tokens("inputOther"),
            output_tokens: tokens("output"),
            cache_read_tokens: tokens("inputCacheRead"),
            cache_creation_tokens: tokens("inputCacheCreation"),
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
        let identity = meta
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{:x}", Sha256::digest(record.to_string().as_bytes())));
        let provider = meta
            .pointer("/model/provider")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let request = request_id(agent, provider, &identity);
        // Retain only metadata and usage. step.end may appear after the human projection
        // (and after tools); resolve its messageId before taking the database lock.
        pending.push((request, meta.clone(), usage, created));
    }
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for (request, meta, mut usage, created) in pending {
        let model = requests.model(&meta).unwrap_or("unknown");
        usage.model = Some(model.into());
        let info = requests.get(&meta);
        let latency = info.map_or(0, |info| info.latency_ms);
        let first_token = info.and_then(|info| info.first_token_ms);
        // Permanent receipts prevent re-import after wire rewrites, forks, or log pruning.
        if tx.execute("INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES ('kimi_session', ?1, ?1, 1)", [&request])? == 0 {
            // As with Claude imports, enrichment counts as changed data so the normal
            // usage-log-recorded notification refreshes open tables and detail panels.
            if refresh_existing(&tx, &request, model, &usage, latency, first_token)? {
                result.imported += 1;
            } else {
                result.skipped += 1;
            }
            continue;
        }
        let amounts = costs(&tx, model, &usage, Decimal::ONE);
        tx.execute("INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
        ) VALUES (?1, '_kimi_session', 'kimi', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?15, ?16, 200, ?12, 'kimi_session', 1, '1', ?13, 'kimi_session', ?14)",
            rusqlite::params![request, model, usage.input_tokens, usage.output_tokens, usage.cache_read_tokens, usage.cache_creation_tokens,
                amounts[0], amounts[1], amounts[2], amounts[3], amounts[4], session, created/1000, INPUT_TOKEN_SEMANTICS_FRESH, latency, first_token])?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

/// The main agent's ids are frozen: they are what the permanent receipts in
/// `session_usage_dedup` already hold, so changing them would re-import every request
/// imported before sub-agents were scanned. Sub-agents are prefixed with their agent id
/// so two agents cannot collide when identity falls back to a content hash.
fn request_id(agent: &str, provider: &str, identity: &str) -> String {
    if agent == MAIN_AGENT {
        format!("kimi:{provider}:{identity}")
    } else {
        format!("kimi:{agent}:{provider}:{identity}")
    }
}

fn costs(
    conn: &rusqlite::Connection,
    model: &str,
    usage: &TokenUsage,
    multiplier: Decimal,
) -> [String; 5] {
    find_model_pricing(conn, model)
        .map(|pricing| {
            let cost = CostCalculator::calculate_for_app("kimi", usage, &pricing, multiplier);
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

/// Enrich retained details only. A receipt without a detail row belongs to pruned history;
/// recreating it would count the request again on top of its daily rollup.
fn refresh_existing(
    conn: &rusqlite::Connection,
    request: &str,
    model: &str,
    usage: &TokenUsage,
    latency: i64,
    first_token: Option<i64>,
) -> Result<bool, AppError> {
    let stored: Option<(String, String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT model, cost_multiplier, latency_ms, first_token_ms FROM proxy_request_logs
         WHERE request_id = ?1 AND app_type = 'kimi' AND data_source = 'kimi_session'",
            [request],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((old_model, multiplier, old_latency, old_first)) = stored else {
        return Ok(false);
    };
    let model_changed = model != old_model && model != "unknown";
    // Preserve native timing when a copied history contains only timestamp estimates.
    let latency = if latency > 0 && (old_first.is_none() || first_token.is_some()) {
        latency
    } else {
        old_latency
    };
    let first_token = first_token.or(old_first).filter(|first| *first <= latency);
    if !model_changed && latency == old_latency && first_token == old_first {
        return Ok(false);
    }
    if model_changed {
        let amounts = costs(
            conn,
            model,
            usage,
            multiplier.parse().unwrap_or(Decimal::ONE),
        );
        conn.execute(
            "UPDATE proxy_request_logs SET model = ?2, request_model = ?2,
             input_cost_usd = ?3, output_cost_usd = ?4, cache_read_cost_usd = ?5,
             cache_creation_cost_usd = ?6, total_cost_usd = ?7
             WHERE request_id = ?1 AND app_type = 'kimi' AND data_source = 'kimi_session'",
            rusqlite::params![
                request, model, amounts[0], amounts[1], amounts[2], amounts[3], amounts[4]
            ],
        )?;
    }
    conn.execute(
        "UPDATE proxy_request_logs SET latency_ms = ?2, first_token_ms = ?3
         WHERE request_id = ?1 AND app_type = 'kimi' AND data_source = 'kimi_session'",
        rusqlite::params![request, latency, first_token],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, time: i64, output: u64) -> Value {
        json!({"type":"agent.message.appended","time":time,"message":{
            "message":{"role":"assistant","content":[{"type":"text","text":"Done"}]},
            "meta":{"messageId":id,"model":{"provider":"agent-loop","model":"agent-loop"},
                "usage":{"inputOther":10,"output":output,"inputCacheRead":30,"inputCacheCreation":40}}
        }})
    }

    fn request(
        agent: &str,
        id: &str,
        model: &str,
        start: i64,
        first: i64,
        stream: i64,
        output: u64,
    ) -> Vec<Value> {
        vec![
            json!({"type":"context.append_loop_event","time":start-10,"event":{"type":"step.begin","uuid":id,"turnId":"0","step":1}}),
            json!({"type":"llm.request","time":start,"agentId":agent,"kind":"loop","turnStep":"0.1","provider":"openai","model":model}),
            json!({"type":"context.append_loop_event","time":start+first+stream,"event":{"type":"content.part","stepUuid":id,"part":{"type":"text","text":"Done"}}}),
            // The human projection can precede step.end while tools are running.
            message(id, start + first + stream, output),
            json!({"type":"context.append_loop_event","time":start+first+stream+100_000,"event":{"type":"tool.call","stepUuid":id}}),
            json!({"type":"context.append_loop_event","time":start+first+stream+200_000,"event":{"type":"step.end","uuid":id,"messageId":id,"llmFirstTokenLatencyMs":first,"llmStreamDurationMs":stream}}),
        ]
    }

    fn write_records(path: &Path, records: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }

    /// `<root>/<workspace>/<session>/agents/<agent>/wire.jsonl`, the layout Kimi writes.
    fn agent_wire(root: &Path, agent: &str) -> std::path::PathBuf {
        root.join("ws")
            .join("session-1")
            .join("agents")
            .join(agent)
            .join("wire.jsonl")
    }

    /// `(request_id, session_id, model, output_tokens)`, oldest first.
    fn imported_rows(db: &Database) -> Vec<(String, String, String, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, session_id, model, output_tokens FROM proxy_request_logs
                 WHERE app_type = 'kimi' ORDER BY created_at",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    #[test]
    fn kimi_usage_resolves_actual_models_and_timing_for_detail_and_aggregate_stats() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let mut records = request(
            MAIN_AGENT,
            "one",
            "model-one",
            1_700_000_000_000,
            3_500,
            6_500,
            900,
        );
        records.extend(request(
            MAIN_AGENT,
            "two",
            "model-two",
            1_700_000_300_000,
            1_500,
            18_500,
            600,
        ));
        write_records(&path, &records);
        assert_eq!(
            sync_file(&db, &path, "session", "file", 1, MAIN_AGENT)
                .unwrap()
                .imported,
            2
        );
        {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT model,request_model,latency_ms,first_token_ms FROM proxy_request_logs ORDER BY created_at").unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                rows,
                vec![
                    ("model-one".into(), "model-one".into(), 10_000, Some(3_500)),
                    ("model-two".into(), "model-two".into(), 20_000, Some(1_500))
                ]
            );
        }
        let stats = db
            .get_provider_stats(None, None, Some("kimi"), None, None)
            .unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].request_count, 2);
        assert_eq!(stats[0].est_speed_output_tokens, 1_500);
        assert_eq!(stats[0].est_speed_duration_ms, 30_000);
        assert_eq!(stats[0].est_first_token_median_ms, Some(2_500));
        assert_eq!(
            (
                stats[0].speed_output_tokens,
                stats[0].speed_generation_ms,
                stats[0].first_token_median_ms
            ),
            (0, 0, None)
        );
        let models = db
            .get_model_stats(None, None, Some("kimi"), None, None)
            .unwrap();
        assert_eq!(models.len(), 2);
        assert!(models
            .iter()
            .all(|model| model.model != "agent-loop" && model.est_speed_output_tokens > 0));
    }

    #[test]
    fn kimi_usage_enriches_details_when_step_end_arrives_on_next_sync() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let records = request(
            MAIN_AGENT,
            "live",
            "real-model",
            1_700_000_000_000,
            2_000,
            8_000,
            500,
        );
        write_records(&path, &records[..4]);
        assert_eq!(
            sync_file(&db, &path, "session", "file", 1, MAIN_AGENT)
                .unwrap()
                .imported,
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            let row: (String, i64) = conn
                .query_row(
                    "SELECT model,latency_ms FROM proxy_request_logs",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(row, ("unknown".into(), 0));
        }
        write_records(&path, &records);
        assert_eq!(
            sync_file(&db, &path, "session", "file", 2, MAIN_AGENT)
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let row: (String, i64, Option<i64>) = conn
            .query_row(
                "SELECT model,latency_ms,first_token_ms FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("real-model".into(), 10_000, Some(2_000)));
        drop(conn);
        // Identical scans and truncated copies must neither trigger refreshes nor erase
        // the native metadata that has already been resolved for this message.
        assert_eq!(
            sync_file(&db, &path, "session", "file", 3, MAIN_AGENT)
                .unwrap()
                .imported,
            0
        );
        write_records(&path, &records[..4]);
        assert_eq!(
            sync_file(&db, &path, "fork", "fork", 4, MAIN_AGENT)
                .unwrap()
                .imported,
            0
        );
        let conn = db.conn.lock().unwrap();
        let preserved: (String, i64, Option<i64>) = conn
            .query_row(
                "SELECT model,latency_ms,first_token_ms FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(preserved, row);
    }

    #[test]
    fn kimi_usage_normalizes_tokens_and_does_not_reimport_pruned_or_forked_history() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let row = json!({"type":"agent.message.appended","time":1700000000000i64,"message":{"message":{"role":"assistant","content":[{"type":"text","text":"Done"}]},"meta":{"messageId":"request-1","model":{"provider":"custom","model":"vendor/model"},"usage":{"inputOther":10,"output":20,"inputCacheRead":30,"inputCacheCreation":40}}}});
        fs::write(&path, format!("{row}\n")).unwrap();
        assert_eq!(
            sync_file(&db, &path, "s1", "first", 1, MAIN_AGENT)
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            sync_file(&db, &path, "fork", "forked", 2, MAIN_AGENT)
                .unwrap()
                .imported,
            0
        );
        let conn = db.conn.lock().unwrap();
        let values:(String,i64,i64,i64,i64)=conn.query_row("SELECT model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens FROM proxy_request_logs",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
        assert_eq!(values, ("vendor/model".into(), 10, 20, 30, 40));
        conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        drop(conn);
        assert_eq!(
            sync_file(&db, &path, "s1", "rewritten", 3, MAIN_AGENT)
                .unwrap()
                .imported,
            0
        );
    }

    #[test]
    fn kimi_usage_retries_incomplete_tail() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        fs::write(&path, "{\"type\":").unwrap();
        assert_eq!(
            sync_file(&db, &path, "s", "partial", 1, MAIN_AGENT)
                .unwrap()
                .imported,
            0
        );
        let cursor: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT last_line_offset FROM session_log_sync WHERE file_path='partial'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursor, 0);
    }

    #[test]
    fn subagent_usage_is_imported_from_each_agent_wire() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_records(
            &agent_wire(dir.path(), MAIN_AGENT),
            &request(
                MAIN_AGENT,
                "main",
                "main-model",
                1_700_000_000_000,
                1_000,
                1_000,
                500,
            ),
        );
        write_records(
            &agent_wire(dir.path(), "agent-1"),
            &request(
                "agent-1",
                "sub",
                "sub-model",
                1_700_000_100_000,
                2_000,
                3_000,
                700,
            ),
        );
        // `agents/` also holds task state and artefacts; only journals are agents.
        let agent_dir = dir.path().join("ws/session-1/agents");
        fs::create_dir_all(agent_dir.join("agent-1/tasks")).unwrap();
        fs::write(agent_dir.join("agent-1/tasks/task.json"), "{}").unwrap();
        fs::create_dir_all(agent_dir.join("shared")).unwrap();
        fs::write(agent_dir.join("notes.txt"), "x").unwrap();

        let result = sync_sessions(&db, dir.path()).unwrap();
        assert_eq!((result.imported, result.files_scanned), (2, 2));
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let rows = imported_rows(&db);
        assert_eq!(
            rows.iter().map(|row| row.2.as_str()).collect::<Vec<_>>(),
            ["main-model", "sub-model"]
        );
        // A sub-agent's requests belong to the session that owns its journal.
        assert!(rows.iter().all(|row| row.1 == "session-1"));
    }

    #[test]
    fn mirrored_records_from_other_agents_are_not_imported() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut journal = request(
            MAIN_AGENT,
            "own",
            "own-model",
            1_700_000_000_000,
            1_000,
            1_000,
            100,
        );
        // The parent journal can mirror a sub-agent's committed message; that usage is
        // imported from the sub-agent's own journal only.
        let mut mirrored = message("mirrored", 1_700_000_300_000, 900);
        mirrored["agentId"] = json!("agent-1");
        journal.push(mirrored);
        // Newer writers also label the committed message itself.
        let mut nested = message("nested", 1_700_000_400_000, 800);
        nested["message"]["meta"]["agentId"] = json!("agent-2");
        journal.push(nested);
        write_records(&agent_wire(dir.path(), MAIN_AGENT), &journal);
        write_records(
            &agent_wire(dir.path(), "agent-1"),
            &request(
                "agent-1",
                "sub",
                "sub-model",
                1_700_000_300_000,
                1_000,
                1_000,
                900,
            ),
        );

        assert_eq!(sync_sessions(&db, dir.path()).unwrap().imported, 2);
        let rows = imported_rows(&db);
        assert_eq!(
            rows.iter()
                .map(|row| (row.0.as_str(), row.3))
                .collect::<Vec<_>>(),
            [
                ("kimi:agent-loop:own", 100),
                ("kimi:agent-1:agent-loop:sub", 900)
            ]
        );
        let total: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT SUM(output_tokens) FROM proxy_request_logs
                 WHERE app_type = 'kimi' AND data_source = 'kimi_session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // Neither mirrored aggregate (900 + 800) is counted on top of the sub-agent journal.
        assert_eq!(total, 1_000);
    }

    #[test]
    fn child_session_directories_are_skipped() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("ws/parent");
        write_records(
            &parent.join("agents/main/wire.jsonl"),
            &request(
                MAIN_AGENT,
                "parent",
                "main-model",
                1_700_000_000_000,
                1_000,
                1_000,
                100,
            ),
        );
        write_records(
            &parent.join("agents/agent-1/wire.jsonl"),
            &request(
                "agent-1",
                "child",
                "sub-model",
                1_700_000_000_000,
                1_000,
                1_000,
                200,
            ),
        );
        // A child session directory repeats the same run; importing it would double count.
        let child = dir.path().join("ws/child");
        fs::create_dir_all(&child).unwrap();
        fs::write(
            child.join("state.json"),
            json!({"custom":{"session_kind":"child"}}).to_string(),
        )
        .unwrap();
        write_records(
            &child.join("agents/main/wire.jsonl"),
            &request(
                MAIN_AGENT,
                "child",
                "sub-model",
                1_700_000_000_000,
                1_000,
                1_000,
                200,
            ),
        );
        // A session without state.json (or with unreadable metadata) is still imported.
        write_records(
            &dir.path().join("ws/legacy/agents/main/wire.jsonl"),
            &request(
                MAIN_AGENT,
                "legacy",
                "legacy-model",
                1_700_000_500_000,
                1_000,
                1_000,
                300,
            ),
        );

        let result = sync_sessions(&db, dir.path()).unwrap();
        assert_eq!(result.imported, 3);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let sessions: Vec<String> = imported_rows(&db).into_iter().map(|row| row.1).collect();
        assert!(!sessions.contains(&"child".to_string()));
        assert_eq!(sessions, ["parent", "parent", "legacy"]);
    }

    #[test]
    fn session_total_sums_main_and_subagent_usage() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let agents = [
            (MAIN_AGENT, "main", 1_700_000_000_000, 100),
            ("agent-1", "sub-one", 1_700_000_100_000, 200),
            ("agent-2", "sub-two", 1_700_000_200_000, 300),
        ];
        for (agent, id, start, output) in agents {
            write_records(
                &agent_wire(dir.path(), agent),
                &request(
                    agent,
                    id,
                    &format!("{id}-model"),
                    start,
                    1_000,
                    9_000,
                    output,
                ),
            );
        }
        assert_eq!(sync_sessions(&db, dir.path()).unwrap().imported, 3);

        // The session's totals are the main agent plus both sub-agents.
        let totals: (i64, i64) = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT SUM(input_tokens), SUM(output_tokens) FROM proxy_request_logs
                 WHERE session_id = 'session-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(totals, (30, 600));
        // ...and the model distribution shows each agent's real model.
        let models = db
            .get_model_stats(None, None, Some("kimi"), None, None)
            .unwrap();
        let mut names = models
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, ["main-model", "sub-one-model", "sub-two-model"]);
    }

    #[test]
    fn subagent_usage_resolves_model_and_timing() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_records(
            &agent_wire(dir.path(), "agent-2"),
            &request(
                "agent-2",
                "sub",
                "sub-model",
                1_700_000_000_000,
                2_500,
                7_500,
                400,
            ),
        );
        assert_eq!(sync_sessions(&db, dir.path()).unwrap().imported, 1);
        let row: (String, i64, Option<i64>) = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT model, latency_ms, first_token_ms FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        // A sub-agent is timed on the same basis as the main agent.
        assert_eq!(row, ("sub-model".into(), 10_000, Some(2_500)));
    }

    #[test]
    fn subagent_usage_is_not_double_counted_from_parent_aggregate() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut parent = request(
            MAIN_AGENT,
            "parent",
            "main-model",
            1_700_000_000_000,
            1_000,
            1_000,
            100,
        );
        // The parent mirrors each sub-agent run with the aggregate it already counted.
        parent.push(
            json!({"type":"subagent.completed","time":1_700_000_200_000i64,
            "subagentId":"agent-1","resultSummary":"Done","contextTokens":10,
            "usage":{"inputOther":1000,"output":2000,"inputCacheRead":0,"inputCacheCreation":0}}),
        );
        write_records(&agent_wire(dir.path(), MAIN_AGENT), &parent);
        write_records(
            &agent_wire(dir.path(), "agent-1"),
            &request(
                "agent-1",
                "sub",
                "sub-model",
                1_700_000_100_000,
                1_000,
                1_000,
                400,
            ),
        );

        assert_eq!(sync_sessions(&db, dir.path()).unwrap().imported, 2);
        let rows = imported_rows(&db);
        // The 2 000-token aggregate stays out; the child journal is the only source.
        assert_eq!(
            rows.iter()
                .map(|row| (row.2.as_str(), row.3))
                .collect::<Vec<_>>(),
            [("main-model", 100), ("sub-model", 400)]
        );
    }

    #[test]
    fn resync_does_not_duplicate_subagent_usage() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = agent_wire(dir.path(), "agent-1");
        let records = request(
            "agent-1",
            "sub",
            "sub-model",
            1_700_000_000_000,
            1_000,
            1_000,
            400,
        );
        write_records(&path, &records);
        assert_eq!(
            sync_file(&db, &path, "session-1", "kimi:agent-1", 1, "agent-1")
                .unwrap()
                .imported,
            1
        );
        // Same content with a newer mtime: the permanent receipt still holds.
        assert_eq!(
            sync_file(&db, &path, "session-1", "kimi:agent-1", 2, "agent-1")
                .unwrap()
                .imported,
            0
        );
        // A fork can rewrite the journal without the original request events.
        write_records(&path, &records[..4]);
        assert_eq!(
            sync_file(&db, &path, "fork", "forked", 3, "agent-1")
                .unwrap()
                .imported,
            0
        );
        assert_eq!(imported_rows(&db).len(), 1);
    }

    #[test]
    fn main_agent_request_ids_are_unchanged() {
        let shared = |provider: &str| json!({"type":"agent.message.appended","time":1700000000000i64,"message":{"message":{"role":"assistant","content":[{"type":"text","text":"Done"}]},"meta":{"messageId":"shared","model":{"provider":provider,"model":"vendor/model"},"usage":{"inputOther":10,"output":20,"inputCacheRead":0,"inputCacheCreation":0}}}});
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_records(&agent_wire(dir.path(), MAIN_AGENT), &[shared("custom")]);
        write_records(&agent_wire(dir.path(), "agent-1"), &[shared("custom")]);
        // A receipt written before sub-agents were scanned must still match, so upgrading
        // does not import the main agent's history a second time.
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id)
                 VALUES ('kimi_session', 'kimi:custom:shared', 'kimi:custom:shared', 1)",
                [],
            )
            .unwrap();
        let result = sync_sessions(&db, dir.path()).unwrap();
        assert_eq!((result.imported, result.skipped), (1, 1));
        // Only the sub-agent is new, and its id is isolated from the main agent's.
        assert_eq!(
            imported_rows(&db),
            vec![(
                "kimi:agent-1:custom:shared".to_string(),
                "session-1".to_string(),
                "vendor/model".to_string(),
                20
            )]
        );

        // The frozen main-agent format itself, byte for byte.
        let fresh = Database::memory().unwrap();
        let other = tempfile::tempdir().unwrap();
        write_records(&agent_wire(other.path(), MAIN_AGENT), &[shared("custom")]);
        assert_eq!(sync_sessions(&fresh, other.path()).unwrap().imported, 1);
        assert_eq!(imported_rows(&fresh)[0].0, "kimi:custom:shared");
    }

    #[test]
    fn subagent_usage_retries_incomplete_tail() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_records(
            &agent_wire(dir.path(), MAIN_AGENT),
            &request(
                MAIN_AGENT,
                "main",
                "main-model",
                1_700_000_000_000,
                1_000,
                1_000,
                500,
            ),
        );
        let unit = request(
            "agent-1",
            "sub",
            "sub-model",
            1_700_000_100_000,
            1_000,
            1_000,
            400,
        );
        let sub = agent_wire(dir.path(), "agent-1");
        let committed: usize = unit[..4]
            .iter()
            .map(|record| record.to_string().len() + 1)
            .sum();
        let mut text = unit[..4]
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        // A live writer flushed only part of its last record.
        text.push_str("{\"type\":\"agent.message.app");
        fs::create_dir_all(sub.parent().unwrap()).unwrap();
        fs::write(&sub, text).unwrap();

        assert_eq!(sync_sessions(&db, dir.path()).unwrap().imported, 2);
        let cursor: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT last_line_offset FROM session_log_sync WHERE file_path LIKE '%agent-1%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // The truncated record is retried next round, so the cursor stops before it.
        assert_eq!(cursor, committed as i64);
    }

    #[test]
    fn broken_subagent_file_does_not_abort_session() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_records(
            &agent_wire(dir.path(), MAIN_AGENT),
            &request(
                MAIN_AGENT,
                "main",
                "main-model",
                1_700_000_000_000,
                1_000,
                1_000,
                500,
            ),
        );
        write_records(
            &agent_wire(dir.path(), "agent-1"),
            &request(
                "agent-1",
                "sub",
                "sub-model",
                1_700_000_100_000,
                1_000,
                1_000,
                400,
            ),
        );
        // Unparsable records are skipped one by one, as for the main agent.
        let broken = agent_wire(dir.path(), "agent-2");
        fs::create_dir_all(broken.parent().unwrap()).unwrap();
        fs::write(&broken, "not json\n{\"type\":\"agent.message.appended\"}\n").unwrap();

        let result = sync_sessions(&db, dir.path()).unwrap();
        assert_eq!(result.imported, 2);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }
}
