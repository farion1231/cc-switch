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
fn hydrogen_orchestrator_native_display_is_priced_without_losing_provenance() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    let mode = "Orchestrator (Carbon)";
    let native = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        mode,
    );
    native_models::set_test_generation(
        &native,
        7,
        "gemini-flash-worker-agy",
        &native_models::test_model_id("Gemini 4 Hydrogen (Orchestrator)"),
    );
    let db = Database::memory().unwrap();
    let rows = rows(mode);
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (2, 0)
    );
    assert_price(&db, 1, "gemini-4-carbon", mode, "3.1");
    assert_price(&db, 3, "gemini-4-hydrogen-orchestrator", mode, "2.325");
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (0, 2)
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

#[test]
fn cli_orchestrator_worker_alias_attributes_to_hydrogen_with_or_without_native_db() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    let cli_rows = vec![
        json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-pro-orchestrator","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];

    // Without a native database, the planner alias stays unresolved while the worker alias
    // already identifies Hydrogen and uses its base price.
    let db_no_native = Database::memory().unwrap();
    assert_eq!(
        import_records(&db_no_native, "session", &cli_rows, None).unwrap(),
        (3, 0)
    );
    assert_price(
        &db_no_native,
        1,
        "gemini-pro-orchestrator",
        "gemini-pro-orchestrator",
        "0",
    );
    assert_price(
        &db_no_native,
        3,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "2.325",
    );
    assert_price(
        &db_no_native,
        5,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "2.325",
    );

    // With native `gen_metadata`, field-21 Hydrogen provenance and concrete tiers win, and
    // survive a later unreadable native database without reverting to bare `gemini-4-hydrogen`.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    let native = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        "Orchestrator (Carbon)",
    );
    native_models::set_test_generation(
        &native,
        7,
        "gemini-flash-worker-agy",
        &native_models::test_model_id("Gemini 4 Hydrogen (Orchestrator)"),
    );
    native_models::set_test_generation(
        &native,
        2,
        "gemini-flash-worker-agy",
        &native_models::test_model_id("gemini-4-hydrogen-high"),
    );

    let db = Database::memory().unwrap();
    let mut rows = cli_rows;
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (3, 0)
    );
    assert_price(&db, 1, "gemini-4-carbon", "gemini-pro-orchestrator", "3.1");
    assert_price(
        &db,
        3,
        "gemini-4-hydrogen-orchestrator",
        "gemini-flash-worker-agy",
        "2.325",
    );
    assert_price(
        &db,
        5,
        "gemini-4-hydrogen-high",
        "gemini-flash-worker-agy",
        "2.325",
    );

    native.execute_batch("DROP TABLE gen_metadata").unwrap();
    rows[1]["input_tokens"] = json!(2_000_000);
    rows[2]["input_tokens"] = json!(2_000_000);
    assert_eq!(
        import_records(&db, "session", &rows, Some(&source)).unwrap(),
        (2, 1)
    );
    assert_price(
        &db,
        3,
        "gemini-4-hydrogen-orchestrator",
        "gemini-flash-worker-agy",
        "3.825",
    );
    assert_price(
        &db,
        5,
        "gemini-4-hydrogen-high",
        "gemini-flash-worker-agy",
        "3.825",
    );
}

#[test]
fn cli_orchestrator_worker_history_reattributes_after_v2_cursor_upgrade_once() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let rows = [
        json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-pro-orchestrator","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&source, text).unwrap();

    let native = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        "Orchestrator (Carbon)",
    );
    let mut worker_fields = native_models::test_model_id("Gemini 4 Hydrogen (Orchestrator)");
    worker_fields.extend(native_models::test_timing(1200, 800));
    native_models::set_test_generation(&native, 7, "gemini-flash-worker-agy", &worker_fields);
    drop(native);

    let db = Database::memory().unwrap();
    let files = || vec![("session".into(), source.clone())];
    sync_agy_usage_from_files(&db, files()).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        // Simulate v4.0.32–v4.0.34: settled timing, unchanged files, `v2:` cursor, unpriced worker row.
        conn.execute(
            "UPDATE proxy_request_logs
             SET model='gemini-flash-worker-agy',
                 request_model='gemini-flash-worker-agy',
                 input_cost_usd='0',
                 output_cost_usd='0',
                 cache_read_cost_usd='0',
                 total_cost_usd='0'
             WHERE request_id='agy:session:3'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE session_log_sync SET parser_state=?1",
            [format!(
                "v2:{}",
                native_models::native_state_fingerprint(&source)
            )],
        )
        .unwrap();
    }

    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!((result.imported, result.skipped), (1, 1));
    assert_price(&db, 1, "gemini-4-carbon", "gemini-pro-orchestrator", "3.1");
    assert_price(
        &db,
        3,
        "gemini-4-hydrogen-orchestrator",
        "gemini-flash-worker-agy",
        "2.325",
    );
    let result = sync_agy_usage_from_files(&db, files()).unwrap();
    assert_eq!((result.imported, result.skipped), (0, 0));
}

