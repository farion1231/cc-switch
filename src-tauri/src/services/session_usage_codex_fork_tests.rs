// End-to-end tests: the production directory scanner, persisted cursors/costs,
// and the same aggregate queries used by the usage dashboard.
use std::ffi::OsString;
use std::str::FromStr;

const REPLACEMENT_ID: &str = "00000000-0000-4000-8000-000000000002";
const REFERENCED_CHILD_ID: &str = "00000000-0000-4000-8000-000000000003";
const FIXTURES: &[(&str, &str)] = &[
    (
        "parent",
        include_str!("../../tests/fixtures/codex-fork-usage/parent.jsonl"),
    ),
    (
        "replacement",
        include_str!("../../tests/fixtures/codex-fork-usage/replacement.jsonl"),
    ),
    (
        "child-a",
        include_str!("../../tests/fixtures/codex-fork-usage/child-a.jsonl"),
    ),
    (
        "child-b",
        include_str!("../../tests/fixtures/codex-fork-usage/child-b.jsonl"),
    ),
    (
        "grandchild",
        include_str!("../../tests/fixtures/codex-fork-usage/grandchild.jsonl"),
    ),
];

struct FixtureHome {
    dir: tempfile::TempDir,
    previous: Option<OsString>,
}

impl FixtureHome {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        clear_codex_replay_caches();
        fs::create_dir_all(dir.path().join(".codex/sessions/2026/09/30")).unwrap();
        assert_eq!(get_codex_config_dir(), dir.path().join(".codex"));
        assert_eq!(
            crate::config::get_app_config_dir(),
            dir.path().join(".cc-switch")
        );
        Self { dir, previous }
    }

    fn path(&self, name: &str) -> PathBuf {
        let suffix = match name {
            "parent" => PARENT_ID.to_string(),
            "replacement" => format!("{PARENT_ID}_{REPLACEMENT_ID}"),
            "child-a" => REFERENCED_CHILD_ID.to_string(),
            "child-b" => "00000000-0000-4000-8000-000000000004".to_string(),
            "grandchild" => "00000000-0000-4000-8000-000000000005".to_string(),
            _ => panic!("unknown fixture"),
        };
        self.dir
            .path()
            .join(".codex/sessions/2026/09/30")
            .join(format!("rollout-2026-09-30T00-00-00-{suffix}.jsonl"))
    }

    fn install(&self, names: &[&str]) {
        for name in names {
            let text = FIXTURES.iter().find(|(n, _)| n == name).unwrap().1;
            fs::write(self.path(name), text).unwrap();
        }
    }

    fn database(&self) -> Result<Database, AppError> {
        let db = Database::init()?;
        let conn = lock_conn!(db.conn);
        // Synthetic prices, not a live provider tariff: $2/$10/$1 per million.
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing VALUES ('gpt-5.6-sol', 'Fixture', '2', '10', '1', '0')",
            [],
        )?;
        drop(conn);
        Ok(db)
    }
}

impl Drop for FixtureHome {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        clear_codex_replay_caches();
    }
}

