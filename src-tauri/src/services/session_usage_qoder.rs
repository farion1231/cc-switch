//! Native Qoder transcripts carry Anthropic-shaped usage, including custom
//! provider identity. Import token usage without treating account Credits as USD.
//! Subagent transcripts under <session-id>/subagents/ carry the parent's
//! sessionId; import their usage alongside the main transcript and deduplicate
//! inherited messages by the same request identity.
use crate::{
    app_config::AppType,
    database::{lock_conn, Database},
    error::AppError,
    proxy::usage::{calculator::CostCalculator, parser::TokenUsage},
    services::{
        session_usage::{
            load_sync_cursors, metadata_modified_nanos, parse_timestamp_millis,
            update_sync_state_on_conn, SessionSyncResult,
        },
        sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL},
        usage_stats::find_model_pricing,
    },
};
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

pub fn sync_qoder_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    for app in [AppType::Qoder, AppType::QoderCn] {
        let providers = match crate::qoder_models::get(&app) {
            Ok(models) => models.providers,
            Err(error) => {
                result.errors.push(format!("{}: {error}", app.as_str()));
                continue;
            }
        };
        match sync_root(
            db,
            &app,
            &crate::qoder_config::data_dir(&app).join("projects"),
            &providers,
        ) {
            Ok(part) => result.merge(part),
            Err(error) => result.errors.push(format!("{}: {error}", app.as_str())),
        }
    }
    Ok(result)
}

fn transcript_files(root: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            transcript_files(&entry.path(), depth + 1, files);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "jsonl") {
            files.push(entry.path());
        }
    }
}

fn sync_root(
    db: &Database,
    app: &AppType,
    root: &Path,
    providers: &serde_json::Map<String, Value>,
) -> Result<SessionSyncResult, AppError> {
    let cursors = load_sync_cursors(db)?;
    let mut result = SessionSyncResult::default();
    let mut files = Vec::new();
    transcript_files(root, 0, &mut files);
    for path in files {
        let metadata = fs::metadata(&path).map_err(|error| AppError::io(&path, error))?;
        let modified = metadata_modified_nanos(&metadata);
        // Normalize retained v1 cache-inclusive input and recover requests
        // skipped by v2 when an empty message/request ID masked a valid UUID.
        let key = format!("{}:v3:{}", app.as_str(), path.display());
        if cursors.get(&key).is_some_and(|cursor| {
            cursor.last_modified == modified && cursor.last_line_offset == metadata.len() as i64
        }) {
            continue;
        }
        match sync_file(db, app, &path, &key, modified, providers) {
            Ok(part) => result.merge(part),
            Err(error) => {
                result
                    .errors
                    .push(format!("{} {}: {error}", app.as_str(), path.display()))
            }
        }
    }
    Ok(result)
}

struct UsageRecord {
    provider: String,
    model: String,
    session: String,
    created: i64,
    usage: TokenUsage,
    custom: bool,
    input_semantics: i64,
}

