//! Exact counters exported by agy; absent models remain unknown (never use today's provider).
//! Amounts are the matching Gemini API prices, like every other session-log import.
use super::{session_usage::SessionSyncResult, sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH};
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    session_manager::providers::agy,
};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::path::Path;

mod native_models;

pub fn sync_agy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    for session in agy::scan_sessions() {
        let Some(path) = session.source_path else {
            continue;
        };
        result.files_scanned += 1;
        match agy::records(std::path::Path::new(&path)) {
            Ok(rows) => {
                match import_records(db, &session.session_id, &rows, Some(Path::new(&path))) {
                    Ok((imported, skipped)) => {
                        result.imported += imported;
                        result.skipped += skipped;
                    }
                    Err(e) => result
                        .errors
                        .push(format!("Antigravity {}: {e}", session.session_id)),
                }
            }
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

/// agy 把模型切换写进会话正文：`changed setting \`Model Selection\` from X to Y`。
/// 逐条响应本身没有模型字段，按 step 顺序继承最近一次切换即可还原当时的模型。
fn model_from_settings_change(content: &str) -> Option<String> {
    let start = content.find("Model Selection")?;
    let rest = &content[start..];
    let to = rest.find(" to ")? + 4;
    let tail = &rest[to..];
    // 模型名里本身有点（Gemini 3.8），只把「句号 + 空格」当成句子结束。
    let end = tail
        .find(". ")
        .or_else(|| tail.find('\n'))
        .or_else(|| tail.find('`'))
        .unwrap_or(tail.len());
    let model = tail[..end].trim().trim_matches('.').trim();
    if model.is_empty() || model.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(model.to_string())
    }
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

/// 五个金额（输入、输出、缓存读、缓存写、合计）；查不到价格的记 0。
fn costs(conn: &rusqlite::Connection, model: &str, tokens: [i64; 4]) -> [String; 5] {
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
    match crate::services::usage_stats::find_model_pricing(conn, model) {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app(
                "agy",
                &usage,
                &pricing,
                rust_decimal::Decimal::from(1),
            );
            [
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            ]
        }
        None => std::array::from_fn(|_| "0".to_string()),
    }
}

fn import_records(
    db: &Database,
    session: &str,
    rows: &[Value],
    transcript: Option<&Path>,
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

    // 子代理通常没有 Model Selection；按步骤关联原生生成记录，不推测档位映射。
    if let Some(transcript) = transcript {
        let missing: Vec<u64> = ordered
            .iter()
            .filter(|row| row["type"] == "PLANNER_RESPONSE" && row["status"] == "DONE")
            .filter(|row| known_model(row).is_none())
            .filter_map(|row| row["step_index"].as_u64())
            .filter(|step| !model_by_step.contains_key(step))
            .collect();
        if !missing.is_empty() {
            match native_models::models_for_transcript(transcript, &missing) {
                Ok(models) => model_by_step.extend(models),
                Err(error) => {
                    log::debug!("Antigravity {session}: cannot read native models: {error}")
                }
            }
        }
    }

    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
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
        let fresh = tx.execute("INSERT OR IGNORE INTO session_usage_dedup(data_source,request_id,semantic_id,has_entry_id) VALUES ('agy_session',?1,?1,1)", [&id])?;
        // `model` 存 ID（对得上定价和其它应用的统计），`request_model` 留显示名。
        let display = known_model(row)
            .map(str::to_string)
            .or_else(|| model_by_step.get(&step).cloned())
            .unwrap_or_else(|| "unknown".to_string());
        let model = model_id_of(&display);
        // 原生库暂时不可读时，保留已恢复的模型；Token 仍按最新日志更新。
        let (model, display) = if model == "unknown" && fresh == 0 {
            tx.query_row(
                "SELECT model, COALESCE(request_model, model) FROM proxy_request_logs
                 WHERE request_id=?1 AND model!='unknown'",
                [&id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .unwrap_or((model, display))
        } else {
            (model, display)
        };
        let [input_cost, output_cost, cache_cost, write_cost, total_cost] =
            costs(&tx, &model, [input, output, cache, write]);
        if fresh == 0 {
            // A rewritten completed step may correct counters; update retained detail only.
            // If retention already removed it, its tombstone still forbids resurrection.
            let changed = tx.execute("UPDATE proxy_request_logs SET model=?2,request_model=?7,input_tokens=?3,output_tokens=?4,cache_read_tokens=?5,cache_creation_tokens=?6,input_cost_usd=?8,output_cost_usd=?9,cache_read_cost_usd=?10,cache_creation_cost_usd=?11,total_cost_usd=?12 WHERE request_id=?1 AND (model!=?2 OR request_model!=?7 OR input_tokens!=?3 OR output_tokens!=?4 OR cache_read_tokens!=?5 OR cache_creation_tokens!=?6 OR total_cost_usd!=?12)", params![id,model,input,output,cache,write,display,input_cost,output_cost,cache_cost,write_cost,total_cost])?;
            imported += changed as u32;
            skipped += u32::from(changed == 0);
            continue;
        }
        // Native agy was verified with a local mock: input excludes cache, output includes
        // thinking. Never subtract cache or add thinking a second time. Missing historical
        // models remain unknown and unpriced; never fall back to the active provider.
        tx.execute("INSERT INTO proxy_request_logs (
            request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
            cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,
            cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,latency_ms,status_code,
            session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,input_token_semantics
        ) VALUES (?1,'_agy_session','agy',?2,?10,?3,?4,?5,?6,?11,?12,?13,?14,?15,0,200,?7,'agy_session',1,'1',?8,'agy_session',?9)",
        params![id, model, input, output, cache, write, session, agy::timestamp(row).unwrap_or(0)/1000, INPUT_TOKEN_SEMANTICS_FRESH, display, input_cost, output_cost, cache_cost, write_cost, total_cost])?;
        imported += 1;
    }
    tx.commit()?;
    Ok((imported, skipped))
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
            assert_eq!(flash, ("gemini-3.8-flash-tiered".into(), "0".into()));
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
        assert_eq!(model_id_of("gemini-3.8-flash"), "gemini-3.8-flash");
        assert_eq!(model_id_of("unknown"), "unknown");

        let db = Database::memory().unwrap();
        let rows = [
            json!({"step_index":0,"type":"USER_INPUT","status":"DONE","content":"The user changed setting `Model Selection` from None to Gemini 3.8 Flash (High). No need to comment"}),
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":0}),
            json!({"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE","model":"Gemini 3.1 Pro (High)","input_tokens":1_000_000}),
            json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"Mystery Model","input_tokens":5}),
        ];
        assert_eq!(import_records(&db, "priced", &rows, None).unwrap(), (3, 0));
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
}