fn assert_usage(
    db: &Database,
    requests: u64,
    input: u64,
    cached: u64,
    output: u64,
) -> Result<(), AppError> {
    let expected_cost =
        Decimal::from((input - cached) * 2 + cached + output * 10) / Decimal::from(1_000_000);
    let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
    eprintln!("[FORK-USAGE] {}", serde_json::to_string(&summary).unwrap());
    assert_eq!(summary.total_requests, requests);
    assert_eq!(summary.total_input_tokens, input - cached);
    assert_eq!(summary.total_cache_read_tokens, cached);
    assert_eq!(summary.total_output_tokens, output);
    assert_eq!(summary.real_total_tokens, input + output);
    assert_eq!(
        Decimal::from_str(&summary.total_cost).unwrap(),
        expected_cost
    );
    let models = db.get_model_stats(None, None, Some("codex"), None, None)?;
    if requests > 0 {
        assert_eq!(models.len(), 1);
        // ModelStats.total_tokens and UsageSummary.real_total_tokens both
        // include cached input; Codex's input already includes cache reads.
        assert_eq!(models[0].total_tokens, input + output);
        assert_eq!(
            Decimal::from_str(&models[0].total_cost).unwrap(),
            expected_cost
        );
    }
    // Verify committed storage separately, not just the scanner/parser result.
    let conn = lock_conn!(db.conn);
    let totals: (u64, u64, u64, u64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(output_tokens), 0)
         FROM proxy_request_logs WHERE data_source = 'codex_session'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(totals, (requests, input, cached, output));
    let costs = conn.prepare("SELECT input_cost_usd, cache_read_cost_usd, output_cost_usd, total_cost_usd FROM proxy_request_logs")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut total = Decimal::ZERO;
    for (i, c, o, t) in costs {
        let amount = Decimal::from_str(&t).unwrap();
        assert_eq!(
            Decimal::from_str(&i).unwrap()
                + Decimal::from_str(&c).unwrap()
                + Decimal::from_str(&o).unwrap(),
            amount
        );
        total += amount;
    }
    assert_eq!(total, expected_cost);
    Ok(())
}

fn scan(db: &Database) -> Result<SessionSyncResult, AppError> {
    let result = sync_codex_usage(db)?;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    Ok(result)
}

#[test]
#[serial_test::serial]
fn ordinary_session_storage_and_cost_are_unchanged() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent"]);
    let db = home.database()?;
    assert_eq!(scan(&db)?.imported, 2);
    assert_usage(&db, 2, 1500, 600, 150)?;
    assert_eq!(scan(&db)?.imported, 0);
    assert_usage(&db, 2, 1500, 600, 150)
}

#[test]
#[serial_test::serial]
fn legacy_single_multiple_and_nested_forks_do_not_rebill_history() -> Result<(), AppError> {
    for children in [1, 2] {
        let home = FixtureHome::new();
        let db = home.database()?;
        let a = token_count_at(1000, 400, 100, "2026-09-30T00:00:02Z");
        write_jsonl(
            &home.path("parent"),
            &[
                session_meta_at(PARENT_ID, None, None, "2026-09-30T00:00:00Z"),
                turn_context(),
                a.clone(),
                turn_context_at("2026-09-30T00:00:59Z"),
            ],
        );
        for (name, id, input) in [
            ("child-a", REFERENCED_CHILD_ID, 1200),
            ("child-b", "00000000-0000-4000-8000-000000000004", 1400),
        ]
        .into_iter()
        .take(children)
        {
            write_jsonl(
                &home.path(name),
                &[
                    session_meta_at(id, Some(PARENT_ID), None, "2026-09-30T00:00:10Z"),
                    turn_context(),
                    a.clone(),
                    token_count_at(input, 500, 120, "2026-09-30T00:00:12Z"),
                    turn_context_at("2026-09-30T00:00:59Z"),
                ],
            );
        }
        write_jsonl(
            &home.path("grandchild"),
            &[
                session_meta_at(
                    "00000000-0000-4000-8000-000000000005",
                    None,
                    Some(REFERENCED_CHILD_ID),
                    "2026-09-30T00:00:20Z",
                ),
                turn_context(),
                a,
                token_count_at(1200, 500, 120, "2026-09-30T00:00:12Z"),
                token_count_at(1300, 550, 130, "2026-09-30T00:00:22Z"),
            ],
        );
        assert_eq!(scan(&db)?.deferred_files, 0);
        let (rows, i, c, o) = if children == 1 {
            (3, 1300, 550, 130)
        } else {
            (4, 1700, 650, 150)
        };
        assert_usage(&db, rows, i, c, o)?;
        clear_codex_replay_caches();
        assert_eq!(scan(&db)?.imported, 0);
        assert_usage(&db, rows, i, c, o)?;
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn referenced_idle_parent_uses_the_exact_inherited_prefix() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "child-b"]);
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 0);
    assert_usage(&db, 3, 1900, 700, 190)
}

