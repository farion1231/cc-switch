//! Exact counters exported by agy; absent models remain unknown (never use today's provider).
//! Amounts are the matching Gemini API prices, like every other session-log import.
use super::{
    session_usage::{cost_columns, load_sync_cursors, metadata_modified_nanos, SessionSyncResult},
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
};
use crate::proxy::usage::calculator::ModelPricing;
use crate::proxy::usage::parser::TokenUsage;
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    session_manager::providers::agy::{self, model_from_settings_change},
};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod native_models;

#[cfg(test)]
mod orchestrator_tests;

struct AgySyncCursorUpdate<'a> {
    key: &'a str,
    modified: i64,
    size: i64,
    native_state: &'a str,
}

type ParentModelEvent = (i64, Option<String>);
type CachedParentEvents = (Vec<ParentModelEvent>, bool);

#[derive(Default)]
struct SyncParentCache {
    lookup: agy::ParentLookupCache,
    parent_events: HashMap<PathBuf, CachedParentEvents>,
}

pub fn sync_agy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_agy_usage_from_files(db, agy::transcript_files())
}

fn sync_agy_usage_from_files(
    db: &Database,
    files: Vec<(String, PathBuf)>,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    if files.is_empty() {
        return Ok(result);
    }
    let cursors = load_sync_cursors(db)?;
    let mut parent_cache = SyncParentCache::default();
    for (session_id, path) in files {
        result.files_scanned += 1;
        let cursor_key = format!("agy:{}", path.to_string_lossy());
        let cursor = cursors.get(&cursor_key);
        let metadata = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) => {
                result
                    .errors
                    .push(format!("Cannot read Antigravity transcript: {e}"));
                continue;
            }
        };
        let modified = metadata_modified_nanos(&metadata);
        let size = metadata.len() as i64;
        // Invalidate pre-Orchestrator / pre-worker-alias / pre-Neon-Orchestrator /
        // pre-parent-inheritance cursors once, including unchanged, already timed history.
        let native_state = format!("v5:{}", native_models::native_state_fingerprint(&path));
        if cursor.is_some_and(|c| {
            c.last_modified == modified
                && c.last_byte_offset == Some(size)
                && c.parser_state.as_deref() == Some(native_state.as_str())
        }) {
            continue;
        }
        match agy::records(&path) {
            Ok(rows) => {
                let sync_cursor = AgySyncCursorUpdate {
                    key: &cursor_key,
                    modified,
                    size,
                    native_state: &native_state,
                };
                match import_records_with_cursor(
                    db,
                    &session_id,
                    &rows,
                    Some(&path),
                    Some(sync_cursor),
                    &mut parent_cache,
                ) {
                    Ok((imported, skipped)) => {
                        result.imported += imported;
                        result.skipped += skipped;
                    }
                    Err(e) => result.errors.push(format!("Antigravity {session_id}: {e}")),
                }
            }
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

/// agy 记下的是显示名（`Gemini 3.8 Flash (High)`）；换成 `agy models` 第一列那样的 ID
/// （`gemini-3.8-flash-high`）才能对上定价表，查价时档位后缀会被剥掉。已经是 ID 的原样返回。
fn model_id_of(display: &str) -> String {
    if !display.contains(' ') {
        return display.to_string();
    }
    display
        .to_ascii_lowercase()
        .replace(" (", "-")
        .replace(['(', ')'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// Multi-model modes, not evidence of the model used for an individual response.
/// Display name and native alias of the same mode map to the same planner.
fn orchestrator_mode(model: &str) -> Option<&'static str> {
    match model_id_of(model.trim()).to_ascii_lowercase().as_str() {
        "orchestrator-carbon" | "gemini-pro-orchestrator" => Some("carbon"),
        "orchestrator-argon"
        | "gemini-pro-orchestrator-argon"
        | "gemini-pro-orchestrator-g1-pro" => Some("argon"),
        "orchestrator-neon" | "gemini-pro-orchestrator-neon" => Some("neon"),
        _ => None,
    }
}

fn is_orchestrator(model: &str) -> bool {
    orchestrator_mode(model).is_some()
}

/// Per-step alias exported by Antigravity (`agy` CLI and Desktop 2.33+) on Orchestrator worker
/// generations (`MODEL_PLACEHOLDER_M669`). Unlike `orchestrator_mode`, this already identifies the
/// Hydrogen worker role even without `conversations/<id>.db`, while still preferring native
/// field-21 display/tier metadata when present.
fn is_orchestrator_worker(model: &str) -> bool {
    model_id_of(model.trim()).eq_ignore_ascii_case("gemini-flash-worker-agy")
}

/// Mode names, role aliases, and generic labels that do not name a concrete worker model/tier.
fn is_unresolved_worker_target(model: &str) -> bool {
    let id = model_id_of(model.trim());
    id.is_empty()
        || id.eq_ignore_ascii_case("unknown")
        || is_orchestrator(&id)
        || is_orchestrator_worker(&id)
        || id == "gemini-flash"
}

/// 五个金额（输入、输出、缓存读、缓存写、合计）；查不到价格的记 0。
fn costs(
    conn: &rusqlite::Connection,
    pricing_cache: &mut HashMap<String, Option<ModelPricing>>,
    model: &str,
    tokens: [i64; 4],
) -> [String; 5] {
    let [input, output, cache_read, cache_write] =
        tokens.map(|v| u32::try_from(v).unwrap_or(u32::MAX));
    let usage = TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };
    let pricing = pricing_cache
        .entry(model.to_string())
        .or_insert_with(|| crate::services::usage_stats::find_model_pricing(conn, model));
    cost_columns("agy", &usage, pricing.as_ref(), rust_decimal::Decimal::ONE)
}

#[cfg(test)]
fn import_records(
    db: &Database,
    session: &str,
    rows: &[Value],
    transcript: Option<&Path>,
) -> Result<(u32, u32), AppError> {
    let mut parent_cache = SyncParentCache::default();
    import_records_with_cursor(db, session, rows, transcript, None, &mut parent_cache)
}

fn import_records_with_cursor(
    db: &Database,
    session: &str,
    rows: &[Value],
    transcript: Option<&Path>,
    sync_cursor: Option<AgySyncCursorUpdate<'_>>,
    parent_cache: &mut SyncParentCache,
) -> Result<(u32, u32), AppError> {
    // step_index 顺序决定模型归属：先按顺序扫一遍切换事件，再给每个响应打上当时的模型。
    let mut ordered: Vec<&Value> = rows.iter().collect();
    ordered.sort_by_key(|row| row["step_index"].as_u64().unwrap_or(u64::MAX));
    let mut current_model: Option<String> = None;
    let mut model_by_step = std::collections::HashMap::<u64, String>::new();
    for row in &ordered {
        let Some(step) = row["step_index"].as_u64() else {
            continue;
        };
        if row["type"] == "USER_INPUT" {
            if let Some(model) = row["content"].as_str().and_then(model_from_settings_change) {
                current_model = Some(model);
            }
        } else if row["type"] == "PLANNER_RESPONSE" {
            if let Some(model) = &current_model {
                model_by_step.entry(step).or_insert_with(|| model.clone());
            }
        }
    }

    // 子代理通常没有 Model Selection；Orchestrator 的选择只代表模式，需逐次读取实际模型。
    // 耗时和首字也只在原生库里有（transcript 的时间戳只到秒、没有完成时间）。
    let mut timing_by_step = std::collections::HashMap::new();
    let mut native_model_by_step = HashMap::new();
    let mut native_ok = true;
    if let Some(transcript) = transcript {
        let (settled, pruned) = steps_with_timing(db, session, &ordered)?;
        let wanted: Vec<u64> = ordered
            .iter()
            .filter(|row| row["type"] == "PLANNER_RESPONSE" && row["status"] == "DONE")
            .filter_map(|row| {
                let step = row["step_index"].as_u64()?;
                let selected =
                    known_model(row).or_else(|| model_by_step.get(&step).map(String::as_str));
                // Pruned steps can never be updated again, so their model is moot.
                let needs_model = selected
                    .is_none_or(|m| is_orchestrator(m) || is_orchestrator_worker(m))
                    && !pruned.contains(&step);
                (needs_model || !settled.contains(&step)).then_some(step)
            })
            .collect();
        if !wanted.is_empty() {
            match native_models::generations_for_transcript(transcript, &wanted) {
                Ok(generations) => {
                    for (step, generation) in generations {
                        if let Some(model) = generation.model {
                            native_model_by_step.insert(step, model);
                        }
                        if let Some(timing) = generation.timing {
                            timing_by_step.insert(step, timing);
                        }
                    }
                }
                Err(error) => {
                    native_ok = false;
                    log::warn!("Antigravity {session}: cannot read native models: {error}");
                }
            }
        }
        // Own transcript and own database still have no model. Use the parent's concrete
        // model from just before this session started (Model Selection, else the parent's
        // database). Never inherit Orchestrator/worker selector names or guess a tier.
        // A failed parent database read leaves the sync cursor unset so the next sync retries.
        if native_ok {
            let missing: Vec<u64> = ordered
                .iter()
                .filter(|row| row["type"] == "PLANNER_RESPONSE" && row["status"] == "DONE")
                .filter_map(|row| {
                    let step = row["step_index"].as_u64()?;
                    (known_model(row).is_none()
                        && !model_by_step.contains_key(&step)
                        && !native_model_by_step.contains_key(&step)
                        && !pruned.contains(&step))
                    .then_some(step)
                })
                .collect();
            if !missing.is_empty() {
                let child_start = ordered.iter().filter_map(|row| agy::timestamp(row)).min();
                let (model, parent_native_ok) = parent_model(transcript, child_start, parent_cache);
                if !parent_native_ok {
                    native_ok = false;
                }
                if let Some(model) = model {
                    for step in missing {
                        model_by_step.entry(step).or_insert_with(|| model.clone());
                    }
                }
            }
        }
    }

    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let mut pricing_cache = HashMap::new();
    let (mut imported, mut skipped) = (0, 0);
    for row in rows {
        if row["type"] != "PLANNER_RESPONSE" || row["status"] != "DONE" {
            continue;
        }
        let Some(step) = row["step_index"].as_u64() else {
            continue;
        };
        let token = |name: &str| {
            row[name]
                .as_u64()
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or(0)
        };
        let input = token("input_tokens");
        let output = token("output_tokens");
        let cache = token("cache_read_tokens");
        let write = token("cache_write_tokens");
        if input == 0 && output == 0 && cache == 0 && write == 0 {
            continue;
        }
        let id = format!("agy:{session}:{step}");
        // Persist a tombstone with the row so retention never imports pruned history again.
        let fresh = tx
            .prepare_cached("INSERT OR IGNORE INTO session_usage_dedup(data_source,request_id,semantic_id,has_entry_id) VALUES ('agy_session',?1,?1,1)")?
            .execute([&id])?;
        // `model` 存 ID（对得上定价和其它应用的统计），`request_model` 留显示名。
        let display = known_model(row)
            .map(str::to_string)
            .or_else(|| model_by_step.get(&step).cloned())
            .or_else(|| native_model_by_step.get(&step).cloned())
            .unwrap_or_else(|| "unknown".to_string());
        let native_worker_model = native_model_by_step
            .get(&step)
            .map(String::as_str)
            .filter(|model| !is_unresolved_worker_target(model));
        let worker_without_native =
            is_orchestrator_worker(&display) && native_worker_model.is_none();
        let actual = if is_orchestrator(&display) {
            native_model_by_step
                .get(&step)
                .map(String::as_str)
                .unwrap_or(&display)
        } else if is_orchestrator_worker(&display) {
            native_worker_model.unwrap_or("gemini-4-hydrogen")
        } else {
            &display
        };
        let model = model_id_of(actual);
        // 原生库暂时不可读时，保留已恢复的模型；Token 仍按最新日志更新。
        let (model, display) = if (model == "unknown"
            || is_orchestrator(&model)
            || worker_without_native)
            && fresh == 0
        {
            let previous = tx
                .prepare_cached(
                    "SELECT model, COALESCE(request_model, model) FROM proxy_request_logs
                  WHERE request_id=?1 AND model!='unknown'",
                )?
                .query_row([&id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .optional()?;
            match previous {
                Some((old_model, old_display)) if model == "unknown" => (old_model, old_display),
                Some((old_model, old_display))
                    if worker_without_native
                        && ((is_orchestrator_worker(&old_display)
                            && !is_unresolved_worker_target(&old_model))
                            || (is_orchestrator(&old_display)
                                && old_model.starts_with("gemini-4-hydrogen"))) =>
                {
                    (old_model, display)
                }
                Some((old_model, old_display))
                    if !worker_without_native
                        && !is_orchestrator(&old_model)
                        && !is_orchestrator_worker(&old_model)
                        && orchestrator_mode(&old_display) == orchestrator_mode(&display) =>
                {
                    (old_model, old_display)
                }
                _ => (model, display),
            }
        } else {
            (model, display)
        };
        let [input_cost, output_cost, cache_cost, write_cost, total_cost] = costs(
            &tx,
            &mut pricing_cache,
            &model,
            [input, output, cache, write],
        );
        // 原生库记的是精确值；读不到时耗时记 0、首字记 NULL，和其它会话日志一样表示「没有计时」。
        let timing = timing_by_step.get(&step);
        let latency_ms = timing.map_or(0, |t| t.latency_ms);
        let first_token_ms = timing.map(|t| t.first_token_ms);
        if fresh == 0 {
            // A rewritten completed step may correct counters; update retained detail only.
            // If retention already removed it, its tombstone still forbids resurrection.
            // Timing is only ever filled in: an unreadable native database keeps what was stored.
            let changed = tx
                .prepare_cached("UPDATE proxy_request_logs SET model=?2,request_model=?7,input_tokens=?3,output_tokens=?4,cache_read_tokens=?5,cache_creation_tokens=?6,input_cost_usd=?8,output_cost_usd=?9,cache_read_cost_usd=?10,cache_creation_cost_usd=?11,total_cost_usd=?12,latency_ms=CASE WHEN ?13>0 THEN ?13 ELSE latency_ms END,first_token_ms=COALESCE(?14,first_token_ms) WHERE request_id=?1 AND (model!=?2 OR request_model!=?7 OR input_tokens!=?3 OR output_tokens!=?4 OR cache_read_tokens!=?5 OR cache_creation_tokens!=?6 OR total_cost_usd!=?12 OR (?13>0 AND latency_ms!=?13) OR (?14 IS NOT NULL AND first_token_ms IS NOT ?14))")?
                .execute(params![id,model,input,output,cache,write,display,input_cost,output_cost,cache_cost,write_cost,total_cost,latency_ms,first_token_ms])?;
            imported += changed as u32;
            skipped += u32::from(changed == 0);
            continue;
        }
        // Native agy was verified with a local mock: input excludes cache, output includes
        // thinking. Never subtract cache or add thinking a second time. Missing historical
        // models remain unknown and unpriced; never fall back to the active provider.
        tx.prepare_cached("INSERT INTO proxy_request_logs (
            request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
            cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,
            cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,latency_ms,first_token_ms,status_code,
            session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,input_token_semantics
        ) VALUES (?1,'_agy_session','agy',?2,?10,?3,?4,?5,?6,?11,?12,?13,?14,?15,?16,?17,200,?7,'agy_session',1,'1',?8,'agy_session',?9)")?
        .execute(params![id, model, input, output, cache, write, session, agy::timestamp(row).unwrap_or(0)/1000, INPUT_TOKEN_SEMANTICS_FRESH, display, input_cost, output_cost, cache_cost, write_cost, total_cost, latency_ms, first_token_ms])?;
        imported += 1;
    }
    if let Some(cursor) = sync_cursor {
        let parser_state = native_ok.then_some(cursor.native_state);
        tx.prepare_cached(
            "INSERT OR REPLACE INTO session_log_sync
                 (file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset, parser_state)
             VALUES (?1, ?2, 0, unixepoch(), ?3, ?4)",
        )?
        .execute(params![
            cursor.key,
            cursor.modified,
            cursor.size,
            parser_state,
        ])?;
    }
    tx.commit()?;
    Ok((imported, skipped))
}

/// Steps that need nothing more from the native database for timing: imported with a
/// timing already, or imported and since pruned (the tombstone forbids re-importing them).
/// Also returns the pruned subset on its own. Keeps steady-state syncs from reopening
/// every conversation database.
fn steps_with_timing(
    db: &Database,
    session: &str,
    rows: &[&Value],
) -> Result<(HashSet<u64>, HashSet<u64>), AppError> {
    let conn = lock_conn!(db.conn);
    let mut query = conn.prepare(
        "SELECT EXISTS(SELECT 1 FROM session_usage_dedup
                       WHERE data_source='agy_session' AND request_id=?1),
                EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id=?1),
                EXISTS(SELECT 1 FROM proxy_request_logs
                       WHERE request_id=?1 AND latency_ms<=0)",
    )?;
    let (mut settled, mut pruned) = (HashSet::new(), HashSet::new());
    for step in rows
        .iter()
        .filter(|row| row["type"] == "PLANNER_RESPONSE" && row["status"] == "DONE")
        .filter_map(|row| row["step_index"].as_u64())
    {
        let (imported, detail, untimed) =
            query.query_row([format!("agy:{session}:{step}")], |r| {
                Ok((
                    r.get::<_, bool>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?;
        if imported && !untimed {
            settled.insert(step);
        }
        if imported && !detail {
            pruned.insert(step);
        }
    }
    Ok((settled, pruned))
}

/// Parent concrete model active at the latest moment that is not after `child_start`.
///
/// Mode selector names (`Orchestrator (*)`, `gemini-flash-worker-agy`) clear any earlier
/// selection rather than inheriting either a selector name or a stale pre-Orchestrator model.
/// The bool is false only when the parent's transcript or conversation database exists
/// but cannot be opened. The sync cursor stays unset in that case.
fn parent_model(
    transcript: &Path,
    child_start: Option<i64>,
    cache: &mut SyncParentCache,
) -> (Option<String>, bool) {
    let Some(parent) = cache.lookup.parent_session_id(transcript, child_start) else {
        return (None, true);
    };
    let Some((root, _)) = agy::transcript_home(transcript) else {
        return (None, true);
    };
    let parent_transcript = root
        .join("brain")
        .join(&parent)
        .join(".system_generated/logs/transcript_full.jsonl");
    if !parent_transcript.is_file() {
        return (None, true);
    }
    if let Some((events, parent_native_ok)) = cache.parent_events.get(&parent_transcript) {
        return (model_active_before(events, child_start), *parent_native_ok);
    }
    let rows = match agy::records(&parent_transcript) {
        Ok(rows) => rows,
        Err(err) => {
            log::warn!(
                "Antigravity parent {parent}: cannot read transcript for model inheritance: {err}"
            );
            return (None, false);
        }
    };
    let mut events = Vec::new();
    let mut steps = Vec::new();
    for row in &rows {
        let Some(at) = agy::timestamp(row) else {
            continue;
        };
        if row["type"] == "USER_INPUT" {
            if let Some(model) = row["content"].as_str().and_then(model_from_settings_change) {
                if is_unresolved_worker_target(&model) {
                    events.push((at, None));
                } else {
                    events.push((at, Some(model)));
                }
            }
        } else if row["type"] == "PLANNER_RESPONSE" && row["status"] == "DONE" {
            if let Some(step) = row["step_index"].as_u64() {
                steps.push((step, at));
            }
        }
    }
    let mut parent_native_ok = true;
    if !steps.is_empty() {
        let wanted: Vec<u64> = steps.iter().map(|(step, _)| *step).collect();
        match native_models::generations_for_transcript(&parent_transcript, &wanted) {
            Ok(generations) => {
                for (step, at) in steps {
                    if let Some(model) = generations.get(&step).and_then(|item| item.model.clone())
                    {
                        if !is_unresolved_worker_target(&model) {
                            events.push((at, Some(model)));
                        }
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "Antigravity parent {parent}: cannot read native models for inheritance: {err}"
                );
                parent_native_ok = false;
            }
        }
    }
    let chosen = model_active_before(&events, child_start);
    cache
        .parent_events
        .insert(parent_transcript, (events, parent_native_ok));
    (chosen, parent_native_ok)
}

/// Latest model state at or before `child_start`. A later switch (including entering
/// Orchestrator mode, represented as `None`) overrides earlier concrete selections;
/// within the same 1-second timestamp, a resolved concrete model beats a `None` marker.
fn model_active_before(events: &[ParentModelEvent], child_start: Option<i64>) -> Option<String> {
    let mut best: Option<(i64, Option<&str>)> = None;
    for (at, model) in events {
        if child_start.is_some_and(|start| *at > start) {
            continue;
        }
        let concrete = model
            .as_deref()
            .filter(|name| !is_unresolved_worker_target(name));
        let replace = match best {
            None => true,
            Some((best_at, _)) if *at > best_at => true,
            Some((best_at, best_model)) if *at == best_at => {
                concrete.is_some() || best_model.is_none()
            }
            Some(_) => false,
        };
        if replace {
            best = Some((*at, concrete));
        }
    }
    best.and_then(|(_, model)| model.map(str::to_string))
}

fn known_model(row: &Value) -> Option<&str> {
    row["model"]
        .as_str()
        .filter(|model| !model.trim().is_empty() && !model.eq_ignore_ascii_case("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_models_backfill_unknown_rows_without_changing_tokens_or_deduplication() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/subagent/.system_generated/logs/transcript_full.jsonl");
        let path = dir.path().join("conversations/subagent.db");
        let db = Database::memory().unwrap();
        let mut rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":80,"output_tokens":12,"cache_read_tokens":20}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"unknown","input_tokens":100,"output_tokens":5}),
        ];
        assert_eq!(
            import_records(&db, "subagent", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        let native = native_models::write_test_database(
            &path,
            &[(1, 0, "gemini-4-argon"), (3, 7, "gemini-3.8-flash-tiered")],
        );
        assert_eq!(
            import_records(&db, "subagent", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        assert_eq!(
            import_records(&db, "subagent", &rows, Some(&source)).unwrap(),
            (0, 2)
        );
        {
            let conn = db.conn.lock().unwrap();
            let got = conn
                .query_row(
                    "SELECT model,input_tokens,output_tokens,cache_read_tokens,total_cost_usd
                 FROM proxy_request_logs WHERE request_id='agy:subagent:1'",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, String>(4)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(
                (&*got.0, got.1, got.2, got.3),
                ("gemini-4-argon", 80, 12, 20)
            );
            assert!(got.4.parse::<f64>().unwrap() > 0.0);
            let flash: (String, String) = conn.query_row(
                "SELECT model,total_cost_usd FROM proxy_request_logs WHERE request_id='agy:subagent:3'", [],
                |r| Ok((r.get(0)?,r.get(1)?)),
            ).unwrap();
            assert_eq!(
                flash,
                ("gemini-3.8-flash-tiered".into(), "0.00009375".into())
            );
        }
        // A missing native table must not erase recovered names or block newer counters.
        native.execute_batch("DROP TABLE gen_metadata").unwrap();
        rows[0]["input_tokens"] = json!(90);
        assert_eq!(
            import_records(&db, "subagent", &rows, Some(&source)).unwrap(),
            (1, 1)
        );
        {
            let conn = db.conn.lock().unwrap();
            let got: (String, i64) = conn.query_row(
                "SELECT model,input_tokens FROM proxy_request_logs WHERE request_id='agy:subagent:1'", [],
                |r| Ok((r.get(0)?,r.get(1)?)),
            ).unwrap();
            assert_eq!(got, ("gemini-4-argon".into(), 90));
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        assert_eq!(
            import_records(&db, "subagent", &rows, Some(&source)).unwrap(),
            (0, 2)
        );
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn native_models_do_not_replace_explicit_models_or_selection_events() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/session/.system_generated/logs/transcript_full.jsonl");
        native_models::write_test_database(
            &dir.path().join("conversations/session.db"),
            &[
                (1, 0, "gemini-3.8-flash-tiered"),
                (3, 1, "gemini-3.8-flash-tiered"),
            ],
        );
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":0,"type":"USER_INPUT","content":"changed setting `Model Selection` from None to Gemini 4 Argon (Medium). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10,"model":"explicit-model"}),
        ];
        assert_eq!(
            import_records(&db, "session", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        let conn = db.conn.lock().unwrap();
        for (id, expected) in [
            ("agy:session:1", "gemini-4-argon-medium"),
            ("agy:session:3", "explicit-model"),
        ] {
            let model: String = conn
                .query_row(
                    "SELECT model FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(model, expected);
        }
    }

    #[test]
    fn native_tiered_ids_share_the_model_selection_bucket_and_price() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/acp/.system_generated/logs/transcript_full.jsonl");
        let native = native_models::write_test_database(
            &dir.path().join("conversations/acp.db"),
            &[(1, 0, "gemini-4-argon"), (2, 1, "gemini-4-argon")],
        );
        native_models::set_test_generation(
            &native,
            0,
            "gemini-4-argon",
            &native_models::test_model_id("gemini-4-argon-medium"),
        );
        // Older builds may store the display name in field 21.
        native_models::set_test_generation(
            &native,
            1,
            "gemini-4-argon",
            &native_models::test_model_id("Gemini 4 Argon (Medium)"),
        );
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1000}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1000}),
        ];
        assert_eq!(
            import_records(&db, "acp", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        let conn = db.conn.lock().unwrap();
        for (id, request_model) in [
            ("agy:acp:1", "gemini-4-argon-medium"),
            ("agy:acp:2", "Gemini 4 Argon (Medium)"),
        ] {
            let got: (String, String, String) = conn
                .query_row(
                    "SELECT model,request_model,total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!((&*got.0, &*got.1), ("gemini-4-argon-medium", request_model));
            assert!(got.2.parse::<f64>().unwrap() > 0.0);
        }
    }

    #[test]
    fn native_flash_tiered_subagent_generations_are_priced_without_losing_provenance() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/browser-subagent/.system_generated/logs/transcript_full.jsonl");
        let _native = native_models::write_test_database(
            &dir.path().join("conversations/browser-subagent.db"),
            &[
                (1, 0, "gemini-3.8-flash-tiered"),
                (3, 1, "gemini-3.7-flash-tiered"),
                (5, 2, "gemini-3.1-flash-tiered"),
            ],
        );
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
            json!({"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        ];
        assert_eq!(
            import_records(&db, "browser-subagent", &rows, Some(&source)).unwrap(),
            (3, 0)
        );
        assert_eq!(
            import_records(&db, "browser-subagent", &rows, Some(&source)).unwrap(),
            (0, 3)
        );
        let conn = db.conn.lock().unwrap();
        for (id, expected_model, expected_cost) in [
            ("agy:browser-subagent:1", "gemini-3.8-flash-tiered", "1.2"),
            ("agy:browser-subagent:3", "gemini-3.7-flash-tiered", "1.2"),
            ("agy:browser-subagent:5", "gemini-3.1-flash-tiered", "0"),
        ] {
            let (model, request_model, cost): (String, String, String) = conn
                .query_row(
                    "SELECT model,request_model,total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(model, expected_model);
            assert_eq!(request_model, expected_model);
            assert_eq!(
                cost.parse::<rust_decimal::Decimal>().unwrap(),
                expected_cost.parse::<rust_decimal::Decimal>().unwrap()
            );
        }
    }

    #[test]
    fn hydrogen_tiered_native_and_explicit_models_are_priced() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/tiered/.system_generated/logs/transcript_full.jsonl");
        let native = native_models::write_test_database(
            &dir.path().join("conversations/tiered.db"),
            &[(1, 0, "gemini-4-hydrogen")],
        );
        native_models::set_test_generation(
            &native,
            0,
            "gemini-4-hydrogen",
            &native_models::test_model_id("gemini-4-hydrogen-tiered-high"),
        );
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-4-hydrogen-tiered-high","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        ];
        assert_eq!(
            import_records(&db, "tiered", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        assert_eq!(
            import_records(&db, "tiered", &rows, Some(&source)).unwrap(),
            (0, 2)
        );
        let conn = db.conn.lock().unwrap();
        for id in ["agy:tiered:1", "agy:tiered:2"] {
            let row: (String, String, String, i64, i64, i64) = conn
                .query_row(
                    "SELECT model, request_model, total_cost_usd, input_tokens, output_tokens,
                        cache_read_tokens FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "gemini-4-hydrogen-tiered-high");
            assert_eq!(row.1, "gemini-4-hydrogen-tiered-high");
            assert_eq!(
                row.2.parse::<rust_decimal::Decimal>().unwrap(),
                "2.325".parse::<rust_decimal::Decimal>().unwrap()
            );
            assert_eq!((row.3, row.4, row.5), (1_000_000, 100_000, 1_000_000));
        }
    }

    #[test]
    fn native_timing_fills_latency_and_first_token_and_backfills_untimed_rows() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/timed/.system_generated/logs/transcript_full.jsonl");
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-4-argon-high","input_tokens":4988,"output_tokens":257}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-4-argon-high","input_tokens":2483,"output_tokens":4145}),
        ];
        let timing = |conn: &rusqlite::Connection, step: u64| -> (i64, Option<i64>) {
            conn.query_row(
                "SELECT latency_ms, first_token_ms FROM proxy_request_logs WHERE request_id=?1",
                [format!("agy:timed:{step}")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        // Imported before the native database existed (or by an older build): no timing.
        assert_eq!(
            import_records(&db, "timed", &rows[..1], Some(&source)).unwrap(),
            (1, 0)
        );
        assert_eq!(timing(&db.conn.lock().unwrap(), 1), (0, None));

        let native = native_models::write_test_database(
            &dir.path().join("conversations/timed.db"),
            &[(1, 0, "gemini-4-argon"), (2, 1, "gemini-4-argon")],
        );
        for (generation, wait, stream) in [(0, 5220, 435), (1, 3100, 41_500)] {
            native_models::set_test_generation(
                &native,
                generation,
                "gemini-4-argon",
                &native_models::test_timing(wait, stream),
            );
        }
        // Step 1 is backfilled, step 2 arrives with its timing; explicit models stay put.
        assert_eq!(
            import_records(&db, "timed", &rows, Some(&source)).unwrap(),
            (2, 0)
        );
        {
            let conn = db.conn.lock().unwrap();
            assert_eq!(timing(&conn, 1), (5655, Some(5220)));
            assert_eq!(timing(&conn, 2), (44_600, Some(3100)));
            let model: String = conn
                .query_row(
                    "SELECT model FROM proxy_request_logs WHERE request_id='agy:timed:2'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(model, "gemini-4-argon-high");
        }
        assert_eq!(
            import_records(&db, "timed", &rows, Some(&source)).unwrap(),
            (0, 2)
        );
        // An unreadable native database never erases stored timing.
        native.execute_batch("DROP TABLE gen_metadata").unwrap();
        let mut rewritten = rows.clone();
        rewritten[0]["output_tokens"] = json!(300);
        assert_eq!(
            import_records(&db, "timed", &rewritten, Some(&source)).unwrap(),
            (1, 1)
        );
        assert_eq!(timing(&db.conn.lock().unwrap(), 1), (5655, Some(5220)));
    }

    #[test]
    fn agy_native_fixture_preserves_fresh_input_and_combined_thinking_output() {
        let db = Database::memory().unwrap();
        let rows = include_str!("../../../tests/fixtures/sessions/agy.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect::<Vec<Value>>();
        assert_eq!(import_records(&db, "fixture", &rows, None).unwrap(), (1, 0));
        let conn = db.conn.lock().unwrap();
        let got=conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,input_token_semantics FROM proxy_request_logs",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?))).unwrap();
        assert_eq!(got, (80, 12, 20, INPUT_TOKEN_SEMANTICS_FRESH));
    }

    #[test]
    fn agy_rewritten_counters_update_retained_rows_without_reimporting_pruned_rows() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10});
        assert_eq!(
            import_records(&db, "s", &[row.clone()], None).unwrap(),
            (1, 0)
        );
        row["input_tokens"] = json!(20);
        assert_eq!(
            import_records(&db, "s", &[row.clone()], None).unwrap(),
            (1, 0)
        );
        assert_eq!(import_records(&db, "s", &[row], None).unwrap(), (0, 1));
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT input_tokens FROM proxy_request_logs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            20
        );
    }

    #[test]
    fn agy_usage_is_exact_deduplicated_and_survives_retention() {
        let db = Database::memory().unwrap();
        let rows = vec![
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":11830,"output_tokens":137,"cache_read_tokens":25}),
        ];
        assert_eq!(import_records(&db, "session", &rows, None).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "session", &rows, None).unwrap(), (0, 1));
        let conn = db.conn.lock().unwrap();
        let got = conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,model FROM proxy_request_logs WHERE app_type='agy'", [], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?, r.get::<_,String>(3)?))).unwrap();
        assert_eq!(got, (11830, 137, 25, "unknown".into()));
        conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        drop(conn);
        assert_eq!(import_records(&db, "session", &rows, None).unwrap(), (0, 1));
        assert_eq!(import_records(&db, "other", &rows, None).unwrap(), (1, 0));
    }
    #[test]
    fn model_selection_change_labels_later_responses() {
        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","status":"DONE","content":"The user changed setting `Model Selection` from None to Gemini 3.8 Flash (High). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10,"output_tokens":5}),
        ];
        assert_eq!(import_records(&db, "models", &rows, None).unwrap(), (1, 0));
        let conn = db.conn.lock().unwrap();
        let model: String = conn
            .query_row(
                "SELECT model FROM proxy_request_logs WHERE app_type='agy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "gemini-3.8-flash-high");
    }

    #[test]
    fn display_names_map_to_priced_model_ids() {
        assert_eq!(
            model_id_of("Gemini 3.8 Flash (High)"),
            "gemini-3.8-flash-high"
        );
        assert_eq!(model_id_of("Gemini 4 Argon (Low)"), "gemini-4-argon-low");
        assert_eq!(
            model_id_of("Claude Opus 4.6 (Thinking)"),
            "claude-opus-4.6-thinking"
        );
        assert_eq!(model_id_of("gemini-3.8-flash"), "gemini-3.8-flash");
        assert_eq!(model_id_of("unknown"), "unknown");

        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","status":"DONE","content":"The user changed setting `Model Selection` from None to Gemini 3.8 Flash (High). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":0}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","model":"Gemini 3.1 Pro (High)","input_tokens":1_000_000}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"Mystery Model","input_tokens":5}),
            json!({"step_index":4,"type":"PLANNER_RESPONSE","status":"DONE","model":"Claude Opus 4.6 (Thinking)","input_tokens":1_000_000}),
        ];
        assert_eq!(import_records(&db, "priced", &rows, None).unwrap(), (4, 0));
        let conn = db.conn.lock().unwrap();
        let price = |model: &str| -> (String, String) {
            conn.query_row(
                "SELECT request_model, total_cost_usd FROM proxy_request_logs WHERE model=?1",
                [model],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let (display, flash) = price("gemini-3.8-flash-high");
        assert_eq!(display, "Gemini 3.8 Flash (High)");
        assert!(flash.parse::<f64>().unwrap() > 0.0, "{flash}");
        let (_, pro) = price("gemini-3.1-pro-high");
        assert!(pro.parse::<f64>().unwrap() > 0.0, "{pro}");
        // Claude Opus 4.6 input is $5 / 1M, the same as the base Opus 4.6 row.
        let (display, opus) = price("claude-opus-4.6-thinking");
        assert_eq!(display, "Claude Opus 4.6 (Thinking)");
        assert_eq!(opus.parse::<f64>().unwrap(), 5.0, "{opus}");
        assert_eq!(price("mystery-model").1, "0");
    }

    #[test]
    fn agy_incomplete_steps_do_not_consume_dedup_ids() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":0,"type":"PLANNER_RESPONSE","status":"RUNNING","input_tokens":10});
        assert_eq!(
            import_records(&db, "s", &[row.clone()], None).unwrap(),
            (0, 0)
        );
        row["status"] = json!("DONE");
        assert_eq!(import_records(&db, "s", &[row], None).unwrap(), (1, 0));
    }

    #[test]
    fn sync_agy_usage_skips_unchanged_transcripts_and_reimports_on_native_or_transcript_update() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let brain = dir.path().join("brain");
        let convs = dir.path().join("conversations");
        std::fs::create_dir_all(&convs).unwrap();
        let logs = brain.join("sess-1/.system_generated/logs");
        std::fs::create_dir_all(&logs).unwrap();
        let transcript = logs.join("transcript_full.jsonl");
        std::fs::write(
            &transcript,
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"input_tokens\":80,\"output_tokens\":12}\n",
        )
        .unwrap();

        let db = Database::memory().unwrap();
        let roots = [(brain.clone(), None)];

        // 1. Initial sync imports step 1 and records a cursor in session_log_sync.
        let r1 = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!((r1.files_scanned, r1.imported, r1.skipped), (1, 1, 0));

        // 2. Steady-state sync with unchanged transcript and native DB skips re-parsing/updating.
        let r2 = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!((r2.files_scanned, r2.imported, r2.skipped), (1, 0, 0));

        // 3. Creating the companion native DB invalidates the cursor and backfills model + timing.
        let native = native_models::write_test_database(
            &convs.join("sess-1.db"),
            &[(1, 0, "gemini-4-argon")],
        );
        native_models::set_test_generation(
            &native,
            0,
            "gemini-4-argon",
            &native_models::test_timing(1200, 800),
        );
        drop(native);

        let r3 = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!((r3.files_scanned, r3.imported, r3.skipped), (1, 1, 0));
        {
            let conn = db.conn.lock().unwrap();
            let got: (String, i64, Option<i64>) = conn
                .query_row(
                    "SELECT model, latency_ms, first_token_ms FROM proxy_request_logs WHERE request_id='agy:sess-1:1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(got, ("gemini-4-argon".into(), 2000, Some(1200)));
        }

        // 4. Subsequent steady-state sync is skipped again.
        let r4 = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!((r4.files_scanned, r4.imported, r4.skipped), (1, 0, 0));

        // 5. Appending a new step to transcript_full.jsonl invalidates the cursor and imports step 2.
        std::fs::write(
            &transcript,
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"input_tokens\":80,\"output_tokens\":12}\n\
             {\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"model\":\"gemini-4-argon-high\",\"input_tokens\":50,\"output_tokens\":25}\n",
        )
        .unwrap();
        let r5 = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!((r5.files_scanned, r5.imported, r5.skipped), (1, 1, 1));
    }

    #[test]
    fn sync_agy_usage_imports_subagent_transcripts_excluded_from_session_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("brain");
        for (id, lines) in [
            (
                "main-1",
                concat!(
                    "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"<USER_REQUEST>\\nhello\\n</USER_REQUEST>\"}\n",
                    "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"input_tokens\":40,\"output_tokens\":10}\n",
                ),
            ),
            (
                "subagent-1",
                concat!(
                    "{\"step_index\":0,\"type\":\"SYSTEM_MESSAGE\",\"content\":\"subagent task\"}\n",
                    "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"input_tokens\":120,\"output_tokens\":30}\n",
                ),
            ),
        ] {
            let logs = root.join(id).join(".system_generated/logs");
            std::fs::create_dir_all(&logs).unwrap();
            std::fs::write(logs.join("transcript_full.jsonl"), lines).unwrap();
        }
        let db = Database::memory().unwrap();
        let files = agy::transcript_files_in_roots(&[(root, None)]);
        let res = sync_agy_usage_from_files(&db, files).unwrap();
        assert_eq!(res.files_scanned, 2);
        assert_eq!(res.imported, 2);
        assert!(res.errors.is_empty());
        let conn = db.conn.lock().unwrap();
        let total_input: i64 = conn
            .query_row(
                "SELECT SUM(input_tokens) FROM proxy_request_logs WHERE app_type='agy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total_input, 160);
    }

    #[test]
    fn gemini_root_fixture_scans_sessions_and_imports_usage() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::agy_config::agy_paths(dir.path());
        let logs = paths.sessions.join("conv-1").join(".system_generated/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("transcript_full.jsonl"),
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>hello from wsl</USER_REQUEST>","created_at":"2026-11-01T00:00:00Z"}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":12,"output_tokens":3,"created_at":"2026-11-01T00:00:01Z"}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(paths.settings.parent().unwrap()).unwrap();
        std::fs::write(&paths.settings, r#"{"model":"Gemini 3.8 Flash (High)"}"#).unwrap();
        std::fs::create_dir_all(paths.mcp.parent().unwrap()).unwrap();
        std::fs::write(&paths.mcp, r#"{"mcpServers":{}}"#).unwrap();
        std::fs::create_dir_all(&paths.skills).unwrap();
        std::fs::write(&paths.prompts, "agents\n").unwrap();
        let summaries = rusqlite::Connection::open(&paths.summaries).unwrap();
        summaries
            .execute_batch(
                "CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT);",
            )
            .unwrap();
        summaries
            .execute(
                "INSERT INTO conversation_summaries VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![
                    "conv-1",
                    "WSL chat",
                    "[]",
                    "hello from wsl",
                    "2026-11-01T00:00:02Z"
                ],
            )
            .unwrap();
        drop(summaries);

        let sessions = agy::scan_gemini_root(dir.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "conv-1");
        assert_eq!(sessions[0].title.as_deref(), Some("WSL chat"));
        let source = sessions[0].source_path.clone().expect("transcript");
        assert!(source.contains("antigravity-cli"));
        assert!(paths.settings.is_file());
        assert!(paths.mcp.is_file());
        assert!(paths.prompts.is_file());
        assert!(paths.skills.is_dir());

        let rows = agy::records(std::path::Path::new(&source)).unwrap();
        let db = Database::memory().unwrap();
        assert_eq!(
            import_records(&db, "conv-1", &rows, Some(std::path::Path::new(&source))).unwrap(),
            (1, 0)
        );
        let got: (i64, i64) = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT input_tokens, output_tokens FROM proxy_request_logs WHERE request_id='agy:conv-1:1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(got, (12, 3));
    }

    fn transcript(root: &std::path::Path, id: &str) -> std::path::PathBuf {
        root.join("brain")
            .join(id)
            .join(".system_generated/logs/transcript_full.jsonl")
    }

    #[test]
    fn child_transcript_without_a_model_uses_its_conversation_db_and_updates_unknown_rows() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = transcript(dir.path(), "child");
        let rows = [json!({
            "step_index": 1,
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": "2026-11-11T00:00:02Z",
            "input_tokens": 80,
            "output_tokens": 12
        })];
        let db = Database::memory().unwrap();
        assert_eq!(
            import_records(&db, "child", &rows, Some(&source)).unwrap(),
            (1, 0)
        );
        let model = |db: &Database| -> (String, i64, Option<i64>) {
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT model, latency_ms, first_token_ms FROM proxy_request_logs WHERE request_id='agy:child:1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap()
        };
        assert_eq!(model(&db).0, "unknown");

        std::fs::create_dir_all(dir.path().join("conversations")).unwrap();
        let native = native_models::write_test_database(
            &dir.path().join("conversations/child.db"),
            &[(1, 0, "gemini-4-argon")],
        );
        native_models::set_test_generation(
            &native,
            0,
            "gemini-4-argon",
            &native_models::test_timing(1200, 800),
        );
        assert_eq!(
            import_records(&db, "child", &rows, Some(&source)).unwrap(),
            (1, 0)
        );
        assert_eq!(model(&db), ("gemini-4-argon".into(), 2000, Some(1200)));

        // A parent model must not replace the child's own database model.
        std::fs::create_dir_all(transcript(dir.path(), "parent").parent().unwrap()).unwrap();
        std::fs::write(
            transcript(dir.path(), "parent"),
            r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Gemini 3.8 Flash (High)."}"#,
        )
        .unwrap();
        let summaries =
            rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        summaries
            .execute_batch(
                "CREATE TABLE conversation_summaries(conversation_id TEXT, parent_conversation_id TEXT);
                 INSERT INTO conversation_summaries VALUES ('child','parent');",
            )
            .unwrap();
        assert_eq!(
            import_records(&db, "child", &rows, Some(&source)).unwrap(),
            (0, 1)
        );
        assert_eq!(model(&db).0, "gemini-4-argon");
    }

    #[test]
    fn child_without_a_db_model_inherits_the_parent_model_from_before_it_started() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let parent = transcript(dir.path(), "parent");
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        std::fs::write(
            &parent,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Gemini 4 Argon (High)."}"#,
                "\n",
                r#"{"step_index":2,"type":"USER_INPUT","created_at":"2026-11-11T00:00:03Z","content":"changed setting `Model Selection` from Gemini 4 Argon (High) to Gemini 4 Argon (Low)."}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(transcript(dir.path(), "db-parent").parent().unwrap()).unwrap();
        std::fs::write(
            transcript(dir.path(), "db-parent"),
            concat!(
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T00:00:01Z","input_tokens":1,"output_tokens":1}"#,
                "\n",
                r#"{"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T00:00:03Z","input_tokens":1,"output_tokens":1}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("conversations")).unwrap();
        native_models::write_test_database(
            &dir.path().join("conversations/db-parent.db"),
            &[(1, 0, "gemini-4-argon"), (2, 1, "gemini-3.8-flash-tiered")],
        );

        let at = "2026-11-11T00:00:02Z";
        let row = |step| {
            json!({
                "step_index": step,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "created_at": at,
                "input_tokens": 10,
                "output_tokens": 4
            })
        };
        let db = Database::memory().unwrap();
        let summary = transcript(dir.path(), "from-summary");
        assert_eq!(
            import_records(&db, "from-summary", &[row(1)], Some(&summary)).unwrap(),
            (1, 0)
        );
        let stored = |db: &Database, id: &str| -> String {
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT model FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(stored(&db, "agy:from-summary:1"), "unknown");

        let summaries =
            rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        summaries
            .execute_batch(
                "CREATE TABLE conversation_summaries(conversation_id TEXT, parent_conversation_id TEXT);
                 INSERT INTO conversation_summaries VALUES ('from-summary','parent');
                 INSERT INTO conversation_summaries VALUES ('too-early','parent');
                 INSERT INTO conversation_summaries VALUES ('from-db','db-parent');
                 INSERT INTO conversation_summaries VALUES ('via-invoke', NULL);",
            )
            .unwrap();
        std::fs::write(
            &parent,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Gemini 4 Argon (High)."}"#,
                "\n",
                r#"{"step_index":1,"type":"INVOKE_SUBAGENT","created_at":"2026-11-11T00:00:01Z","content":"started {\"conversationId\":\"via-invoke\",\"logAbsoluteUri\":\"file:///home/alice/.gemini/antigravity-cli/brain/via-invoke/.system_generated/logs/transcript_full.jsonl\"}"}"#,
                "\n",
                r#"{"step_index":2,"type":"USER_INPUT","created_at":"2026-11-11T00:00:03Z","content":"changed setting `Model Selection` from Gemini 4 Argon (High) to Gemini 4 Argon (Low)."}"#,
                "\n",
            ),
        )
        .unwrap();

        assert_eq!(
            import_records(&db, "from-summary", &[row(1)], Some(&summary)).unwrap(),
            (1, 0)
        );
        assert_eq!(stored(&db, "agy:from-summary:1"), "gemini-4-argon-high");

        let early = transcript(dir.path(), "too-early");
        let early_row = json!({
            "step_index": 1,
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": "2026-11-11T00:00:00Z",
            "input_tokens": 10,
            "output_tokens": 4
        });
        import_records(&db, "too-early", &[early_row], Some(&early)).unwrap();
        assert_eq!(stored(&db, "agy:too-early:1"), "unknown");

        let via = transcript(dir.path(), "via-invoke");
        import_records(&db, "via-invoke", &[row(1)], Some(&via)).unwrap();
        assert_eq!(stored(&db, "agy:via-invoke:1"), "gemini-4-argon-high");

        let from_db = transcript(dir.path(), "from-db");
        import_records(&db, "from-db", &[row(1)], Some(&from_db)).unwrap();
        assert_eq!(stored(&db, "agy:from-db:1"), "gemini-4-argon");
    }

    #[test]
    fn child_does_not_inherit_orchestrator_or_worker_mode_selector_from_parent() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mode_only = transcript(dir.path(), "mode-only");
        let worker_only = transcript(dir.path(), "worker-only");
        let switched = transcript(dir.path(), "switched");
        let with_db = transcript(dir.path(), "with-db");
        for p in [&mode_only, &worker_only, &switched, &with_db] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        std::fs::write(
            &mode_only,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Orchestrator (Carbon)."}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            &worker_only,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to gemini-flash-worker-agy."}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            &switched,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Gemini 4 Argon (High)."}"#,
                "\n",
                r#"{"step_index":1,"type":"USER_INPUT","created_at":"2026-11-11T00:00:02Z","content":"changed setting `Model Selection` from Gemini 4 Argon (High) to Orchestrator (Argon)."}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            &with_db,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Orchestrator (Carbon)."}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T00:00:01Z","input_tokens":1,"output_tokens":1}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("conversations")).unwrap();
        let native = native_models::write_test_database(
            &dir.path().join("conversations/with-db.db"),
            &[(1, 0, "gemini-pro-orchestrator")],
        );
        native_models::set_test_generation(
            &native,
            0,
            "gemini-pro-orchestrator",
            &native_models::test_model_id("Orchestrator (Carbon)"),
        );
        drop(native);

        let summaries =
            rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        summaries
            .execute_batch(
                "CREATE TABLE conversation_summaries(conversation_id TEXT, parent_conversation_id TEXT);
                 INSERT INTO conversation_summaries VALUES ('c-mode','mode-only');
                 INSERT INTO conversation_summaries VALUES ('c-worker','worker-only');
                 INSERT INTO conversation_summaries VALUES ('c-switch','switched');
                 INSERT INTO conversation_summaries VALUES ('c-db','with-db');",
            )
            .unwrap();
        drop(summaries);

        let db = Database::memory().unwrap();
        let child_row = [json!({
            "step_index": 1,
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": "2026-11-11T00:00:03Z",
            "input_tokens": 10,
            "output_tokens": 4
        })];
        let stored = |id: &str| -> String {
            db.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT model FROM proxy_request_logs WHERE request_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap()
        };

        import_records(
            &db,
            "c-mode",
            &child_row,
            Some(&transcript(dir.path(), "c-mode")),
        )
        .unwrap();
        assert_eq!(stored("agy:c-mode:1"), "unknown");

        import_records(
            &db,
            "c-worker",
            &child_row,
            Some(&transcript(dir.path(), "c-worker")),
        )
        .unwrap();
        assert_eq!(stored("agy:c-worker:1"), "unknown");

        import_records(
            &db,
            "c-switch",
            &child_row,
            Some(&transcript(dir.path(), "c-switch")),
        )
        .unwrap();
        assert_eq!(stored("agy:c-switch:1"), "unknown");

        import_records(
            &db,
            "c-db",
            &child_row,
            Some(&transcript(dir.path(), "c-db")),
        )
        .unwrap();
        assert_eq!(stored("agy:c-db:1"), "gemini-4-carbon");
    }

    #[test]
    fn sync_agy_usage_v5_cursor_backfills_unknown_subagent_from_v4_cursor() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let parent = transcript(dir.path(), "parent-p");
        let child = transcript(dir.path(), "child-c");
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        std::fs::create_dir_all(child.parent().unwrap()).unwrap();
        std::fs::write(
            &parent,
            concat!(
                r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-11-11T00:00:01Z","content":"changed setting `Model Selection` from None to Gemini 4 Neon (High)."}"#,
                "\n",
                r#"{"step_index":1,"type":"INVOKE_SUBAGENT","created_at":"2026-11-11T00:00:01Z","content":"started {\"conversationId\":\"child-c\",\"logAbsoluteUri\":\"file:///home/alice/.gemini/antigravity-cli/brain/child-c/.system_generated/logs/transcript_full.jsonl\"}"}"#,
                "\n",
            ),
        )
        .unwrap();
        std::fs::write(
            &child,
            r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T00:00:02Z","input_tokens":40,"output_tokens":10}"#,
        )
        .unwrap();

        let db = Database::memory().unwrap();
        let child_row = [json!({
            "step_index": 1,
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": "2026-11-11T00:00:02Z",
            "input_tokens": 40,
            "output_tokens": 10
        })];
        // Seed an earlier v4: import where child landed as `unknown`.
        import_records(&db, "child-c", &child_row, None).unwrap();
        let meta = std::fs::metadata(&child).unwrap();
        let modified = metadata_modified_nanos(&meta);
        let size = meta.len() as i64;
        let key = format!("agy:{}", child.to_string_lossy());
        let old_state = format!("v4:{}", native_models::native_state_fingerprint(&child));
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO session_log_sync
                     (file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset, parser_state)
                 VALUES (?1, ?2, 0, unixepoch(), ?3, ?4)",
                params![key, modified, size, old_state],
            )
            .unwrap();
        }

        let roots = [(dir.path().join("brain"), None)];
        let res = sync_agy_usage_from_files(&db, agy::transcript_files_in_roots(&roots)).unwrap();
        assert_eq!(res.imported, 1);
        let conn = db.conn.lock().unwrap();
        let model: String = conn
            .query_row(
                "SELECT model FROM proxy_request_logs WHERE request_id='agy:child-c:1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "gemini-4-neon-high");
        let parser_state: String = conn
            .query_row(
                "SELECT parser_state FROM session_log_sync WHERE file_path=?1",
                [&key],
                |r| r.get(0),
            )
            .unwrap();
        assert!(parser_state.starts_with("v5:"), "{parser_state}");
    }
}
