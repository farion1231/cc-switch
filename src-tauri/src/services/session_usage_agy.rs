//! Exact counters exported by agy; absent models remain unknown (never use today's provider).
use super::{session_usage::SessionSyncResult, sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH};
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    session_manager::providers::agy,
};
use rusqlite::params;
use serde_json::Value;

pub fn sync_agy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    for session in agy::scan_sessions() {
        let Some(path) = session.source_path else {
            continue;
        };
        result.files_scanned += 1;
        match agy::records(std::path::Path::new(&path)) {
            Ok(rows) => match import_records(db, &session.session_id, &rows) {
                Ok((imported, skipped)) => {
                    result.imported += imported;
                    result.skipped += skipped;
                }
                Err(e) => result
                    .errors
                    .push(format!("Antigravity {}: {e}", session.session_id)),
            },
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

fn import_records(db: &Database, session: &str, rows: &[Value]) -> Result<(u32, u32), AppError> {
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
        let model = row["model"].as_str().unwrap_or("unknown");
        if fresh == 0 {
            // A rewritten completed step may correct counters; update retained detail only.
            // If retention already removed it, its tombstone still forbids resurrection.
            let changed = tx.execute("UPDATE proxy_request_logs SET model=?2,request_model=?2,input_tokens=?3,output_tokens=?4,cache_read_tokens=?5,cache_creation_tokens=?6 WHERE request_id=?1 AND (model!=?2 OR input_tokens!=?3 OR output_tokens!=?4 OR cache_read_tokens!=?5 OR cache_creation_tokens!=?6)", params![id,model,input,output,cache,write])?;
            imported += changed as u32;
            skipped += u32::from(changed == 0);
            continue;
        }
        // Native agy was verified with a local mock: input excludes cache, output includes
        // thinking. Never subtract cache or add thinking a second time. Missing historical
        // models remain unknown; do not manufacture billing or use the active provider.
        tx.execute("INSERT INTO proxy_request_logs (
            request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
            cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,
            cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,latency_ms,status_code,
            session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,input_token_semantics
        ) VALUES (?1,'_agy_session','agy',?2,?2,?3,?4,?5,?6,'0','0','0','0','0',0,200,?7,'agy_session',1,'1',?8,'agy_session',?9)",
        params![id, model, input, output, cache, write, session, agy::timestamp(row).unwrap_or(0)/1000, INPUT_TOKEN_SEMANTICS_FRESH])?;
        imported += 1;
    }
    tx.commit()?;
    Ok((imported, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn agy_native_fixture_preserves_fresh_input_and_combined_thinking_output() {
        let db = Database::memory().unwrap();
        let rows = include_str!("../../../tests/fixtures/sessions/agy.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect::<Vec<Value>>();
        assert_eq!(import_records(&db, "fixture", &rows).unwrap(), (1, 0));
        let conn = db.conn.lock().unwrap();
        let got=conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,input_token_semantics FROM proxy_request_logs",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?))).unwrap();
        assert_eq!(got, (80, 12, 20, INPUT_TOKEN_SEMANTICS_FRESH));
    }

    #[test]
    fn agy_rewritten_counters_update_retained_rows_without_reimporting_pruned_rows() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":10});
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        row["input_tokens"] = json!(20);
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (0, 1));
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
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        let conn = db.conn.lock().unwrap();
        let got = conn.query_row("SELECT input_tokens,output_tokens,cache_read_tokens,model FROM proxy_request_logs WHERE app_type='agy'", [], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?, r.get::<_,String>(3)?))).unwrap();
        assert_eq!(got, (11830, 137, 25, "unknown".into()));
        conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        drop(conn);
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        assert_eq!(import_records(&db, "other", &rows).unwrap(), (1, 0));
    }
    #[test]
    fn agy_incomplete_steps_do_not_consume_dedup_ids() {
        let db = Database::memory().unwrap();
        let mut row =
            json!({"step_index":0,"type":"PLANNER_RESPONSE","status":"RUNNING","input_tokens":10});
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (0, 0));
        row["status"] = json!("DONE");
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (1, 0));
    }
}