fn sync_file(
    db: &Database,
    app: &AppType,
    path: &Path,
    key: &str,
    modified: i64,
    providers: &serde_json::Map<String, Value>,
) -> Result<SessionSyncResult, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut bytes = 0i64;
    let mut buffer = Vec::new();
    let mut pending: BTreeMap<String, UsageRecord> = BTreeMap::new();
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        if size == 0 || buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        let Ok(record) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        if record["type"] != "assistant" || record["isApiErrorMessage"] == true {
            continue;
        }
        let message = &record["message"];
        let Some(usage) = message.get("usage").filter(|usage| usage.is_object()) else {
            continue;
        };
        let Some(identity) = [
            message["id"].as_str(),
            record["requestId"].as_str(),
            record["uuid"].as_str(),
        ]
        .into_iter()
        .flatten()
        .find(|id| !id.is_empty()) else {
            continue;
        };
        let Some(created) = record["timestamp"]
            .as_str()
            .and_then(parse_timestamp_millis)
        else {
            continue;
        };
        let tokens = |field: &str| usage[field].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
        let usage = TokenUsage {
            input_tokens: tokens("input_tokens"),
            output_tokens: tokens("output_tokens"),
            cache_read_tokens: tokens("cache_read_input_tokens"),
            cache_creation_tokens: tokens("cache_creation_input_tokens"),
            cache_creation_1h_tokens: usage
                .pointer("/cache_creation/ephemeral_1h_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            model: None,
            message_id: None,
        };
        let custom = record["customProviderId"]
            .as_str()
            .filter(|id| !id.is_empty());
        if usage.input_tokens == 0
            && usage.output_tokens == 0
            && usage.cache_read_tokens == 0
            && usage.cache_creation_tokens == 0
        {
            continue;
        }
        let fallback = if *app == AppType::QoderCn {
            "_qoder_cn_session"
        } else {
            "_qoder_session"
        };
        let raw_model = message["model"].as_str().unwrap_or("unknown");
        let model = custom
            .and_then(|id| raw_model.strip_prefix(&format!("{id}/")))
            .unwrap_or(raw_model);
        let request = format!("{}:{identity}", app.as_str());
        let next = UsageRecord {
            // OpenAI/Responses counters retain cached tokens in input_tokens;
            // Anthropic's counter already excludes cache reads and writes.
            input_semantics: if custom
                .and_then(|id| providers.get(id))
                .and_then(|provider| provider.get("protocol"))
                .and_then(Value::as_str)
                == Some("anthropic")
            {
                INPUT_TOKEN_SEMANTICS_FRESH
            } else {
                INPUT_TOKEN_SEMANTICS_TOTAL
            },
            provider: custom.unwrap_or(fallback).into(),
            model: model.into(),
            session: record["sessionId"]
                .as_str()
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown")
                })
                .into(),
            created: created / 1000,
            usage,
            custom: custom.is_some(),
        };
        // Multiple content blocks and streaming snapshots share a message ID.
        // They describe one request; retain the fullest counters once.
        if let Some(previous) = pending.get_mut(&request) {
            merge_tokens(&mut previous.usage, &next.usage);
        } else {
            pending.insert(request, next);
        }
    }
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let source = if *app == AppType::QoderCn {
        "qoder_cn_session"
    } else {
        "qoder_session"
    };
    for (request, mut record) in pending {
        // Persist the same fresh-input counters used for billing so dashboard
        // totals and daily rollups count cache reads and writes exactly once.
        if record.input_semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
            record.usage.input_tokens = record
                .usage
                .input_tokens
                .saturating_sub(record.usage.cache_read_tokens)
                .saturating_sub(record.usage.cache_creation_tokens);
        }
        let mut multiplier = Decimal::ONE;
        let inserted = tx.execute("INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES (?1, ?2, ?2, 1)", rusqlite::params![source, request])? > 0;
        if !inserted {
            // A receipt whose detail has been pruned must never recreate usage.
            let stored = tx.query_row("SELECT input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, COALESCE(cost_multiplier,'1'), model, provider_id, input_token_semantics FROM proxy_request_logs WHERE request_id = ?1 AND app_type = ?2", rusqlite::params![request, app.as_str()], |row| Ok((row.get::<_,u32>(0)?, row.get::<_,u32>(1)?, row.get::<_,u32>(2)?, row.get::<_,u32>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?,row.get::<_,i64>(7)?)));
            match stored {
                Ok((input, output, read, write, stored_multiplier, model, provider, semantics)) => {
                    multiplier = stored_multiplier.parse().unwrap_or(Decimal::ONE);
                    // Older details may still store total input. Compare and
                    // merge counters only after bringing both sides to fresh.
                    let input = if semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
                        input.saturating_sub(read).saturating_sub(write)
                    } else {
                        input
                    };
                    if input >= record.usage.input_tokens
                        && output >= record.usage.output_tokens
                        && read >= record.usage.cache_read_tokens
                        && write >= record.usage.cache_creation_tokens
                        && model == record.model
                        && provider == record.provider
                        && semantics == INPUT_TOKEN_SEMANTICS_FRESH
                    {
                        result.skipped += 1;
                        continue;
                    }
                    record.usage.input_tokens = record.usage.input_tokens.max(input);
                    record.usage.output_tokens = record.usage.output_tokens.max(output);
                    record.usage.cache_read_tokens = record.usage.cache_read_tokens.max(read);
                    record.usage.cache_creation_tokens =
                        record.usage.cache_creation_tokens.max(write);
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    result.skipped += 1;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let cost = if record.custom {
            find_model_pricing(&tx, &record.model).map(|pricing| {
                CostCalculator::calculate_for_app(app.as_str(), &record.usage, &pricing, multiplier)
            })
        } else {
            None
        };
        let amounts = cost
            .map(|cost| {
                [
                    cost.input_cost,
                    cost.output_cost,
                    cost.cache_read_cost,
                    cost.cache_creation_cost,
                    cost.total_cost,
                ]
            })
            .unwrap_or([Decimal::ZERO; 5])
            .map(|amount| amount.to_string());
        tx.execute("INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd, latency_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics)
            VALUES (?1,?2,?3,?4,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,200,?14,?15,1,'1',?16,?15,?17)
            ON CONFLICT(request_id) DO UPDATE SET model=excluded.model,request_model=excluded.request_model,provider_id=excluded.provider_id,input_token_semantics=excluded.input_token_semantics,input_tokens=excluded.input_tokens,output_tokens=excluded.output_tokens,cache_read_tokens=excluded.cache_read_tokens,cache_creation_tokens=excluded.cache_creation_tokens,
            input_cost_usd=excluded.input_cost_usd,output_cost_usd=excluded.output_cost_usd,cache_read_cost_usd=excluded.cache_read_cost_usd,cache_creation_cost_usd=excluded.cache_creation_cost_usd,total_cost_usd=excluded.total_cost_usd",
            rusqlite::params![request,record.provider,app.as_str(),record.model,record.usage.input_tokens,record.usage.output_tokens,record.usage.cache_read_tokens,record.usage.cache_creation_tokens,
                amounts[0],amounts[1],amounts[2],amounts[3],amounts[4],record.session,source,record.created,INPUT_TOKEN_SEMANTICS_FRESH])?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn message(id: &str, provider: Option<&str>, output: u32) -> Value {
        json!({"type":"assistant","uuid":format!("block-{output}"),"sessionId":"session", "timestamp":"2020-01-02T00:00:00Z", "customProviderId":provider,
            "message":{"id":id,"role":"assistant","model":if let Some(provider)=provider {format!("{provider}/fixture-model")}else{"fixture-model".into()},
            "usage":{"input_tokens":100,"output_tokens":output,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}})
    }
    fn write(path: &Path, records: &[Value]) {
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
    fn qoder_usage_imports_each_edition_and_deduplicates_streams_forks_and_pruned_history() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project/session.jsonl");
        let providers = json!({"custom":{"protocol":"openai"}})
            .as_object()
            .unwrap()
            .clone();
        write(
            &path,
            &[
                message("reply", Some("custom"), 5),
                message("reply", Some("custom"), 10),
            ],
        );
        for app in [AppType::Qoder, AppType::QoderCn] {
            let first = sync_root(&db, &app, dir.path(), &providers).unwrap();
            assert_eq!(first.imported, 1);
            assert!(first.errors.is_empty());
            assert_eq!(
                sync_root(&db, &app, dir.path(), &providers)
                    .unwrap()
                    .files_scanned,
                0
            );
        }
        let partial = message("partial", Some("custom"), 15).to_string();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(partial.as_bytes())
            .unwrap();
        assert_eq!(
            sync_root(&db, &AppType::QoderCn, dir.path(), &providers)
                .unwrap()
                .imported,
            0
        );
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(
            sync_root(&db, &AppType::QoderCn, dir.path(), &providers)
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),SUM(output_tokens) FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (3, 35));
        drop(conn);
        assert_eq!(db.rollup_and_prune(30).unwrap(), 3);
        write(
            &dir.path().join("project/fork.jsonl"),
            &[message("reply", Some("custom"), 20)],
        );
        assert_eq!(
            sync_root(&db, &AppType::QoderCn, dir.path(), &providers)
                .unwrap()
                .imported,
            0
        );
        let conn = db.conn.lock().unwrap();
        let counts:(i64,i64)=conn.query_row("SELECT (SELECT COUNT(*) FROM proxy_request_logs),(SELECT SUM(request_count) FROM usage_daily_rollups)",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, (0, 3));
    }

    #[test]
    fn qoder_usage_distinguishes_cached_inputs_and_does_not_price_official_credits() {
        let db = Database::memory().unwrap();
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO model_pricing (model_id,display_name,input_cost_per_million,output_cost_per_million,cache_read_cost_per_million,cache_creation_cost_per_million) VALUES ('fixture-model','Fixture','1','2','0.1','0.5')",[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project/session.jsonl");
        let providers =
            json!({"openai":{"protocol":"openai-responses"},"anthropic":{"protocol":"anthropic"}})
                .as_object()
                .unwrap()
                .clone();
        write(
            &path,
            &[
                message("o", Some("openai"), 5),
                message("a", Some("anthropic"), 5),
                message("official", None, 5),
            ],
        );
        for app in [AppType::Qoder, AppType::QoderCn] {
            assert_eq!(
                sync_root(&db, &app, dir.path(), &providers)
                    .unwrap()
                    .imported,
                3
            );
            let official = if app == AppType::QoderCn {
                "_qoder_cn_session"
            } else {
                "_qoder_session"
            };
            let conn = db.conn.lock().unwrap();
            for (provider, input, total, cost) in [
                ("openai", 70, 105, "0.00007"),
                ("anthropic", 100, 135, "0.0001"),
                (official, 70, 105, "0"),
            ] {
                let total_sql = crate::services::sql_helpers::real_total_tokens_sql("");
                let row: (String, String, i64, u32, u32) = conn
                    .query_row(
                        &format!("SELECT model,input_cost_usd,input_token_semantics,input_tokens,{total_sql} FROM proxy_request_logs WHERE app_type=?1 AND provider_id=?2"),
                        rusqlite::params![app.as_str(), provider],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                    )
                    .unwrap();
                assert_eq!(row.0, "fixture-model");
                assert_eq!(
                    row.1.parse::<Decimal>().unwrap(),
                    cost.parse::<Decimal>().unwrap()
                );
                assert_eq!(row.2, INPUT_TOKEN_SEMANTICS_FRESH);
                assert_eq!(row.3, input);
                assert_eq!(row.4, total);
            }
            drop(conn);
            let session = db
                .get_session_usage_summary(app.as_str(), "session")
                .unwrap();
            assert_eq!(session.total_input_tokens, 240);
            assert_eq!(session.real_total_tokens, 345);
        }
        assert_eq!(db.rollup_and_prune(30).unwrap(), 6);
        for app in [AppType::Qoder, AppType::QoderCn] {
            let summary = db
                .get_usage_summary(None, None, Some(app.as_str()), None, None)
                .unwrap();
            assert_eq!(summary.total_requests, 3);
            assert_eq!(summary.total_input_tokens, 240);
            assert_eq!(summary.real_total_tokens, 345);
        }
    }

    #[test]
    fn qoder_usage_normalizes_retained_details_before_deduplication_and_max_comparisons() {
        for app in [AppType::Qoder, AppType::QoderCn] {
            for semantics in [INPUT_TOKEN_SEMANTICS_TOTAL, INPUT_TOKEN_SEMANTICS_FRESH] {
                let db = Database::memory().unwrap();
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("project/session.jsonl");
                let providers = serde_json::Map::new();
                let original = message("reply", None, 5);
                write(&path, &[original.clone()]);
                let old_key = format!("{}:v1:{}", app.as_str(), path.display());
                sync_file(&db, &app, &path, &old_key, 0, &providers).unwrap();
                let input = if semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
                    100
                } else {
                    70
                };
                db.conn
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE proxy_request_logs SET input_tokens=?1,input_token_semantics=?2",
                        rusqlite::params![input, semantics],
                    )
                    .unwrap();
                let metadata = fs::metadata(&path).unwrap();
                update_sync_state_on_conn(
                    &db.conn.lock().unwrap(),
                    &old_key,
                    metadata_modified_nanos(&metadata),
                    metadata.len() as i64,
                )
                .unwrap();

                // An unchanged v1 file is revisited once; TOTAL rows must be
                // rewritten even when all their normalized counters match.
                let result = sync_root(&db, &app, dir.path(), &providers).unwrap();
                assert_eq!(result.files_scanned, 1);
                assert_eq!(
                    result.imported,
                    u32::from(semantics == INPUT_TOKEN_SEMANTICS_TOTAL)
                );
                let stored: (u32, i64) = db
                    .conn
                    .lock()
                    .unwrap()
                    .query_row(
                        "SELECT input_tokens,input_token_semantics FROM proxy_request_logs",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(stored, (70, INPUT_TOKEN_SEMANTICS_FRESH));
                assert_eq!(
                    sync_root(&db, &app, dir.path(), &providers)
                        .unwrap()
                        .files_scanned,
                    0
                );

                // A later snapshot has 120 total input, 40 cached, and 80 fresh.
                // Merge against fresh stored input, then leave a stale fork alone.
                let mut next = message("reply", None, 8);
                next["message"]["usage"]["input_tokens"] = json!(120);
                next["message"]["usage"]["cache_read_input_tokens"] = json!(25);
                next["message"]["usage"]["cache_creation_input_tokens"] = json!(15);
                // Exercise old TOTAL details in the update branch as well.
                db.conn
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE proxy_request_logs SET input_tokens=?1,input_token_semantics=?2",
                        rusqlite::params![input, semantics],
                    )
                    .unwrap();
                let key = format!("{}:v3:{}", app.as_str(), path.display());
                write(&path, &[next]);
                assert_eq!(
                    sync_file(&db, &app, &path, &key, 0, &providers)
                        .unwrap()
                        .imported,
                    1
                );
                assert_eq!(
                    sync_file(&db, &app, &path, &key, 0, &providers)
                        .unwrap()
                        .skipped,
                    1
                );
                write(&path, &[original]);
                assert_eq!(
                    sync_file(&db, &app, &path, &key, 0, &providers)
                        .unwrap()
                        .skipped,
                    1
                );
                let summary = db
                    .get_session_usage_summary(app.as_str(), "session")
                    .unwrap();
                assert_eq!(summary.total_requests, 1);
                assert_eq!(summary.total_input_tokens, 80);
                assert_eq!(summary.real_total_tokens, 128);
            }
        }
    }

    #[test]
    fn qoder_usage_saturates_fresh_input_when_cache_counters_exceed_total_input() {
        let db = Database::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut record = message("reply", None, 5);
        record["message"]["usage"]["input_tokens"] = json!(10);
        write(&path, &[record]);
        sync_root(&db, &AppType::Qoder, dir.path(), &serde_json::Map::new()).unwrap();
        let summary = db.get_session_usage_summary("qoder", "session").unwrap();
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.real_total_tokens, 35);
    }

    #[test]
    fn qoder_usage_counts_subagents_once_in_parent_session_and_rollups() {
        for app in [AppType::Qoder, AppType::QoderCn] {
            let db = Database::memory().unwrap();
            db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO model_pricing (model_id,display_name,input_cost_per_million,output_cost_per_million,cache_read_cost_per_million,cache_creation_cost_per_million) VALUES ('fixture-model','Fixture','1','2','0.1','0.5')",[]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let project = dir.path().join("project");
            let agents = project.join("session/subagents");
            let providers = json!({
                "openai": {"protocol": "openai-responses"},
                "anthropic": {"protocol": "anthropic"}
            })
            .as_object()
            .unwrap()
            .clone();
            let parent = message("parent", Some("openai"), 5);
            write(&project.join("session.jsonl"), &[parent.clone()]);
            let sidechain = |mut record: Value, agent: &str| {
                record["isSidechain"] = json!(true);
                record["agentId"] = json!(agent);
                record
            };
            let child = |output| sidechain(message("child", Some("openai"), output), "worker");
            // Forks retain inherited parent messages. Content blocks and
            // snapshots of the child's reply still represent one request.
            let worker = agents.join("agent-worker.jsonl");
            write(
                &worker,
                &[sidechain(parent.clone(), "worker"), child(5), child(10)],
            );
            let mut cached = sidechain(message("cached", Some("anthropic"), 0), "cached");
            cached["message"]["usage"]["input_tokens"] = json!(0);
            cached["message"]["usage"]["cache_read_input_tokens"] = json!(500);
            cached["message"]["usage"]["cache_creation_input_tokens"] = json!(20);
            write(&agents.join("agent-cached.jsonl"), &[cached]);

            let first = sync_root(&db, &app, dir.path(), &providers).unwrap();
            assert!(first.errors.is_empty(), "{:?}", first.errors);
            assert_eq!(first.files_scanned, 3);
            assert_eq!(first.imported, 3);
            let summary = db
                .get_session_usage_summary(app.as_str(), "session")
                .unwrap();
            assert_eq!(summary.total_requests, 3);
            assert_eq!(summary.total_input_tokens, 140);
            assert_eq!(summary.total_output_tokens, 15);
            assert_eq!(summary.total_cache_read_tokens, 540);
            assert_eq!(summary.total_cache_creation_tokens, 40);
            assert_eq!(summary.real_total_tokens, 735);
            assert_eq!(
                summary.total_cost.parse::<Decimal>().unwrap(),
                Decimal::new(244, 6)
            );
            assert_eq!(
                sync_root(&db, &app, dir.path(), &providers)
                    .unwrap()
                    .files_scanned,
                0
            );

            // A running agent may append a fuller snapshot while the parent
            // transcript remains unchanged. Refresh its usage without adding
            // another request or recounting the inherited context.
            fs::OpenOptions::new()
                .append(true)
                .open(&worker)
                .unwrap()
                .write_all(format!("{}\n", child(15)).as_bytes())
                .unwrap();
            let update = sync_root(&db, &app, dir.path(), &providers).unwrap();
            assert_eq!(update.files_scanned, 1);
            assert_eq!(update.imported, 1);
            let summary = db
                .get_session_usage_summary(app.as_str(), "session")
                .unwrap();
            assert_eq!(summary.total_requests, 3);
            assert_eq!(summary.real_total_tokens, 740);
            assert_eq!(
                summary.total_cost.parse::<Decimal>().unwrap(),
                Decimal::new(254, 6)
            );

            assert_eq!(db.rollup_and_prune(30).unwrap(), 3);
            write(&agents.join("agent-fork.jsonl"), &[child(20)]);
            assert_eq!(
                sync_root(&db, &app, dir.path(), &providers)
                    .unwrap()
                    .imported,
                0
            );
            let summary = db
                .get_usage_summary(None, None, Some(app.as_str()), None, None)
                .unwrap();
            assert_eq!(summary.total_requests, 3);
            assert_eq!(summary.real_total_tokens, 740);
            assert_eq!(
                summary.total_cost.parse::<Decimal>().unwrap(),
                Decimal::new(254, 6)
            );
        }
    }

    #[test]
    fn qoder_usage_backfills_subagent_replies_with_empty_ids() {
        for app in [AppType::Qoder, AppType::QoderCn] {
            let db = Database::memory().unwrap();
            let dir = tempfile::tempdir().unwrap();
            let path = dir
                .path()
                .join("project/session/subagents/agent-worker.jsonl");
            let mut request = message("", None, 5);
            request["isSidechain"] = json!(true);
            request["agentId"] = json!("worker");
            request["requestId"] = json!("request");
            let mut snapshot = request.clone();
            snapshot["uuid"] = json!("request-snapshot");
            snapshot["message"]["usage"]["output_tokens"] = json!(10);
            let mut uuid = request.clone();
            uuid["requestId"] = json!("");
            uuid["uuid"] = json!("reply-uuid");
            let mut unidentified = uuid.clone();
            unidentified["uuid"] = json!("");
            write(&path, &[request, snapshot, uuid, unidentified]);
            // v2 marked these records as scanned despite dropping their usage.
            let metadata = fs::metadata(&path).unwrap();
            update_sync_state_on_conn(
                &db.conn.lock().unwrap(),
                &format!("{}:v2:{}", app.as_str(), path.display()),
                metadata_modified_nanos(&metadata),
                metadata.len() as i64,
            )
            .unwrap();

            let result = sync_root(&db, &app, dir.path(), &serde_json::Map::new()).unwrap();
            assert!(result.errors.is_empty(), "{:?}", result.errors);
            assert_eq!(result.files_scanned, 1);
            assert_eq!(result.imported, 2);
            let summary = db
                .get_session_usage_summary(app.as_str(), "session")
                .unwrap();
            assert_eq!(summary.total_requests, 2);
            assert_eq!(summary.real_total_tokens, 215);
            assert_eq!(
                sync_root(&db, &app, dir.path(), &serde_json::Map::new())
                    .unwrap()
                    .files_scanned,
                0
            );
        }
    }
}

fn merge_tokens(previous: &mut TokenUsage, next: &TokenUsage) {
    previous.input_tokens = previous.input_tokens.max(next.input_tokens);
    previous.output_tokens = previous.output_tokens.max(next.output_tokens);
    previous.cache_read_tokens = previous.cache_read_tokens.max(next.cache_read_tokens);
    previous.cache_creation_tokens = previous
        .cache_creation_tokens
        .max(next.cache_creation_tokens);
    previous.cache_creation_1h_tokens = previous
        .cache_creation_1h_tokens
        .max(next.cache_creation_1h_tokens);
}
