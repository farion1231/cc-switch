//! Import Kimi Code's committed request usage. Tokens are already normalized to fresh input.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::kimi_session::{timestamp_ms, RequestIndex};
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
            // The main agent and every subagent (`agents/agent-N`) keep separate wires.
            let Ok(agents) = fs::read_dir(session.path().join("agents")) else {
                continue;
            };
            for agent in agents.flatten() {
                let path = agent.path().join("wire.jsonl");
                if !path.is_file() {
                    continue;
                }
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
                match sync_file(
                    db,
                    &path,
                    &agent.file_name().to_string_lossy(),
                    &session.file_name().to_string_lossy(),
                    &key,
                    modified,
                ) {
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

fn sync_file(
    db: &Database,
    path: &Path,
    agent: &str,
    session: &str,
    key: &str,
    modified: i64,
) -> Result<SessionSyncResult, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = 0i64;
    let main = agent == "main";
    let mut requests = if main {
        RequestIndex::default()
    } else {
        RequestIndex::for_agent(agent)
    };
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
        requests.observe(&record);
        let (meta, usage) = if main {
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
            let meta = record["message"]["meta"].clone();
            let Some(usage) = meta.get("usage").cloned() else {
                continue;
            };
            (meta, usage)
        } else {
            // Subagent replies never reach the human projection; each completed step
            // carries its own usage, and the parent's subagent.completed summary is not
            // imported, so nothing is counted twice.
            let event = &record["event"];
            if record["type"].as_str() != Some("context.append_loop_event")
                || event["type"].as_str() != Some("step.end")
            {
                continue;
            }
            let Some(usage) = event.get("usage").cloned() else {
                continue;
            };
            let meta = serde_json::json!({
                "messageId": event["messageId"],
                "model": {"provider": "subagent"},
            });
            (meta, usage)
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
        let request = format!("kimi:{provider}:{identity}");
        // Retain only metadata and usage. step.end may appear after the human projection
        // (and after tools); resolve its messageId before taking the database lock.
        pending.push((request, meta, usage, created));
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
        id: &str,
        model: &str,
        start: i64,
        first: i64,
        stream: i64,
        output: u64,
    ) -> Vec<Value> {
        vec![
            json!({"type":"context.append_loop_event","time":start-10,"event":{"type":"step.begin","uuid":id,"turnId":"0","step":1}}),
            json!({"type":"llm.request","time":start,"agentId":"main","kind":"loop","turnStep":"0.1","provider":"openai","model":model}),
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

    #[test]
    fn kimi_usage_resolves_actual_models_and_timing_for_detail_and_aggregate_stats() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        let mut records = request("one", "model-one", 1_700_000_000_000, 3_500, 6_500, 900);
        records.extend(request(
            "two",
            "model-two",
            1_700_000_300_000,
            1_500,
            18_500,
            600,
        ));
        write_records(&path, &records);
        assert_eq!(
            sync_file(&db, &path, "main", "session", "file", 1)
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
        let records = request("live", "real-model", 1_700_000_000_000, 2_000, 8_000, 500);
        write_records(&path, &records[..4]);
        assert_eq!(
            sync_file(&db, &path, "main", "session", "file", 1)
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
            sync_file(&db, &path, "main", "session", "file", 2)
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
            sync_file(&db, &path, "main", "session", "file", 3)
                .unwrap()
                .imported,
            0
        );
        write_records(&path, &records[..4]);
        assert_eq!(
            sync_file(&db, &path, "main", "fork", "fork", 4)
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
            sync_file(&db, &path, "main", "s1", "first", 1)
                .unwrap()
                .imported,
            1
        );
        assert_eq!(
            sync_file(&db, &path, "main", "fork", "forked", 2)
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
            sync_file(&db, &path, "main", "s1", "rewritten", 3)
                .unwrap()
                .imported,
            0
        );
    }

    fn subagent_step(agent: &str, id: &str, model: &str, start: i64, output: u64) -> Vec<Value> {
        let usage =
            json!({"inputOther":10,"output":output,"inputCacheRead":30,"inputCacheCreation":0});
        vec![
            json!({"type":"context.append_loop_event","time":start-10,"event":{"type":"step.begin","uuid":id,"turnId":"0","step":1}}),
            json!({"type":"llm.request","time":start,"agentId":agent,"kind":"loop","turnStep":"0.1","provider":"openai","model":model}),
            json!({"type":"usage.record","time":start+900,"agentId":agent,"model":format!("kimi-code/{model}"),"usage":usage,"usageScope":"turn"}),
            json!({"type":"context.append_loop_event","time":start+1_000,"event":{"type":"step.end","uuid":id,"turnId":"0","step":1,"usage":usage,"messageId":id,"llmFirstTokenLatencyMs":200,"llmStreamDurationMs":800}}),
        ]
    }

    #[test]
    fn kimi_usage_imports_subagent_wires_once() {
        let db = Database::memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("wd_project/session_one/agents");
        write_records(
            &session.join("main/wire.jsonl"),
            &[
                request("main-step", "main-model", 1_700_000_000_000, 1_000, 1_000, 50),
                // The parent only records lifecycle events for its subagents.
                vec![json!({"type":"subagent.spawned","subagentId":"agent-0","time":1_700_000_001_000i64})],
            ]
            .concat(),
        );
        let mut child = vec![
            json!({"type":"agent.message.appended","time":1_700_000_001_000i64,"message":{"message":{"role":"user","content":[]},"meta":{}}}),
        ];
        child.extend(subagent_step(
            "agent-0",
            "child-1",
            "child-model",
            1_700_000_002_000,
            70,
        ));
        child.extend(subagent_step(
            "agent-0",
            "child-2",
            "child-model",
            1_700_000_004_000,
            90,
        ));
        // Requests attributed to another agent must not leak into this wire's models.
        child.push(
            json!({"type":"llm.request","agentId":"agent-1","kind":"loop","model":"other-model"}),
        );
        write_records(&session.join("agent-0/wire.jsonl"), &child);

        let result = sync_sessions(&db, root.path()).unwrap();
        assert_eq!((result.files_scanned, result.imported), (2, 3));
        {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT model, output_tokens, latency_ms, session_id FROM proxy_request_logs ORDER BY created_at, output_tokens")
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                rows,
                vec![
                    ("main-model".into(), 50, 2_000, "session_one".into()),
                    ("child-model".into(), 70, 1_000, "session_one".into()),
                    ("child-model".into(), 90, 1_000, "session_one".into()),
                ]
            );
        }
        // Unchanged wires are skipped; a forced rescan finds every step already recorded.
        assert_eq!(sync_sessions(&db, root.path()).unwrap().files_scanned, 0);
        assert_eq!(
            sync_file(
                &db,
                &session.join("agent-0/wire.jsonl"),
                "agent-0",
                "session_one",
                "rescan",
                9
            )
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
            sync_file(&db, &path, "main", "s", "partial", 1)
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
}