#[test]
#[serial_test::serial]
fn referenced_replacement_multiple_and_nested_forks_reach_dashboard_once() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "replacement", "child-a", "child-b", "grandchild"]);
    let db = home.database()?;
    let result = scan(&db)?;
    assert_eq!(result.deferred_files, 0);
    assert_eq!(result.imported, 6);
    assert_usage(&db, 6, 2500, 950, 250)?;
    clear_codex_replay_caches();
    assert_eq!(scan(&db)?.imported, 0);
    assert_usage(&db, 6, 2500, 950, 250)?;
    // Erase cursors too: request-level identity must independently prevent rebilling.
    {
        let conn = lock_conn!(db.conn);
        conn.execute("DELETE FROM session_log_sync", [])?;
    }
    clear_codex_replay_caches();
    assert_eq!(scan(&db)?.imported, 0);
    assert_usage(&db, 6, 2500, 950, 250)?;
    if let Some(out) = std::env::var_os("CODEX_FORK_EVIDENCE_DIR") {
        fs::create_dir_all(&out).unwrap();
        fs::copy(
            home.dir.path().join(".cc-switch/cc-switch.db"),
            Path::new(&out).join("verified-usage.db"),
        )
        .unwrap();
        let summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        fs::write(
            Path::new(&out).join("verified-summary.json"),
            serde_json::to_vec_pretty(&summary).unwrap(),
        )
        .unwrap();
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn missing_referenced_parent_recovers_without_advancing_child_cursor() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["child-a"]);
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 1);
    assert_usage(&db, 0, 0, 0, 0)?;
    assert_eq!(
        get_sync_state(&db, &home.path("child-a").to_string_lossy())?,
        (0, 0)
    );
    home.install(&["parent", "replacement"]);
    assert_eq!(scan(&db)?.deferred_files, 0);
    assert_usage(&db, 4, 2000, 800, 200)
}

#[test]
#[serial_test::serial]
fn referenced_total_only_usage_without_bootstrap_uses_inherited_baseline() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "replacement", "child-a"]);
    advance_legacy_parent_past_fork(&home);
    let mut records: Vec<serde_json::Value> = FIXTURES[2]
        .1
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records[2]["type"] = serde_json::json!("event_msg");
    records[2]["payload"] = serde_json::json!({"type": "task_started"});
    // No inherited token snapshot physically present; preserve local ordinals.
    records[3]["payload"]["info"]
        .as_object_mut()
        .unwrap()
        .remove("last_token_usage");
    write_jsonl(&home.path("child-a"), &records);
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 0);
    assert_usage(&db, 4, 2000, 800, 200)
}

#[test]
#[serial_test::serial]
fn parent_events_beyond_reference_boundary_cannot_hide_real_child_requests() -> Result<(), AppError>
{
    let home = FixtureHome::new();
    home.install(&["parent", "child-b"]);
    let mut parent: Vec<serde_json::Value> = FIXTURES[0]
        .1
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut matching =
        serde_json::from_str::<serde_json::Value>(FIXTURES[3].1.lines().last().unwrap()).unwrap();
    matching["timestamp"] = serde_json::json!("2026-09-30T00:00:20Z");
    matching["ordinal"] = serde_json::json!(4);
    parent.push(matching);
    parent.push(turn_context_at("2026-09-30T00:00:59Z"));
    write_jsonl(&home.path("parent"), &parent);
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 0);
    // Same counters/last usage, but separate paid requests on opposite sides
    // of the inherited boundary. A timestamp-only match would drop the child.
    assert_usage(&db, 4, 2300, 800, 230)
}

