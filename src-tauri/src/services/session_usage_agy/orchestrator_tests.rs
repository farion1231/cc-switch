use super::*;
use serde_json::json;

fn rows(mode: &str) -> Vec<Value> {
    vec![
        json!({"step_index":0,"type":"USER_INPUT","content":format!("changed setting `Model Selection` from None to {mode}. No need to comment")}),
        json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":mode,"input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ]
}

fn native(path: &Path, planner: &str, mode: &str) -> rusqlite::Connection {
    let conn = native_models::write_test_database(
        path,
        &[
            (1, 0, planner),
            (3, 7, "gemini-flash-worker-agy"),
            (5, 2, "gemini-flash-worker-agy"),
        ],
    );
    for (index, base, display) in [
        (0, planner, mode),
        (7, "gemini-flash-worker-agy", "Gemini Flash"),
    ] {
        let mut fields = native_models::test_model_id(display);
        fields.extend(native_models::test_timing(1200, 800));
        native_models::set_test_generation(&conn, index, base, &fields);
    }
    conn
}

fn detail(db: &Database, step: u64) -> (String, String, String) {
    db.conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT model,request_model,total_cost_usd FROM proxy_request_logs WHERE request_id=?1",
            [format!("agy:session:{step}")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

fn assert_price(db: &Database, step: u64, model: &str, request: &str, cost: &str) {
    let got = detail(db, step);
    assert_eq!((&*got.0, &*got.1), (model, request));
    assert_eq!(
        got.2.parse::<rust_decimal::Decimal>().unwrap(),
        cost.parse::<rust_decimal::Decimal>().unwrap()
    );
}

#[test]
fn orchestrator_splits_native_generations_and_preserves_request_mode() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    for (mode, planner, expected) in [
        (
            "Orchestrator (Carbon)",
            "gemini-pro-orchestrator",
            "gemini-4-carbon",
        ),
        (
            "Orchestrator (Argon)",
            "gemini-pro-orchestrator-argon",
            "gemini-4-argon",
        ),
        (
            "gemini-pro-orchestrator",
            "gemini-pro-orchestrator",
            "gemini-4-carbon",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("conversations")).unwrap();
        let source = dir
            .path()
            .join("brain/session/.system_generated/logs/transcript_full.jsonl");
        let _native = native(&dir.path().join("conversations/session.db"), planner, mode);
        let db = Database::memory().unwrap();
        let mut rows = rows(mode);
        // A concrete per-response model still wins over both mode and native metadata.
        rows.push(json!({"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-4-carbon-high","input_tokens":1_000_000}));
        assert_eq!(
            import_records(&db, "session", &rows, Some(&source)).unwrap(),
            (3, 0)
        );
        assert_price(&db, 1, expected, mode, "3.1");
        assert_price(&db, 3, "gemini-4-hydrogen", mode, "2.325");
        assert_price(&db, 5, "gemini-4-carbon-high", "gemini-4-carbon-high", "2");
        assert_eq!(
            import_records(&db, "session", &rows, Some(&source)).unwrap(),
            (0, 3)
        );
        let counts: (i64, i64, i64, i64) = db.conn.lock().unwrap().query_row(
            "SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(cache_read_tokens) FROM proxy_request_logs",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).unwrap();
        assert_eq!(counts, (3, 3_000_000, 200_000, 2_000_000));
        db.conn.lock().unwrap().execute(
            "UPDATE model_pricing SET input_cost_per_million='4' WHERE model_id='gemini-4-hydrogen'",
            [],
        ).unwrap();
        assert_eq!(
            import_records(&db, "session", &rows, Some(&source)).unwrap(),
            (1, 2)
        );
        assert_price(&db, 3, "gemini-4-hydrogen", mode, "4.825");
    }
}

#[test]
fn orchestrator_requires_native_evidence_and_keeps_recovered_models_on_read_failure() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    let db = Database::memory().unwrap();
    let mode = "Orchestrator (Carbon)";
    let mut rows = rows(mode);
    import_records(&db, "session", &rows, Some(&source)).unwrap();
    assert_price(&db, 1, "orchestrator-carbon", mode, "0");
    assert_price(&db, 3, "orchestrator-carbon", mode, "0");
    let native = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        mode,
    );
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (2, 0)
    );
    assert_price(&db, 1, "gemini-4-carbon", mode, "3.1");
    assert_price(&db, 3, "gemini-4-hydrogen", mode, "2.325");

    native.execute_batch("DROP TABLE gen_metadata").unwrap();
    rows[1]["input_tokens"] = json!(2_000_000);
    // Same Carbon mode, now spelled as the native alias: the recovered model must survive.
    rows[2]["model"] = json!("gemini-pro-orchestrator");
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (1, 1)
    );
    assert_price(&db, 1, "gemini-4-carbon", mode, "5.1");
    assert_price(&db, 3, "gemini-4-hydrogen", mode, "2.325");
    db.conn
        .lock()
        .unwrap()
        .execute("DELETE FROM proxy_request_logs", [])
        .unwrap();
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (0, 2)
    );
    let count: i64 = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn orchestrator_backfills_timed_history_after_parser_upgrade_once() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let mode = "Orchestrator (Carbon)";
    let rows = rows(mode);
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&source, text).unwrap();
    let native = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        mode,
    );
    drop(native);
    let db = Database::memory().unwrap();
    let files = || vec![("session".into(), source.clone())];
    sync_agy_usage_from_files(&db, files()).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        // Simulate v4.0.31: valid timing, unchanged files, legacy parser fingerprint.
        conn.execute("UPDATE proxy_request_logs SET model='orchestrator-carbon',input_cost_usd='0',output_cost_usd='0',cache_read_cost_usd='0',total_cost_usd='0'", []).unwrap();
        conn.execute(
            "UPDATE session_log_sync SET parser_state=?1",
            [native_models::native_state_fingerprint(&source)],
        )
        .unwrap();
    }
    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!((result.imported, result.skipped), (2, 0));
    assert_price(&db, 1, "gemini-4-carbon", mode, "3.1");
    assert_price(&db, 3, "gemini-4-hydrogen", mode, "2.325");
    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!((result.imported, result.skipped), (0, 0));

    // Fully pruned history with a broken native db must not keep the cursor unsettled.
    rusqlite::Connection::open(dir.path().join("conversations/session.db"))
        .unwrap()
        .execute_batch("DROP TABLE gen_metadata")
        .unwrap();
    db.conn
        .lock()
        .unwrap()
        .execute_batch("DELETE FROM proxy_request_logs; DELETE FROM session_log_sync;")
        .unwrap();
    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!(
        (result.imported, result.skipped, result.errors.len()),
        (0, 2, 0)
    );
    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!(
        (result.imported, result.skipped, result.errors.len()),
        (0, 0, 0)
    );
}