#[test]
fn cli_worker_ignores_generic_native_labels_when_field19_is_empty() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    // Native `gen_metadata` with an empty field-19 base and generic/mode field-21 labels
    // (`Orchestrator (Carbon)` or `Gemini Flash`) must not downgrade a CLI worker step to $0.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    let native_conn = native_models::write_test_database(
        &dir.path().join("conversations/session.db"),
        &[(3, 7, ""), (5, 2, "")],
    );
    native_models::set_test_generation(
        &native_conn,
        7,
        "",
        &native_models::test_model_id("Orchestrator (Carbon)"),
    );
    native_models::set_test_generation(
        &native_conn,
        2,
        "",
        &native_models::test_model_id("Gemini Flash"),
    );
    let db = Database::memory().unwrap();
    let worker_rows = [
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];
    assert_eq!(
        import_records(&db, "session", &worker_rows, Some(&source)).unwrap(),
        (2, 0)
    );
    assert_price(
        &db,
        3,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "2.325",
    );
    assert_price(
        &db,
        5,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "2.325",
    );
}

#[test]
fn cli_worker_does_not_inherit_stale_direct_hydrogen_selection_across_rewind() {
    if crate::config::sqlite_unsupported_in_temp_dir() {
        return;
    }
    // Rewinding/rewriting a direct `Gemini 4 Hydrogen (XHigh)` step into a CLI Orchestrator
    // worker step without a native DB must not keep the stale direct-model tier or display.
    let db_rewind = Database::memory().unwrap();
    let direct_rows = [
        json!({"step_index":0,"type":"USER_INPUT","content":"changed setting `Model Selection` from None to Gemini 4 Hydrogen (XHigh). No need to comment"}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];
    assert_eq!(
        import_records(&db_rewind, "session", &direct_rows, None).unwrap(),
        (1, 0)
    );
    assert_price(
        &db_rewind,
        3,
        "gemini-4-hydrogen-xhigh",
        "Gemini 4 Hydrogen (XHigh)",
        "2.325",
    );
    let rewound_rows = [
        json!({"step_index":0,"type":"USER_INPUT","content":"changed setting `Model Selection` from None to Orchestrator (Carbon). No need to comment"}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":2_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];
    assert_eq!(
        import_records(&db_rewind, "session", &rewound_rows, None).unwrap(),
        (1, 0)
    );
    assert_price(
        &db_rewind,
        3,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "3.825",
    );

    // Rewinding an Orchestrator planner step (`gemini-4-carbon`) into a CLI worker step while
    // `gen_metadata` is unreadable must not retain Carbon; a Hydrogen worker step imported under
    // the mode display keeps its Hydrogen variant while adopting the explicit worker request ID.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("conversations")).unwrap();
    let source = dir
        .path()
        .join("brain/session/.system_generated/logs/transcript_full.jsonl");
    let mode = "Orchestrator (Carbon)";
    let native_conn = native(
        &dir.path().join("conversations/session.db"),
        "gemini-pro-orchestrator",
        mode,
    );
    native_models::set_test_generation(
        &native_conn,
        7,
        "gemini-flash-worker-agy",
        &native_models::test_model_id("Gemini 4 Hydrogen (Orchestrator)"),
    );
    let db_mixed = Database::memory().unwrap();
    assert_eq!(
        import_records(&db_mixed, "session", &rows(mode), Some(&source)).unwrap(),
        (2, 0)
    );
    assert_price(&db_mixed, 1, "gemini-4-carbon", mode, "3.1");
    assert_price(
        &db_mixed,
        3,
        "gemini-4-hydrogen-orchestrator",
        mode,
        "2.325",
    );

    native_conn
        .execute_batch("DROP TABLE gen_metadata")
        .unwrap();
    let rewritten_cli = [
        json!({"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","status":"DONE","model":"gemini-flash-worker-agy","input_tokens":1_000_000,"output_tokens":100_000,"cache_read_tokens":1_000_000}),
    ];
    assert_eq!(
        import_records(&db_mixed, "session", &rewritten_cli, Some(&source)).unwrap(),
        (2, 0)
    );
    assert_price(
        &db_mixed,
        1,
        "gemini-4-hydrogen",
        "gemini-flash-worker-agy",
        "2.325",
    );
    assert_price(
        &db_mixed,
        3,
        "gemini-4-hydrogen-orchestrator",
        "gemini-flash-worker-agy",
        "2.325",
    );
}