#[test]
#[serial_test::serial]
fn invalid_reference_boundaries_defer_without_committing_child_usage() -> Result<(), AppError> {
    for (field, value) in [
        ("end_byte_offset", serde_json::json!(654)),
        ("end_byte_offset", serde_json::json!(99999)),
        ("end_ordinal_exclusive", serde_json::json!(4)),
        ("thread_id", serde_json::json!("invalid")),
    ] {
        let home = FixtureHome::new();
        home.install(&["parent", "child-b"]);
        let mut parent: Vec<serde_json::Value> = FIXTURES[0]
            .1
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        parent.push(turn_context_at("2026-09-30T00:00:59Z"));
        write_jsonl(&home.path("parent"), &parent);
        let mut records: Vec<serde_json::Value> = FIXTURES[3]
            .1
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        records[0]["payload"]["history_base"][field] = value;
        write_jsonl(&home.path("child-b"), &records);
        let db = home.database()?;
        assert_eq!(scan(&db)?.deferred_files, 1);
        assert_usage(&db, 2, 1500, 600, 150)?;
        assert_eq!(
            get_sync_state(&db, &home.path("child-b").to_string_lossy())?,
            (0, 0)
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn incomplete_referenced_parent_recovers_after_append() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "child-b"]);
    fs::write(home.path("parent"), &FIXTURES[0].1.as_bytes()[..654]).unwrap();
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 1);
    assert_eq!(
        get_sync_state(&db, &home.path("child-b").to_string_lossy())?,
        (0, 0)
    );
    home.install(&["parent"]);
    assert_eq!(scan(&db)?.deferred_files, 0);
    assert_usage(&db, 3, 1900, 700, 190)
}

#[test]
#[serial_test::serial]
fn conflicting_reference_copies_defer_instead_of_choosing_one() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "child-b"]);
    let archive = home.dir.path().join(".codex/archived_sessions");
    fs::create_dir_all(&archive).unwrap();
    let contents = FIXTURES[0]
        .1
        .replace("\"input_tokens\":1000", "\"input_tokens\":1001");
    fs::write(
        archive.join(home.path("parent").file_name().unwrap()),
        contents,
    )
    .unwrap();
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 1);
    assert_eq!(
        get_sync_state(&db, &home.path("child-b").to_string_lossy())?,
        (0, 0)
    );
    let conn = lock_conn!(db.conn);
    let children: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs WHERE session_id = '00000000-0000-4000-8000-000000000004'", [], |row| row.get(0))?;
    assert_eq!(children, 0);
    Ok(())
}

#[test]
#[serial_test::serial]
fn referenced_filtered_bootstrap_does_not_rebill_omitted_history() -> Result<(), AppError> {
    let home = FixtureHome::new();
    home.install(&["parent", "replacement", "child-a"]);
    advance_legacy_parent_past_fork(&home);
    let mut records: Vec<serde_json::Value> = FIXTURES[2]
        .1
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let ancestor: serde_json::Value =
        serde_json::from_str(FIXTURES[0].1.lines().nth(2).unwrap()).unwrap();
    // Only an older A snapshot is copied locally. C is still inherited through
    // the reference and is not new usage in the subsequent cumulative delta.
    records[2]["payload"]["info"] = ancestor["payload"]["info"].clone();
    for record in &mut records[2..] {
        record["payload"]["info"]
            .as_object_mut()
            .unwrap()
            .remove("last_token_usage");
    }
    write_jsonl(&home.path("child-a"), &records);
    let db = home.database()?;
    assert_eq!(scan(&db)?.deferred_files, 0);
    assert_usage(&db, 4, 2000, 800, 200)
}

fn advance_legacy_parent_past_fork(home: &FixtureHome) {
    let mut records: Vec<serde_json::Value> = FIXTURES[0]
        .1
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // Make the old timestamp resolver succeed, so this test exposes actual
    // inherited-token rebilling rather than merely a deferred child.
    records.push(turn_context_at("2026-09-30T00:00:59Z"));
    write_jsonl(&home.path("parent"), &records);
}
