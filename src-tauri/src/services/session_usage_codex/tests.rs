use super::*;
use crate::services::session_usage::get_sync_state;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_LEGACY;
use tempfile::tempdir;

#[test]
fn test_usage_line_filter_preserves_existing_predicate() {
    let tags = [
        "\"event_msg\"",
        "\"turn_context\"",
        "\"session_meta\"",
        "\"token_usage_record\"",
        "\"token_count\"",
        "\"thread_settings_applied\"",
    ];
    for mask in 0..(1 << tags.len()) {
        let line = tags
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, tag)| *tag)
            .collect::<Vec<_>>()
            .join(" ");
        let event = line.contains(tags[0]);
        let expected =
            (event || line.contains(tags[1]) || line.contains(tags[2]) || line.contains(tags[3]))
                && (!event || line.contains(tags[4]) || line.contains(tags[5]));
        assert_eq!(is_usage_line(&line), expected, "tag mask {mask}");
    }
}

#[test]
fn test_parent_record_preserves_value_projection() {
    let records = [
        r#"{"timestamp":"2026-07-10T03:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100}}}}"#,
        r#"{"payload":{"info":{"last_token_usage":{"output_tokens":10}},"type":"token_count"},"type":"event_msg","timestamp":"2026-07-10T03:00:01Z"}"#,
        r#"{"timestamp":1,"type":false,"payload":{"type":[],"info":{}}}"#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","payload":"legacy payload"}"#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","payload":[]}"#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","payload":[{"type":"token_count","info":{}},true,1.5,"text",null]}"#,
        r#"{"payload":false,"payload":{"type":"token_count","info":{}}}"#,
        r#"{"payload":[1,2,3],"payload":{"type":"token_count","info":{}}}"#,
        r#"{"payload":{"type":"token_count","info":{}},"payload":42}"#,
        r#"{"payload":{"type":"token_count","info":{}},"payload":[{"type":"token_count","info":{}}]}"#,
        r#"{"type":"response_item","type":"event_msg","timestamp":null,"timestamp":"2026-07-10T03:00:01Z","payload":{"type":"token_count","info":{}}}"#,
        r#"{"payload":{"type":"message","type":"token_count","info":{"old":1},"info":{"last_token_usage":{}}},"type":"event_msg"}"#,
        r#"{"payload":{"type":"token_count","info":{}},"payload":null}"#,
        r#"{"payload":null,"payload":{"type":"token_count","info":{}}}"#,
        r#"{"timestamp":null,"type":null,"payload":{"type":null,"info":null}}"#,
        r#"{"unknown":{"nested":[1,true,null,"text"]}}"#,
        "[]",
        r#"[{"timestamp":"2026-07-10T03:00:01Z","payload":{"type":"token_count","info":{}}}]"#,
        r#""legacy record""#,
        "true",
        "-1",
        "18446744073709551615",
        "1.5",
        "null",
    ];
    for line in records {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let record = parse_parent_record(line).unwrap();
        assert_eq!(
            parse_timestamp(record.timestamp.as_ref()),
            parse_timestamp(value.get("timestamp"))
        );
        assert_eq!(
            record
                .event_type
                .as_ref()
                .and_then(serde_json::Value::as_str),
            value.get("type").and_then(serde_json::Value::as_str)
        );
        assert_eq!(
            record
                .payload
                .as_ref()
                .and_then(|payload| payload.event_type.as_ref())
                .and_then(serde_json::Value::as_str),
            value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(serde_json::Value::as_str)
        );
        assert_eq!(
            record
                .payload
                .as_ref()
                .and_then(|payload| payload.info.as_ref())
                .filter(|info| !info.is_null()),
            value
                .get("payload")
                .and_then(|payload| payload.get("info"))
                .filter(|info| !info.is_null())
        );
    }
    for line in [
        r#"{"timestamp":"2026-07-10T03:00:01Z","payload":{ "#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","ignored":{"text":"\ud800"}}"#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","payload":{"text":"\udc00"}}"#,
        r#"{"timestamp":"2026-07-10T03:00:01Z","ignored":[1e999]}"#,
        r#"{"payload":["\ud800"],"payload":{"type":"token_count","info":{}}}"#,
        r#"{"payload":[1e999],"payload":null}"#,
        r#"{"payload":[{"\udc00":null}]}"#,
        r#"["\ud800"]"#,
        "{} {}",
    ] {
        assert!(serde_json::from_str::<serde_json::Value>(line).is_err());
        assert!(parse_parent_record(line).is_err());
    }
}

#[test]
fn test_projected_json_preserves_value_validation() {
    // Exercise all three validation paths: a top-level non-object, a legacy
    // payload, and an ignored field. Even overwritten values must be valid.
    let values = [
        "null",
        "true",
        "false",
        "0",
        "-1",
        "1.5",
        "1e300",
        "1e999",
        "18446744073709551616",
        r#""\ud83d\ude00""#,
        r#""\ud800""#,
        r#""\udc00""#,
        r#"{"\ud800":0}"#,
        "[1,]",
        "{\"a\":}",
        "01",
        "",
    ];
    let nested = (125..=130).map(|depth| format!("{}0{}", "[".repeat(depth), "]".repeat(depth)));
    for value in values.iter().map(|value| value.to_string()).chain(nested) {
        for line in [
            value.clone(),
            format!(r#"{{"payload":{value},"payload":null}}"#),
            format!(r#"{{"ignored":{value}}}"#),
            format!(r#"{{"payload":{{"ignored":{value}}}}}"#),
        ] {
            let expected = serde_json::from_str::<serde_json::Value>(&line).is_ok();
            assert_eq!(parse_parent_record(&line).is_ok(), expected, "{line}");
            assert_eq!(
                serde_json::from_slice::<IgnoredJsonValue>(line.as_bytes()).is_ok(),
                expected,
                "{line}"
            );
        }
    }
    // from_slice must reject invalid UTF-8 too, including object keys.
    for line in [b"\"\xff\"".as_slice(), b"{\"\xff\":0}", b"[\"\xff\"]"] {
        assert!(serde_json::from_slice::<serde_json::Value>(line).is_err());
        assert!(serde_json::from_slice::<IgnoredJsonValue>(line).is_err());
    }
}

const PARENT_ID: &str = "00000000-0000-4000-8000-000000000001";
const CHILD_A_ID: &str = "00000000-0000-4000-8000-000000000002";
const CHILD_B_ID: &str = "00000000-0000-4000-8000-000000000003";
const PARENT_PAGE_ID: &str = "00000000-0000-4000-8000-000000000004";
const CHILD_A_PAGE_ID: &str = "00000000-0000-4000-8000-000000000005";
const CHILD_A_PAGE_2_ID: &str = "00000000-0000-4000-8000-000000000006";

thread_local! {
    static CODEX_PARSE_COUNTS: std::cell::RefCell<HashMap<PathBuf, usize>> =
        std::cell::RefCell::new(HashMap::new());
    static FORK_EVIDENCE_READ_COUNTS: std::cell::RefCell<HashMap<PathBuf, usize>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(super) fn record_codex_parse(path: &Path) {
    CODEX_PARSE_COUNTS.with(|counts| {
        *counts.borrow_mut().entry(path.to_path_buf()).or_default() += 1;
    });
}

fn codex_parse_count(path: &Path) -> usize {
    CODEX_PARSE_COUNTS.with(|counts| counts.borrow().get(path).copied().unwrap_or(0))
}

pub(super) fn record_fork_evidence_read(path: &Path) {
    FORK_EVIDENCE_READ_COUNTS.with(|counts| {
        *counts.borrow_mut().entry(path.to_path_buf()).or_default() += 1;
    });
}

fn fork_evidence_read_count(path: &Path) -> usize {
    FORK_EVIDENCE_READ_COUNTS.with(|counts| counts.borrow().get(path).copied().unwrap_or(0))
}

fn retryable_parent_and_child(dir: &Path) -> (PathBuf, PathBuf) {
    let parent = rollout_path(dir, PARENT_ID);
    let child = rollout_path(dir, CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:02Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:06Z"),
            token_count_at(1_300, 1_050, 150, "2026-07-10T03:00:07Z"),
        ],
    );
    (parent, child)
}

fn append_jsonl(path: &Path, value: serde_json::Value) {
    use std::io::Write;
    let mut writer = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(writer, "{value}").unwrap();
}

fn write_jsonl(path: &Path, values: &[serde_json::Value]) {
    let contents = values
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(path, contents).unwrap();
}

fn rollout_path(dir: &Path, thread_id: &str) -> PathBuf {
    dir.join(format!("rollout-2026-07-10T03-00-00-{thread_id}.jsonl"))
}

fn paginated_session_meta_at(
    thread_id: &str,
    forked_from_id: Option<&str>,
    spawned_from_id: Option<&str>,
    history_base_thread_id: Option<&str>,
    timestamp: &str,
) -> serde_json::Value {
    let source = spawned_from_id.map_or_else(
        || serde_json::Value::String("cli".to_string()),
        |parent| {
            serde_json::json!({
                "subagent": {
                    "thread_spawn": { "parent_thread_id": parent }
                }
            })
        },
    );
    let mut payload = serde_json::json!({
        "id": thread_id,
        "forked_from_id": forked_from_id,
        "source": source,
        "history_mode": "paginated",
    });
    if let Some(hb_id) = history_base_thread_id {
        payload["history_base"] = serde_json::json!({
            "thread_id": hb_id,
            "end_ordinal_exclusive": 100,
            "end_byte_offset": 5000,
        });
    }
    serde_json::json!({
        "timestamp": timestamp,
        "type": "session_meta",
        "payload": payload
    })
}

fn session_meta_at(
    thread_id: &str,
    forked_from_id: Option<&str>,
    spawned_from_id: Option<&str>,
    timestamp: &str,
) -> serde_json::Value {
    let source = spawned_from_id.map_or_else(
        || serde_json::Value::String("cli".to_string()),
        |parent| {
            serde_json::json!({
                "subagent": {
                    "thread_spawn": { "parent_thread_id": parent }
                }
            })
        },
    );
    serde_json::json!({
        "timestamp": timestamp,
        "type": "session_meta",
        "payload": {
            "id": thread_id,
            "forked_from_id": forked_from_id,
            "source": source
        }
    })
}

fn session_meta(thread_id: &str) -> serde_json::Value {
    session_meta_at(thread_id, None, None, "2026-07-10T03:00:00Z")
}

fn turn_context_for_model_at(model: &str, timestamp: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "type": "turn_context",
        "payload": { "model": model }
    })
}

fn turn_context_at(timestamp: &str) -> serde_json::Value {
    turn_context_for_model_at("gpt-5.6-sol", timestamp)
}

fn turn_context() -> serde_json::Value {
    turn_context_at("2026-07-10T03:00:01Z")
}

fn thread_settings_at(model: serde_json::Value, timestamp: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "type": "thread_settings_applied",
            "thread_settings": { "model": model }
        }
    })
}

fn token_count_at(input: u64, cached: u64, output: u64, timestamp: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": { "total_token_usage": {
                "input_tokens": input,
                "cached_input_tokens": cached,
                "output_tokens": output,
                "reasoning_output_tokens": 0,
                "total_tokens": input + output
            }}
        }
    })
}

fn token_count(input: u64, cached: u64, output: u64) -> serde_json::Value {
    token_count_at(input, cached, output, "2026-07-10T03:00:02Z")
}

fn token_count_without_timestamp(input: u64, cached: u64, output: u64) -> serde_json::Value {
    let mut value = token_count(input, cached, output);
    value
        .as_object_mut()
        .expect("token_count must be an object")
        .remove("timestamp");
    value
}

#[allow(clippy::too_many_arguments)]
fn token_count_with_last_at(
    total_input: u64,
    total_cached: u64,
    total_output: u64,
    last_input: u64,
    last_cached: u64,
    last_output: u64,
    limit_id: &str,
    timestamp: &str,
) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "total_token_usage": {
                    "input_tokens": total_input,
                    "cached_input_tokens": total_cached,
                    "output_tokens": total_output,
                    "reasoning_output_tokens": 0,
                    "total_tokens": total_input + total_output
                },
                "last_token_usage": {
                    "input_tokens": last_input,
                    "cached_input_tokens": last_cached,
                    "output_tokens": last_output,
                    "reasoning_output_tokens": 0,
                    "total_tokens": last_input + last_output
                }
            },
            "rate_limits": { "limit_id": limit_id }
        }
    })
}

fn token_usage_record_at(
    response_id: &str,
    input: u64,
    cached: u64,
    output: u64,
    timestamp: &str,
) -> serde_json::Value {
    serde_json::json!({
        "timestamp": timestamp,
        "type": "token_usage_record",
        "payload": {
            "response_id": response_id,
            "usage": {
                "input_tokens": input,
                "cached_input_tokens": cached,
                "cache_write_input_tokens": 0,
                "output_tokens": output,
                "reasoning_output_tokens": 0,
                "total_tokens": input + output
            }
        }
    })
}

fn codex_usage_rows(db: &Database) -> Result<Vec<(String, i64, i64)>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(
            "SELECT request_id, input_tokens, output_tokens
                 FROM proxy_request_logs WHERE data_source = 'codex_session'
                 ORDER BY request_id",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|error| AppError::Database(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(rows)
}

#[test]
fn remote_compaction_usage_record_is_imported_when_the_following_token_count_is_zero(
) -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let path = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &path,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_usage_record_at("resp_compact", 26_705, 0, 491, "2026-07-10T03:00:02Z"),
            token_count_with_last_at(0, 0, 0, 0, 0, 0, "default", "2026-07-10T03:00:02Z"),
        ],
    );

    let result = sync_test_file(&db, &path, &[&path])?;

    assert_eq!(result.imported, 1, "compaction usage must be imported");
    let rows = codex_usage_rows(&db)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 26_705);
    assert_eq!(rows[0].2, 491);
    assert!(rows[0].0.ends_with(":usage:resp_compact"));
    Ok(())
}

#[test]
fn remote_compaction_usage_record_survives_a_sync_pass_before_its_token_count(
) -> Result<(), AppError> {
    // The usage record and its compensating zero-last token_count can land
    // in different sync passes (the token_count may lag by the whole tool
    // execution window). The first pass has nothing billable and advances
    // the cursor past the record; the compensating event must still be
    // imported by the pass that finally sees the token_count.
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let path = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &path,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_usage_record_at("resp_compact", 26_705, 0, 491, "2026-07-10T03:00:02Z"),
        ],
    );

    let first = sync_test_file(&db, &path, &[&path])?;
    assert_eq!(first.imported, 0, "nothing billable yet");

    append_jsonl(
        &path,
        token_count_with_last_at(0, 0, 0, 0, 0, 0, "default", "2026-07-10T03:05:02Z"),
    );

    let second = sync_test_file(&db, &path, &[&path])?;
    assert_eq!(
        second.imported, 1,
        "compaction usage must be imported once its token_count arrives"
    );
    let rows = codex_usage_rows(&db)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 26_705);
    assert_eq!(rows[0].2, 491);
    assert!(rows[0].0.ends_with(":usage:resp_compact"));

    // A later pass that reparses the file (any new content) must not bill
    // the compensating event again: the record is re-discovered on every
    // reparse, so the line-offset gate plus the `usage:<key>` request id
    // are what keep it at exactly one row.
    append_jsonl(
        &path,
        token_count_with_last_at(0, 0, 0, 0, 0, 0, "default", "2026-07-10T03:06:02Z"),
    );
    let third = sync_test_file(&db, &path, &[&path])?;
    assert_eq!(third.imported, 0, "compensating event must not re-import");
    let rows = codex_usage_rows(&db)?;
    assert_eq!(rows.len(), 1);
    Ok(())
}

#[test]
fn usage_record_covered_by_token_count_is_not_imported_twice() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let path = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &path,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_usage_record_at("resp_turn", 26_705, 1_000, 491, "2026-07-10T03:00:02Z"),
            token_count_with_last_at(
                26_705,
                1_000,
                491,
                26_705,
                1_000,
                491,
                "default",
                "2026-07-10T03:00:03Z",
            ),
        ],
    );

    let result = sync_test_file(&db, &path, &[&path])?;

    assert_eq!(result.imported, 1);
    let rows = codex_usage_rows(&db)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 26_705);
    assert_eq!(rows[0].2, 491);
    assert_eq!(
        rows[0].0,
        format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:1")
    );
    Ok(())
}

#[test]
fn already_scanned_compaction_stays_behind_the_cursor() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let path = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &path,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_usage_record_at("resp_compact", 26_705, 0, 491, "2026-07-10T03:00:02Z"),
            token_count_with_last_at(0, 0, 0, 0, 0, 0, "default", "2026-07-10T03:00:02Z"),
        ],
    );
    let metadata = fs::metadata(&path).unwrap();
    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO session_log_sync
                    (file_path, last_modified, last_line_offset, last_byte_offset, last_synced_at)
                 VALUES (?1, ?2, 100, ?3, 0)",
            rusqlite::params![
                path.to_string_lossy().to_string(),
                metadata_modified_nanos(&metadata),
                i64::try_from(metadata.len()).unwrap()
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }

    let result = sync_test_file(&db, &path, &[&path])?;

    assert_eq!(result.imported, 0);
    assert!(codex_usage_rows(&db)?.is_empty());
    Ok(())
}

fn sync_test_file(
    db: &Database,
    file: &Path,
    all_files: &[&Path],
) -> Result<CodexFileSyncResult, AppError> {
    let files = all_files
        .iter()
        .map(|path| path.to_path_buf())
        .collect::<Vec<_>>();
    let mut pass = CodexSyncPass::load(db)?;
    sync_single_codex_file(db, file, &build_rollout_index(&files), &mut pass)
}

fn assert_unchanged_codex_file_is_skipped(db: &Database, file: &Path) -> Result<(), AppError> {
    let changes_before: i64 = {
        let conn = lock_conn!(db.conn);
        conn.query_row("SELECT total_changes()", [], |row| row.get(0))?
    };
    // Reload the persisted cursor each time: returning zero imports alone
    // does not prove that the file was skipped rather than fully reparsed.
    for _ in 0..3 {
        assert_eq!(sync_test_file(db, file, &[file])?.imported, 0);
    }
    let conn = lock_conn!(db.conn);
    let changes_after: i64 = conn.query_row("SELECT total_changes()", [], |row| row.get(0))?;
    assert_eq!(
        changes_after, changes_before,
        "unchanged file rewrote its cursor"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_archived_legacy_idle_fork_recovery_and_append() -> Result<(), AppError> {
    for compressed in [false, true] {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let dir = tempdir().unwrap();
        let (parent, child) = archived_legacy_fork(dir.path());
        let parent = if compressed {
            compress_rollout(&parent)
        } else {
            parent
        };
        let files = [parent.as_path(), child.as_path()];
        // The parent has additional usage after the child's copied prefix:
        // a rewind must not require replaying the parent's entire history.
        assert_eq!(sync_test_file(&db, &parent, &files)?.imported, 3);
        let result = sync_test_file(&db, &child, &files)?;
        assert_eq!(
            (result.imported, result.skipped, result.deferred),
            (1, 2, false)
        );
        assert_unchanged_codex_file_is_skipped(&db, &child)?;
        clear_codex_replay_caches();
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM session_log_sync", [])?;
        }
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        append_jsonl(
            &child,
            token_count_with_last_at(275, 140, 28, 25, 15, 3, "codex", "2026-07-10T03:00:22Z"),
        );
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        // A genuine child compaction fallback keeps its native response ID.
        append_jsonl(
            &child,
            token_usage_record_at("resp_child_compact", 40, 20, 4, "2026-07-10T03:00:23Z"),
        );
        append_jsonl(
            &child,
            token_count_with_last_at(315, 160, 32, 0, 0, 0, "codex", "2026-07-10T03:00:23Z"),
        );
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        let conn = lock_conn!(db.conn);
        let usage: (i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), SUM(input_tokens), SUM(cache_read_tokens), SUM(output_tokens)
                 FROM proxy_request_logs WHERE session_id = ?1",
            [CHILD_A_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(usage, (3, 115, 60, 12));
    }
    Ok(())
}

fn archived_legacy_fork(dir: &Path) -> (PathBuf, PathBuf) {
    let archive = dir.join("archived_sessions");
    fs::create_dir_all(&archive).unwrap();
    let parent = rollout_path(&archive, PARENT_ID);
    let child = rollout_path(dir, CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_with_last_at(100, 50, 10, 100, 50, 10, "codex", "2026-07-10T03:00:01Z"),
            token_count_with_last_at(200, 100, 20, 100, 50, 10, "codex", "2026-07-10T03:00:02Z"),
            token_count_with_last_at(300, 150, 30, 100, 50, 10, "codex", "2026-07-10T03:00:03Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:20Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
            token_count_with_last_at(
                100,
                50,
                10,
                100,
                50,
                10,
                "codex",
                "2026-07-10T03:00:20.001Z",
            ),
            token_count_with_last_at(
                200,
                100,
                20,
                100,
                50,
                10,
                "codex",
                "2026-07-10T03:00:20.002Z",
            ),
            token_count_with_last_at(250, 125, 25, 50, 25, 5, "codex", "2026-07-10T03:00:21Z"),
        ],
    );
    (parent, child)
}

fn owned_legacy_fork(dir: &Path) -> (PathBuf, PathBuf) {
    let (archived, child) = archived_legacy_fork(dir);
    let active = dir.join("sessions");
    fs::create_dir_all(&active).unwrap();
    let parent = active.join(archived.file_name().unwrap());
    fs::rename(archived, &parent).unwrap();
    for (path, is_child) in [(&parent, false), (&child, true)] {
        let values = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let mut records = Vec::new();
        let mut request = 0;
        for value in values {
            if value
                .pointer("/payload/type")
                .and_then(serde_json::Value::as_str)
                == Some("token_count")
            {
                request += 1;
                let own_request = is_child && request == 3;
                // The parent's copied requests can themselves belong to an
                // ancestor; comparing ownership only with parent_id is wrong.
                let owner = if own_request { CHILD_A_ID } else { CHILD_B_ID };
                let response_id = if own_request {
                    "resp_child".to_string()
                } else {
                    format!("resp_inherited_{request}")
                };
                let usage = value
                    .pointer("/payload/info/last_token_usage")
                    .unwrap()
                    .clone();
                records.push(serde_json::json!({
                        "timestamp": value["timestamp"], "type": "token_usage_record",
                        "payload": { "response_id": response_id, "thread_id": owner, "session_id": owner, "usage": usage }
                    }));
            }
            records.push(value);
        }
        write_jsonl(path, &records);
    }
    (parent, child)
}

#[test]
#[serial_test::serial]
fn test_owned_legacy_fork_recovers_from_active_idle_parent() -> Result<(), AppError> {
    for compressed in [false, true] {
        clear_codex_replay_caches();
        let dir = tempdir().unwrap();
        let db = Database::memory()?;
        let (parent, child) = owned_legacy_fork(dir.path());
        let parent = if compressed {
            compress_rollout(&parent)
        } else {
            parent
        };
        let files = [parent.as_path(), child.as_path()];
        let result = sync_test_file(&db, &child, &files)?;
        assert_eq!(
            (result.imported, result.skipped, result.deferred),
            (1, 2, false)
        );
        assert_unchanged_codex_file_is_skipped(&db, &child)?;
        lock_conn!(db.conn).execute("DELETE FROM session_log_sync", [])?;
        clear_codex_replay_caches();
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        let mut usage = token_usage_record_at("resp_append", 25, 15, 3, "2026-07-10T03:00:22Z");
        usage["payload"]["thread_id"] = serde_json::json!(CHILD_A_ID);
        usage["payload"]["session_id"] = serde_json::json!(CHILD_A_ID);
        append_jsonl(&child, usage);
        append_jsonl(
            &child,
            token_count_with_last_at(275, 140, 28, 25, 15, 3, "codex", "2026-07-10T03:00:22Z"),
        );
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        let mut compaction =
            token_usage_record_at("resp_compact_owned", 40, 20, 4, "2026-07-10T03:00:23Z");
        compaction["payload"]["thread_id"] = serde_json::json!(CHILD_A_ID);
        append_jsonl(&child, compaction);
        append_jsonl(
            &child,
            token_count_with_last_at(315, 160, 32, 0, 0, 0, "codex", "2026-07-10T03:00:23Z"),
        );
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        assert_eq!(codex_usage_rows(&db)?.len(), 3);
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_owned_legacy_fork_rejects_unverified_request_identity() -> Result<(), AppError> {
    for case in [
        "missing_record",
        "missing_response",
        "foreign_live_owner",
        "forged_replay_response",
        "replay_usage_mismatch",
        "conflicting_session",
        "duplicate_response",
    ] {
        clear_codex_replay_caches();
        let dir = tempdir().unwrap();
        let db = Database::memory()?;
        let (parent, child) = owned_legacy_fork(dir.path());
        let mut records = fs::read_to_string(&child)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let positions = records
            .iter()
            .enumerate()
            .filter_map(|(i, v)| (v["type"] == "token_usage_record").then_some(i))
            .collect::<Vec<_>>();
        let first = positions[0];
        let last = positions[2];
        match case {
            "missing_record" => {
                records.remove(last);
            }
            "missing_response" => {
                records[last]["payload"]
                    .as_object_mut()
                    .unwrap()
                    .remove("response_id");
            }
            "foreign_live_owner" => {
                records[last]["payload"]["thread_id"] = serde_json::json!(CHILD_B_ID);
                records[last]["payload"]["session_id"] = serde_json::json!(CHILD_B_ID);
            }
            "forged_replay_response" => {
                records[first]["payload"]["response_id"] = serde_json::json!("resp_unknown")
            }
            "replay_usage_mismatch" => {
                records[first]["payload"]["usage"]["input_tokens"] = serde_json::json!(999)
            }
            "conflicting_session" => {
                records[last]["payload"]["session_id"] = serde_json::json!(CHILD_B_ID)
            }
            "duplicate_response" => {
                records[last]["payload"]["response_id"] =
                    records[first]["payload"]["response_id"].clone()
            }
            _ => unreachable!(),
        }
        write_jsonl(&child, &records);
        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert!(result.deferred, "{case}");
        assert!(codex_usage_rows(&db)?.is_empty(), "{case}");
    }
    Ok(())
}

fn read_test_records(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
#[serial_test::serial]
fn test_normal_forks_skip_request_evidence_reads() -> Result<(), AppError> {
    for owned in [false, true] {
        for compressed in [false, true] {
            clear_codex_replay_caches();
            let dir = tempdir().unwrap();
            let db = Database::memory()?;
            let (parent, child) = if owned {
                owned_legacy_fork(dir.path())
            } else {
                archived_legacy_fork(dir.path())
            };
            append_jsonl(&parent, turn_context_at("2026-07-10T03:00:30Z"));
            let parent = if compressed {
                compress_rollout(&parent)
            } else {
                parent
            };
            let files = [parent.as_path(), child.as_path()];
            assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
            append_jsonl(
                &child,
                token_count_with_last_at(275, 140, 28, 25, 15, 3, "codex", "2026-07-10T03:00:22Z"),
            );
            assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
            assert_eq!(fork_evidence_read_count(&parent), 0);
            assert_eq!(fork_evidence_read_count(&child), 0);
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_legacy_idle_fork_without_request_records_skips_evidence_reads() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let db = Database::memory()?;
    let (archive, child) = archived_legacy_fork(dir.path());
    let parent = rollout_path(dir.path(), PARENT_ID);
    fs::rename(archive, &parent).unwrap();
    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert!(result.deferred);
    assert_eq!(fork_evidence_read_count(&parent), 0);
    assert_eq!(fork_evidence_read_count(&child), 0);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_owned_fork_caches_prefix_and_parent_evidence_on_append() -> Result<(), AppError> {
    for compressed in [false, true] {
        clear_codex_replay_caches();
        let dir = tempdir().unwrap();
        let db = Database::memory()?;
        let (parent, child) = owned_legacy_fork(dir.path());
        let parent = if compressed {
            compress_rollout(&parent)
        } else {
            parent
        };
        let files = [parent.as_path(), child.as_path()];
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        assert_eq!(
            replay_caches()
                .lock()
                .unwrap()
                .replay_prefixes
                .get(&child)
                .unwrap()
                .prefix,
            2
        );
        assert_eq!(fork_evidence_read_count(&parent), 1);
        assert_eq!(fork_evidence_read_count(&child), 1);
        // Force a reparse with an unchanged child: a prefix-cache hit must
        // bypass ownership recovery, not just reuse evidence after rereading.
        lock_conn!(db.conn).execute("DELETE FROM session_log_sync", [])?;
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        assert_eq!(fork_evidence_read_count(&parent), 1);
        assert_eq!(fork_evidence_read_count(&child), 1);
        let mut usage = token_usage_record_at("resp_new_cached", 25, 15, 3, "2026-07-10T03:00:22Z");
        usage["payload"]["thread_id"] = serde_json::json!(CHILD_A_ID);
        append_jsonl(&child, usage);
        append_jsonl(
            &child,
            token_count_with_last_at(275, 140, 28, 25, 15, 3, "codex", "2026-07-10T03:00:22Z"),
        );
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        assert_eq!(
            fork_evidence_read_count(&parent),
            1,
            "unchanged parent must not be decompressed again"
        );
        assert_eq!(fork_evidence_read_count(&child), 2);
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_owned_fork_ignores_heartbeats_without_losing_pending_record() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let db = Database::memory()?;
    let (parent, child) = owned_legacy_fork(dir.path());
    for path in [&parent, &child] {
        let mut result = Vec::new();
        for value in read_test_records(path) {
            let is_record = value["type"] == "token_usage_record";
            let timestamp = value["timestamp"].clone();
            result.push(value);
            if is_record {
                for payload in [
                    serde_json::Value::Null,
                    serde_json::json!({}),
                    serde_json::json!({"type": null}),
                    serde_json::json!({"type": "token_count", "info": null}),
                    serde_json::json!({"type": "token_count", "info": {}}),
                ] {
                    result.push(serde_json::json!({"timestamp":timestamp,"type":"event_msg","payload":payload}));
                }
                result.push(serde_json::json!({"timestamp":timestamp,"type":"event_msg"}));
            }
        }
        write_jsonl(path, &result);
    }
    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (result.imported, result.skipped, result.deferred),
        (1, 2, false)
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_valid_total_only_event_consumes_pending_evidence_like_main_parser() -> Result<(), AppError>
{
    let dir = tempdir().unwrap();
    let db = Database::memory()?;
    let (parent, child) = owned_legacy_fork(dir.path());
    let mut values = read_test_records(&child);
    let last_usage = values
        .iter()
        .rposition(|v| v["type"] == "token_usage_record")
        .unwrap();
    values.insert(
        last_usage + 1,
        token_count_at(250, 125, 25, "2026-07-10T03:00:20.999Z"),
    );
    write_jsonl(&child, &values);
    // A valid total-only snapshot consumes the pending record in the main
    // parser too. Do not attach that stale identity to the following request.
    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert!(result.deferred);
    assert!(codex_usage_rows(&db)?.is_empty());
    assert!(!read_fork_file_evidence(&child)
        .unwrap()
        .token_lines
        .contains_key(&(values.len() as i64)));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_legacy_recovery_only_checks_events_after_copied_prefix() -> Result<(), AppError> {
    for owned in [false, true] {
        for mode in ["original_timestamp", "same_millisecond", "cumulative_only"] {
            clear_codex_replay_caches();
            let dir = tempdir().unwrap();
            let db = Database::memory()?;
            let (parent, child) = if owned {
                owned_legacy_fork(dir.path())
            } else {
                archived_legacy_fork(dir.path())
            };
            for path in [&parent, &child] {
                let mut values = read_test_records(path);
                let mut tokens = 0;
                for value in &mut values {
                    if value
                        .pointer("/payload/type")
                        .and_then(serde_json::Value::as_str)
                        == Some("token_count")
                    {
                        tokens += 1;
                        if tokens <= 2 {
                            if path == &child {
                                value["timestamp"] =
                                    serde_json::json!(if mode == "original_timestamp" {
                                        "2026-07-10T03:00:02Z"
                                    } else {
                                        "2026-07-10T03:00:20Z"
                                    });
                            }
                            if mode == "cumulative_only" {
                                value["payload"]["info"]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("last_token_usage");
                            }
                        }
                    }
                }
                write_jsonl(path, &values);
            }
            let result = sync_test_file(&db, &child, &[&parent, &child])?;
            assert_eq!(
                (result.imported, result.skipped, result.deferred),
                (1, 2, false),
                "{owned} {mode}"
            );
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_legacy_recovery_still_rejects_invalid_live_request_boundaries() -> Result<(), AppError> {
    for owned in [false, true] {
        for mode in ["before_fork", "at_fork", "missing_last"] {
            clear_codex_replay_caches();
            let dir = tempdir().unwrap();
            let db = Database::memory()?;
            let (parent, child) = if owned {
                owned_legacy_fork(dir.path())
            } else {
                archived_legacy_fork(dir.path())
            };
            let mut values = read_test_records(&child);
            let last = values.last_mut().unwrap();
            match mode {
                "before_fork" => last["timestamp"] = serde_json::json!("2026-07-10T03:00:19Z"),
                "at_fork" => last["timestamp"] = serde_json::json!("2026-07-10T03:00:20Z"),
                "missing_last" => {
                    last["payload"]["info"]
                        .as_object_mut()
                        .unwrap()
                        .remove("last_token_usage");
                }
                _ => unreachable!(),
            }
            write_jsonl(&child, &values);
            assert!(
                sync_test_file(&db, &child, &[&parent, &child])?.deferred,
                "{owned} {mode}"
            );
            assert!(codex_usage_rows(&db)?.is_empty());
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_request_evidence_keeps_latest_duplicate_timestamp() {
    let dir = tempdir().unwrap();
    let (_, child) = owned_legacy_fork(dir.path());
    let mut values = read_test_records(&child);
    let record = values
        .iter()
        .find(|v| v["type"] == "token_usage_record")
        .unwrap()
        .clone();
    let id = record["payload"]["response_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut future = record.clone();
    future["timestamp"] = serde_json::json!("2026-07-10T03:00:30Z");
    values.push(future);
    values.push(record);
    write_jsonl(&child, &values);
    let evidence = read_fork_file_evidence(&child).unwrap();
    assert_eq!(
        evidence.requests[&id].0,
        "2026-07-10T03:00:30Z".parse::<DateTime<Utc>>().unwrap()
    );
}

#[test]
#[serial_test::serial]
fn test_owned_fork_rejects_child_changed_since_primary_parse() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let (parent, child) = owned_legacy_fork(dir.path());
    let parsed = parse_codex_file(&child, Some(CHILD_A_ID.to_string()))?;
    let stamp = ParentFileStamp::from_file(&fs::File::open(&child).unwrap()).unwrap();
    let index = build_rollout_index(&[parent.clone(), child.clone()]);
    assert_eq!(
        verified_owned_fork_prefix(&parsed, &child, &index, Some(stamp)),
        Some(2)
    );
    append_jsonl(&child, turn_context_at("2026-07-10T03:00:22Z"));
    assert_eq!(
        verified_owned_fork_prefix(&parsed, &child, &index, Some(stamp)),
        None
    );
    assert_eq!(
        verified_owned_fork_prefix(&parsed, &child, &index, None),
        None
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_transient_final_parent_stamp_failure_does_not_poison_cache() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let dir = tempdir().unwrap();
    let (parent, _) = owned_legacy_fork(dir.path());
    let stamp = ParentFileStamp::from_file(&fs::File::open(&parent).unwrap()).unwrap();
    let incomplete = Arc::new(ParentTokenTimeline {
        events: Vec::new(),
        max_timestamp: None,
        has_token_without_timestamp: false,
        high_water: None,
        verified_snapshot: false,
    });
    cache_parent_timeline(&parent, Some(stamp), None, &incomplete);
    assert!(!replay_caches()
        .lock()
        .unwrap()
        .parent_timelines
        .contains_key(&parent));
    let mut changed = stamp;
    changed.size += 1;
    cache_parent_timeline(&parent, Some(stamp), Some(changed), &incomplete);
    assert!(!replay_caches()
        .lock()
        .unwrap()
        .parent_timelines
        .contains_key(&parent));
    // Once metadata is readable again, the unchanged parent must be read
    // successfully rather than reuse a permanently unverified timeline.
    let recovered = load_parent_timeline(&parent).unwrap();
    assert!(recovered.verified_snapshot);
    assert_eq!(recovered.events.len(), 3);
    assert!(replay_caches()
        .lock()
        .unwrap()
        .parent_timelines
        .contains_key(&parent));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_archived_legacy_idle_fork_rejects_unverified_parent() -> Result<(), AppError> {
    for case in [
        "live_copy",
        "malformed_json",
        "invalid_utf8",
        "conflicting_copy",
        "missing_timestamp",
    ] {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let dir = tempdir().unwrap();
        let (parent, child) = archived_legacy_fork(dir.path());
        let mut files = vec![parent.clone(), child.clone()];
        match case {
            "live_copy" | "conflicting_copy" => {
                let copy_dir = if case == "live_copy" {
                    dir.path().join("sessions")
                } else {
                    dir.path().join("other").join("archived_sessions")
                };
                fs::create_dir_all(&copy_dir).unwrap();
                let copy = rollout_path(&copy_dir, PARENT_ID);
                fs::copy(&parent, &copy).unwrap();
                if case == "conflicting_copy" {
                    append_jsonl(&copy, token_count_at(999, 0, 99, "2026-07-10T03:00:04Z"));
                }
                files.push(copy);
            }
            "missing_timestamp" => append_jsonl(&parent, token_count_without_timestamp(999, 0, 99)),
            _ => {
                use std::io::Write;
                let mut file = fs::OpenOptions::new().append(true).open(&parent).unwrap();
                file.write_all(if case == "invalid_utf8" {
                    b"\xff\n"
                } else {
                    b"{unfinished"
                })
                .unwrap();
            }
        }
        let paths = files.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let result = sync_test_file(&db, &child, &paths)?;
        assert!(result.deferred, "{case}");
        assert!(codex_usage_rows(&db)?.is_empty(), "{case}");
        assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_archived_legacy_idle_fork_rejects_ambiguous_replay() -> Result<(), AppError> {
    for case in [
        "no_match",
        "cumulative_only",
        "bounded_history",
        "copied_fallback",
        "replay_after_live",
    ] {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let dir = tempdir().unwrap();
        let (parent, child) = archived_legacy_fork(dir.path());
        let mut records = fs::read_to_string(&child)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        match case {
            "no_match" => {
                records.drain(2..4);
            }
            "cumulative_only" => {
                records[4]["payload"]["info"]
                    .as_object_mut()
                    .unwrap()
                    .remove("last_token_usage");
            }
            "bounded_history" => {
                records[0]["payload"]["history_base"] = serde_json::json!({"thread_id": PARENT_ID});
            }
            "copied_fallback" => {
                records.insert(
                    3,
                    token_usage_record_at("resp_copied", 40, 20, 4, "2026-07-10T03:00:20.001Z"),
                );
                records.insert(
                    4,
                    token_count_with_last_at(
                        200,
                        100,
                        20,
                        0,
                        0,
                        0,
                        "codex",
                        "2026-07-10T03:00:20.001Z",
                    ),
                );
                // This zero-last snapshot is part of the parent's history too.
                let mut inherited = records[4].clone();
                inherited["timestamp"] = serde_json::json!("2026-07-10T03:00:02.001Z");
                append_jsonl(&parent, inherited);
            }
            "replay_after_live" => records.push(records[2].clone()),
            _ => unreachable!(),
        }
        write_jsonl(&child, &records);
        let result = sync_test_file(&db, &child, &[&parent, &child])?;
        assert!(result.deferred, "{case}");
        assert!(codex_usage_rows(&db)?.is_empty(), "{case}");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_bounded_idle_fork_recovery_and_append() -> Result<(), AppError> {
    for inherited in [0, 100] {
        clear_codex_replay_caches();
        let db = Database::memory()?;
        let dir = tempdir().unwrap();
        let parent = rollout_path(dir.path(), PARENT_ID);
        let child = rollout_path(dir.path(), CHILD_A_ID);
        let mut records = vec![
            session_meta(PARENT_ID),
            token_count_at(1000, 900, 100, "2026-07-10T03:00:01Z"),
        ];
        for (i, v) in records.iter_mut().enumerate() {
            v["ordinal"] = serde_json::json!(i + inherited);
        }
        records[0]["payload"]["history_base"] =
            serde_json::json!({"end_ordinal_exclusive":inherited});
        write_jsonl(&parent, &records);
        let mut meta = paginated_session_meta_at(
            CHILD_A_ID,
            Some(PARENT_ID),
            None,
            Some(PARENT_ID),
            "2026-07-10T03:00:20Z",
        );
        meta["payload"]["history_base"]["end_byte_offset"] =
            serde_json::json!(fs::metadata(&parent).unwrap().len());
        meta["payload"]["history_base"]["end_ordinal_exclusive"] = serde_json::json!(inherited + 2);
        let mut event = token_count_at(1200, 1000, 120, "2026-07-10T03:00:25Z");
        event["payload"]["info"]["last_token_usage"] = serde_json::json!({"input_tokens":200,"cached_input_tokens":100,"output_tokens":20,"reasoning_output_tokens":0,"total_tokens":220});
        write_jsonl(&child, &[meta.clone(), turn_context(), event.clone()]);
        let files = [&parent as &Path, &child as &Path];
        let result = sync_test_file(&db, &child, &files)?;
        assert!(!result.deferred);
        assert_eq!(result.imported, 1);
        clear_codex_replay_caches();
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM session_log_sync", [])?;
        }
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        // A repeated quota snapshot is not a second request.
        append_jsonl(&child, event.clone());
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        event["timestamp"] = serde_json::json!("2026-07-10T03:00:30Z");
        event["payload"]["info"]["total_token_usage"]["input_tokens"] = serde_json::json!(1400);
        event["payload"]["info"]["total_token_usage"]["total_tokens"] = serde_json::json!(1520);
        append_jsonl(&child, event);
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        // Invalid bounded references must retain the safe deferred behavior.
        for key in ["end_byte_offset", "end_ordinal_exclusive"] {
            let mut broken = meta.clone();
            broken["payload"]["history_base"][key] = serde_json::json!(1);
            let mut parsed = parse_codex_file(&child, Some(CHILD_A_ID.into()))?;
            parsed.bounded_history = broken["payload"].get("history_base").cloned();
            assert!(!verified_bounded_fork(
                &parsed,
                &build_rollout_index(&[parent.clone(), child.clone()])
            ));
        }
        let index = build_rollout_index(&[parent.clone(), child.clone()]);
        let mut parsed = parse_codex_file(&child, Some(CHILD_A_ID.into()))?;
        parsed.token_events[0]
            .signature
            .last
            .as_mut()
            .unwrap()
            .input = Some(999);
        assert!(!verified_bounded_fork(&parsed, &index));
        let mut parsed = parse_codex_file(&child, Some(CHILD_A_ID.into()))?;
        parsed.token_events[0].timestamp = Some("2026-07-10T03:00:01Z".into());
        assert!(!verified_bounded_fork(&parsed, &index));
        let mut parsed = parse_codex_file(&child, Some(CHILD_A_ID.into()))?;
        parsed.token_events[0].signature.last = None;
        assert!(!verified_bounded_fork(&parsed, &index));
        // Remote compaction uses a standalone record followed by a zero-last
        // snapshot. It must not block the entire bounded fork or be counted twice.
        let usage =
            token_usage_record_at("bounded_compaction", 300, 100, 20, "2026-07-10T03:00:35Z");
        let zero =
            token_count_with_last_at(1400, 1000, 120, 0, 0, 0, "default", "2026-07-10T03:00:36Z");
        append_jsonl(&child, usage.clone());
        append_jsonl(&child, zero.clone());
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 1);
        append_jsonl(&child, usage);
        append_jsonl(&child, zero);
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM session_log_sync", [])?;
        }
        clear_codex_replay_caches();
        assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
        // Only the child's two requests and one compaction are billable.
        // Replaying the file must neither import the parent baseline nor rebill the child.
        let totals: (i64, i64, i64, i64) = {
            let conn = lock_conn!(db.conn);
            conn.query_row(
                "SELECT count(*), sum(input_tokens), sum(cache_read_tokens), sum(output_tokens)
                     FROM proxy_request_logs WHERE data_source = 'codex_session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?
        };
        assert_eq!(totals, (3, 700, 300, 60));
        let mut parsed = parse_codex_file(&child, Some(CHILD_A_ID.into()))?;
        parsed.usage_record_events[0].timestamp = Some("2026-07-10T03:00:01Z".into());
        assert!(!verified_bounded_fork(&parsed, &index));
        parsed.usage_record_events[0].timestamp = None;
        assert!(!verified_bounded_fork(&parsed, &index));
    }
    Ok(())
}

// Codex 0.167 resume/fork of a finished thread: single-UUID child rollout
// whose records are numbered from the parent cutoff. The parent never
// reaches the child's session_meta time, so the timestamp-aligned path
// defers it forever. See #9420.
#[test]
#[serial_test::serial]
fn test_ordinal_fork_after_finished_parent_is_imported() -> Result<(), AppError> {
    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let parent = rollout_path(dir.path(), PARENT_ID);
    let child = rollout_path(dir.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1000, 800, 100, "2026-07-10T03:00:01Z"),
        ],
    );
    // The child numbers its own records from the parent cutoff and copies
    // no parent history, so there is nothing to replay.
    let mut meta = session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:20Z");
    meta["ordinal"] = serde_json::json!(5);
    meta["payload"]["forked_from_ordinal_exclusive"] = serde_json::json!(5);
    let event = token_count_with_last_at(
        1200,
        1000,
        120,
        200,
        200,
        20,
        "default",
        "2026-07-10T03:00:25Z",
    );
    write_jsonl(&child, &[meta.clone(), turn_context(), event.clone()]);
    let files = [&parent as &Path, &child as &Path];
    let result = sync_test_file(&db, &child, &files)?;
    assert!(!result.deferred);
    assert_eq!(result.imported, 1);
    assert_eq!(
        codex_usage_rows(&db)?,
        vec![(
            format!("{}:{}:{}", CODEX_THREAD_REQUEST_ID_PREFIX, CHILD_A_ID, 1),
            200,
            20
        )]
    );
    assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
    clear_codex_replay_caches();
    {
        let conn = lock_conn!(db.conn);
        conn.execute("DELETE FROM session_log_sync", [])?;
    }
    // A full replay must not bill the inherited total again.
    assert_eq!(sync_test_file(&db, &child, &files)?.imported, 0);
    // The parent must not be needed at all: an idle parent that never
    // reaches the fork time no longer blocks the child.
    assert_eq!(sync_test_file(&db, &child, &[&child as &Path])?.imported, 0);

    // Any other shape keeps the timestamp-aligned path: no ordinal boundary.
    let mut no_boundary = meta.clone();
    no_boundary["payload"]
        .as_object_mut()
        .unwrap()
        .remove("forked_from_ordinal_exclusive");
    let mut copied = meta.clone();
    copied["ordinal"] = serde_json::json!(0);
    for (variant, records) in [
        (no_boundary, vec![turn_context(), event.clone()]),
        (copied, vec![turn_context(), event.clone()]),
    ] {
        let path = rollout_path(dir.path(), "00000000-0000-4000-8000-000000000009");
        write_jsonl(
            &path,
            &std::iter::once(variant).chain(records).collect::<Vec<_>>(),
        );
        let result = sync_test_file(&db, &path, &[&parent as &Path, &path])?;
        assert!(result.deferred, "unexpected import for {}", path.display());
    }
    Ok(())
}

#[test]
fn test_unchanged_incomplete_tail_is_skipped_after_cursor_reload() -> Result<(), AppError> {
    use std::io::Write;
    for has_usage in [false, true] {
        for tail in ["{\"type\":\"event_msg\"", "  "] {
            let db = Database::memory()?;
            let dir = tempdir().unwrap();
            let file = rollout_path(dir.path(), PARENT_ID);
            let mut records = vec![session_meta(PARENT_ID), turn_context()];
            if has_usage {
                records.push(token_count(100, 50, 10));
            }
            write_jsonl(&file, &records);
            {
                let mut writer = fs::OpenOptions::new().append(true).open(&file).unwrap();
                writer.write_all(tail.as_bytes()).unwrap();
            }
            assert_eq!(
                sync_test_file(&db, &file, &[&file])?.imported,
                u32::from(has_usage)
            );
            assert_eq!(
                get_sync_state(&db, &file.to_string_lossy())?.1,
                records.len() as i64
            );
            assert_unchanged_codex_file_is_skipped(&db, &file)?;
        }
    }
    Ok(())
}

#[test]
fn test_append_with_unchanged_mtime_survives_reload_without_duplicates() -> Result<(), AppError> {
    use std::io::Write;
    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count(100, 50, 10),
        ],
    );
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    let mut writer = fs::OpenOptions::new().append(true).open(&file).unwrap();
    writeln!(writer, "{}", token_count(250, 100, 30)).unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    // Each call reloads its cursor from the DB, as after an application restart.
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
    let conn = lock_conn!(db.conn);
    let totals: (i64, i64, i64) = conn.query_row(
        "SELECT count(*), sum(input_tokens), sum(output_tokens) FROM proxy_request_logs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(totals, (2, 250, 30));
    Ok(())
}

#[test]
fn test_legacy_cursor_catches_up_with_unchanged_mtime() -> Result<(), AppError> {
    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count(100, 50, 10),
            token_count(250, 100, 30),
        ],
    );
    let modified = metadata_modified_nanos(&fs::metadata(&file).unwrap());
    update_sync_state(&db, &file.to_string_lossy(), modified, 3)?;
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
    let conn = lock_conn!(db.conn);
    let bytes: i64 = conn.query_row("SELECT last_byte_offset FROM session_log_sync", [], |r| {
        r.get(0)
    })?;
    assert_eq!(bytes as u64, fs::metadata(&file).unwrap().len());
    Ok(())
}

#[test]
fn test_complete_final_record_without_newline_is_imported_once() -> Result<(), AppError> {
    use std::io::Write;
    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(&file, &[session_meta(PARENT_ID), turn_context()]);
    let mut writer = fs::OpenOptions::new().append(true).open(&file).unwrap();
    write!(writer, "{}", token_count(100, 50, 10)).unwrap();
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
    writeln!(writer).unwrap();
    writeln!(writer, "{}", token_count(250, 100, 30)).unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
    let conn = lock_conn!(db.conn);
    let totals: (i64, i64, i64) = conn.query_row(
        "SELECT count(*), sum(input_tokens), sum(output_tokens) FROM proxy_request_logs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    assert_eq!(totals, (2, 250, 30));
    Ok(())
}

#[test]
fn test_final_record_validation_preserves_cursor() -> Result<(), AppError> {
    let large_body = format!(
        r#"{{"type":"response_item","payload":{{"content":[{}]}}}}"#,
        vec![r#"{"text":"tool output","nested":[1,true,null]}"#; 2048].join(",")
    );
    let cases: &[(&[u8], bool)] = &[
        (large_body.as_bytes(), true),
        (br#"{"ignored":"\ud83d\ude00"} "#, true),
        (br#"{"ignored":"\ud800"}"#, false),
        (br#"{"ignored":[1e999]}"#, false),
        (b"{\"ignored\":\"\xff\"}", false),
        (br#"{"ignored":[1,"#, false),
    ];
    for compressed in [false, true] {
        for &(tail, complete) in cases {
            let dir = tempdir().unwrap();
            let file = rollout_path(dir.path(), PARENT_ID);
            write_jsonl(
                &file,
                &[
                    session_meta(PARENT_ID),
                    turn_context(),
                    token_count(100, 50, 10),
                ],
            );
            let mut contents = fs::read(&file).unwrap();
            contents.extend_from_slice(tail);
            fs::write(&file, contents).unwrap();
            let file = if compressed {
                compress_rollout(&file)
            } else {
                file
            };
            let parsed = parse_codex_file(&file, Some(PARENT_ID.into()))?;
            assert_eq!(parsed.line_offset, 3 + i64::from(complete));
            assert_eq!(
                parsed.observed_bytes as u64,
                fs::metadata(&file).unwrap().len()
            );
            assert_eq!(parsed.token_events.len(), 1);
        }
    }
    Ok(())
}

#[test]
fn test_partial_live_record_is_retried_after_append() -> Result<(), AppError> {
    use std::io::Write;
    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count(100, 50, 10),
        ],
    );
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let next = format!("{}\n", token_count(250, 100, 30));
    let split = next.len() / 2;
    let mut writer = fs::OpenOptions::new().append(true).open(&file).unwrap();
    writer.write_all(&next.as_bytes()[..split]).unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 3);
    assert_unchanged_codex_file_is_skipped(&db, &file)?;
    writer.write_all(&next.as_bytes()[split..]).unwrap();
    writer
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 0);
    assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 4);
    Ok(())
}

/// revert 产生的替换 rollout 是 `<threadId>_<rolloutId>` 双段文件名，
/// root meta 的 id 是原线程 ID（第一个 UUID）、forked_from_id 为空。
/// 旧校验只认末尾 UUID，会把这类文件永久 deferred，其后 resume 追加
/// 的用量全部丢失。
#[test]
fn test_resumed_rollout_meta_id_matching_leading_uuid_is_not_deferred() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = dir.path().join(format!(
        "rollout-2026-08-26T17-18-13-{PARENT_ID}_{CHILD_A_ID}.jsonl"
    ));
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_at(100, 50, 20, "2026-08-26T09:18:20Z"),
        ],
    );

    let parsed = parse_codex_file(&file, thread_id_from_filename(&file))?;

    // 恢复会话没有显式 parent，不应因 ID 不一致被拒
    assert!(
        !matches!(parsed.parent, ParentResolution::Deferred(_)),
        "恢复会话不应被 deferred，实际: {:?}",
        parsed.parent
    );
    assert_eq!(parsed.root_thread_id.as_deref(), Some(CHILD_A_ID));
    assert!(parsed.has_billable_tokens);
    Ok(())
}

/// 分支线程翻页产生的双段文件名带有残留的 forked_from_id，
/// 解析时应被识别为本线程延续（Continuation），并恢复上一页基线计算增量。
#[test]
fn test_parse_codex_file_recognizes_branch_thread_paginated_continuation() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let root = rollout_path(dir.path(), CHILD_A_ID);
    write_jsonl(
        &root,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-08-26T09:18:00Z"),
            token_count_at(50, 25, 10, "2026-08-26T09:18:05Z"),
        ],
    );
    let file = dir.path().join(format!(
        "rollout-2026-08-26T17-18-13-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &file,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_ID),
                "2026-08-26T09:18:20Z",
            ),
            turn_context(),
            token_count_at(100, 50, 20, "2026-08-26T09:18:21Z"),
        ],
    );

    let index = build_rollout_index(&[root, file.clone()]);
    let parsed = parse_codex_file_with_index(&file, thread_id_from_filename(&file), Some(&index))?;

    assert!(
        matches!(parsed.parent, ParentResolution::Continuation { .. }),
        "分支线程续写页应被识别为 Continuation，实际: {:?}",
        parsed.parent
    );
    assert_eq!(parsed.root_thread_id.as_deref(), Some(CHILD_A_PAGE_ID));
    assert_eq!(parsed.meta_thread_id.as_deref(), Some(CHILD_A_ID));
    assert!(parsed.has_billable_tokens);
    // 验证前序基线恢复：100 - 50 = 50 增量
    assert_eq!(parsed.token_events[0].delta.input, 50);
    assert_eq!(parsed.token_events[0].delta.output, 10);
    Ok(())
}

/// 完整同步链路下双段 rollout 的两个 UUID 分工：request_id 用尾部
/// rollout ID（event_index 按物理文件计数，去重键不能换），库里
/// session_id 记前置线程 ID，与会话管理器侧的会话身份同口径。
#[test]
fn test_resumed_rollout_session_id_uses_leading_thread_id() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let file = temp.path().join(format!(
        "rollout-2026-08-26T17-18-13-{PARENT_ID}_{CHILD_A_ID}.jsonl"
    ));
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count(100, 50, 10),
        ],
    );

    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);

    let conn = lock_conn!(db.conn);
    let (request_id, session_id) = conn
        .prepare(
            "SELECT request_id, session_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session'",
        )?
        .query_row([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
    assert_eq!(
        request_id,
        format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:1")
    );
    assert_eq!(session_id, PARENT_ID);
    Ok(())
}

/// 单段文件名的不一致仍要拒收。
#[test]
fn test_single_uuid_filename_meta_mismatch_still_deferred() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    // meta 里写的是另一个线程的 ID —— 这不是 revert 双段文件名能解释的形态
    write_jsonl(
        &file,
        &[
            session_meta(CHILD_B_ID),
            turn_context(),
            token_count_at(1, 1, 1, "2026-07-10T03:00:02Z"),
        ],
    );

    let parsed = parse_codex_file(&file, thread_id_from_filename(&file))?;
    assert!(matches!(parsed.parent, ParentResolution::Deferred(_)));
    Ok(())
}

#[test]
fn test_delta_first_event() {
    let prev = None;
    let current = CumulativeTokens {
        input: 17934,
        cached_input: 9600,
        cache_write_input: 0,
        output: 454,
    };
    let delta = compute_delta(&prev, &current);
    assert_eq!(delta.input, 17934);
    assert_eq!(delta.cached_input, 9600);
    assert_eq!(delta.output, 454);
    assert!(!delta.is_zero());
}

#[test]
fn test_delta_subsequent_event() {
    let prev = Some(CumulativeTokens {
        input: 17934,
        cached_input: 9600,
        cache_write_input: 0,
        output: 454,
    });
    let current = CumulativeTokens {
        input: 36722,
        cached_input: 27904,
        cache_write_input: 0,
        output: 804,
    };
    let delta = compute_delta(&prev, &current);
    assert_eq!(delta.input, 36722 - 17934);
    assert_eq!(delta.cached_input, 27904 - 9600);
    assert_eq!(delta.output, 804 - 454);
}

#[test]
fn test_delta_zero_at_task_boundary() {
    let prev = Some(CumulativeTokens {
        input: 58346,
        cached_input: 46976,
        cache_write_input: 0,
        output: 1045,
    });
    // task 边界：相同的累计值
    let current = CumulativeTokens {
        input: 58346,
        cached_input: 46976,
        cache_write_input: 0,
        output: 1045,
    };
    let delta = compute_delta(&prev, &current);
    assert!(delta.is_zero());
}

#[test]
fn test_delta_saturating_sub() {
    // 异常情况：当前值小于前值（不应发生，但需防护）
    let prev = Some(CumulativeTokens {
        input: 100,
        cached_input: 50,
        cache_write_input: 0,
        output: 30,
    });
    let current = CumulativeTokens {
        input: 80,
        cached_input: 40,
        cache_write_input: 0,
        output: 20,
    };
    let delta = compute_delta(&prev, &current);
    assert_eq!(delta.input, 0);
    assert_eq!(delta.cached_input, 0);
    assert_eq!(delta.output, 0);
    assert!(delta.is_zero());
}

#[test]
fn test_interleaved_counter_lanes_use_exact_last_usage() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let bengal_event = token_count_with_last_at(
        87_709_262,
        83_563_008,
        240_919,
        151_258,
        147_200,
        87,
        "codex_bengalfox",
        "2026-07-10T03:00:03Z",
    );
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_with_last_at(
                76_780_408,
                73_010_432,
                243_036,
                175_074,
                169_728,
                6_827,
                "codex",
                "2026-07-10T03:00:02Z",
            ),
            bengal_event.clone(),
            token_count_with_last_at(
                76_962_538,
                73_180_160,
                243_258,
                182_130,
                169_728,
                222,
                "codex",
                "2026-07-10T03:00:04Z",
            ),
            // Repeated snapshots are notifications, not additional API usage.
            bengal_event,
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| {
            (
                event.delta.input,
                event.delta.cached_input,
                event.delta.output,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        deltas,
        vec![
            (175_074, 169_728, 6_827),
            (151_258, 147_200, 87),
            (182_130, 169_728, 222),
        ]
    );
    assert!(parsed.token_events[3].delta.is_zero());
    Ok(())
}

#[test]
fn test_cross_limit_snapshot_replay_is_not_double_counted() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
            token_count_with_last_at(
                1_000,
                0,
                10,
                100,
                0,
                10,
                "codex_bengalfox",
                "2026-07-10T03:00:03Z",
            ),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100]);
    Ok(())
}

#[test]
fn test_adjacent_replay_burst_across_multiple_sources_is_deduped() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
            token_count_with_last_at(
                1_000,
                0,
                10,
                100,
                0,
                10,
                "codex_bengalfox",
                "2026-07-10T03:00:03Z",
            ),
            token_count_with_last_at(
                1_000,
                0,
                10,
                100,
                0,
                10,
                "codex_spark",
                "2026-07-10T03:00:04Z",
            ),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100]);
    Ok(())
}

#[test]
fn test_cross_source_replay_remains_adjacent_across_non_token_events() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
            turn_context_for_model_at("gpt-5.6-sol", "2026-07-10T03:00:03Z"),
            token_count_with_last_at(
                1_000,
                0,
                10,
                100,
                0,
                10,
                "codex_bengalfox",
                "2026-07-10T03:00:04Z",
            ),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100]);
    Ok(())
}

#[test]
fn test_same_source_repeat_is_deduped_after_another_source_advances() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
            token_count_with_last_at(
                2_000,
                0,
                20,
                100,
                0,
                10,
                "codex_bengalfox",
                "2026-07-10T03:00:03Z",
            ),
            // `codex` has not advanced since its X snapshot, so this is a
            // same-source replay even though another source was interleaved.
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:04Z"),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100, 100]);
    Ok(())
}

#[test]
fn test_stale_cross_source_signature_does_not_swallow_reset() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            // `codex` emits snapshot X.
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:02Z"),
            // X is replayed under another rate-limit source.
            token_count_with_last_at(
                1_000,
                0,
                10,
                100,
                0,
                10,
                "codex_bengalfox",
                "2026-07-10T03:00:03Z",
            ),
            // The original source advances to Y.
            token_count_with_last_at(2_000, 0, 20, 100, 0, 10, "codex", "2026-07-10T03:00:04Z"),
            // A genuine reset later reproduces X. The stale copy retained
            // by `codex_bengalfox` must not classify this as a replay.
            token_count_with_last_at(1_000, 0, 10, 100, 0, 10, "codex", "2026-07-10T03:00:05Z"),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100, 100, 100]);
    Ok(())
}

#[test]
fn test_full_snapshot_dedupe_allows_counter_reset() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let first = token_count_with_last_at(100, 50, 10, 100, 50, 10, "codex", "2026-07-10T03:00:02Z");
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            first.clone(),
            first,
            token_count_with_last_at(200, 100, 20, 100, 50, 10, "codex", "2026-07-10T03:00:04Z"),
            // A restarted counter may legitimately return to an older
            // total after another full snapshot has advanced the source.
            token_count_with_last_at(100, 50, 10, 50, 25, 5, "codex", "2026-07-10T03:00:05Z"),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100, 100, 50]);
    Ok(())
}

#[test]
fn test_cache_write_tokens_are_billed_as_cache_creation() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    // Codex reports cache writes as a subset of input_tokens, disjoint from cached reads.
    let usage = serde_json::json!({
        "input_tokens": 1_000,
        "cached_input_tokens": 300,
        "cache_write_input_tokens": 600,
        "output_tokens": 50,
        "reasoning_output_tokens": 0,
        "total_tokens": 1_050
    });
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": usage, "last_token_usage": usage }
                }
            }),
        ],
    );

    let db = Database::memory()?;
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);

    let conn = lock_conn!(db.conn);
    let fresh_input = crate::services::sql_helpers::fresh_input_sql("");
    let (fresh, cache_read, cache_creation, cache_creation_cost, total_cost): (
        i64,
        i64,
        i64,
        String,
        String,
    ) = conn.query_row(
        &format!(
            "SELECT {fresh_input}, cache_read_tokens, cache_creation_tokens,
                        cache_creation_cost_usd, total_cost_usd
                 FROM proxy_request_logs WHERE data_source = 'codex_session'"
        ),
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    assert_eq!((fresh, cache_read, cache_creation), (100, 300, 600));
    // gpt-5.6-sol: $4 input, $20 output, $0.40 cache read, $5 cache write per MTok.
    assert_eq!(
        cache_creation_cost.parse::<Decimal>().unwrap(),
        "0.003".parse().unwrap()
    );
    assert_eq!(
        total_cost.parse::<Decimal>().unwrap(),
        "0.00452".parse().unwrap()
    );
    Ok(())
}

#[test]
fn test_empty_last_usage_falls_back_to_total() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": 100,
                            "cached_input_tokens": 0,
                            "output_tokens": 10,
                            "reasoning_output_tokens": 0,
                            "total_tokens": 110
                        },
                        "last_token_usage": {}
                    }
                }
            }),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100]);
    Ok(())
}

#[test]
fn test_empty_total_does_not_enable_snapshot_deduplication() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let event = |limit_id: &str, timestamp: &str| {
        serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {},
                    "last_token_usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 0,
                        "output_tokens": 10,
                        "reasoning_output_tokens": 0,
                        "total_tokens": 110
                    }
                },
                "rate_limits": { "limit_id": limit_id }
            }
        })
    };
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            event("codex", "2026-07-10T03:00:02Z"),
            // Without a usable cumulative total, identical per-request
            // usage is not enough evidence that this is a replay.
            event("codex_bengalfox", "2026-07-10T03:00:03Z"),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100, 100]);
    Ok(())
}

#[test]
fn test_total_fallback_uses_session_baseline_across_model_switch() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context_for_model_at("model-a", "2026-07-10T03:00:01Z"),
            token_count_at(100, 50, 10, "2026-07-10T03:00:02Z"),
            turn_context_for_model_at("model-b", "2026-07-10T03:00:03Z"),
            token_count_at(150, 75, 15, "2026-07-10T03:00:04Z"),
        ],
    );

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    let deltas = parsed
        .token_events
        .iter()
        .filter(|event| !event.delta.is_zero())
        .map(|event| event.delta.input)
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![100, 50]);
    Ok(())
}

#[test]
fn test_thread_settings_model_switch_imports_correct_usage_and_cost() -> Result<(), AppError> {
    use std::io::Write;

    let db = Database::memory()?;
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let first_usage = token_count_at(100, 50, 10, "2026-07-10T03:00:02Z");
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            turn_context_for_model_at("gpt-6-astra", "2026-07-10T03:00:01Z"),
            first_usage.clone(),
        ],
    );
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);

    {
        let mut writer = fs::OpenOptions::new().append(true).open(&file).unwrap();
        for event in [
            thread_settings_at(
                serde_json::json!("OpenAI/GPT-6.1-SOL-2026-10-01"),
                "2026-07-10T03:00:03Z",
            ),
            // A model change must not turn a repeated snapshot into usage.
            first_usage,
            token_count_at(150, 75, 15, "2026-07-10T03:00:04Z"),
        ] {
            writeln!(writer, "{event}").unwrap();
        }
    }
    assert_eq!(sync_test_file(&db, &file, &[&file])?.imported, 1);
    assert_unchanged_codex_file_is_skipped(&db, &file)?;

    let conn = lock_conn!(db.conn);
    let mut statement = conn.prepare(
        "SELECT model, request_model, input_tokens, cache_read_tokens, output_tokens,
                    total_cost_usd, request_id
             FROM proxy_request_logs ORDER BY created_at",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows.len(), 2);
    for (row, (model, input, cached, output, cost, index)) in rows.iter().zip([
        ("gpt-6-astra", 100, 50, 10, "0.00105", 1),
        ("gpt-6.1-sol", 50, 25, 5, "0.0001025", 2),
    ]) {
        assert_eq!(
            (&*row.0, &*row.1, row.2, row.3, row.4),
            (model, model, input, cached, output)
        );
        assert_eq!(
            row.5.parse::<Decimal>().unwrap(),
            cost.parse::<Decimal>().unwrap()
        );
        assert_eq!(
            row.6,
            format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:{index}")
        );
    }
    Ok(())
}

#[test]
fn test_thread_settings_model_without_turn_context() -> Result<(), AppError> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    write_jsonl(
        &file,
        &[
            session_meta(PARENT_ID),
            thread_settings_at(serde_json::json!("gpt-6.1-sol"), "2026-07-10T03:00:01Z"),
            token_count(100, 50, 10),
        ],
    );
    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    assert_eq!(parsed.token_events.len(), 1);
    assert_eq!(parsed.token_events[0].model, "gpt-6.1-sol");
    Ok(())
}

#[test]
fn test_thread_settings_model_ignores_invalid_metadata_and_allows_overrides() -> Result<(), AppError>
{
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let mut events = vec![
        session_meta(PARENT_ID),
        thread_settings_at(serde_json::json!("gpt-6.1-sol"), "2026-07-10T03:00:01Z"),
    ];
    for model in [
        serde_json::Value::Null,
        serde_json::json!(42),
        serde_json::json!(""),
        serde_json::json!(" \t "),
    ] {
        events.push(thread_settings_at(model, "2026-07-10T03:00:02Z"));
    }
    events.push(serde_json::json!({
        "type": "event_msg",
        "payload": { "type": "thread_settings_applied", "thread_settings": { "service_tier": "priority" } }
    }));
    // The fast text filter is not sufficient evidence of an applied setting.
    let mut unrelated =
        thread_settings_at(serde_json::json!("wrong-model"), "2026-07-10T03:00:02Z");
    unrelated["type"] = serde_json::json!("response_item");
    events.push(unrelated);
    events.push(token_count(100, 50, 10));
    events.push(turn_context_for_model_at(
        "gpt-6-astra",
        "2026-07-10T03:00:03Z",
    ));
    events.push(token_count_at(150, 75, 15, "2026-07-10T03:00:04Z"));
    let mut explicit_model = token_count_at(200, 100, 20, "2026-07-10T03:00:05Z");
    explicit_model["payload"]["info"]["model"] = serde_json::json!("custom-model");
    events.push(explicit_model);
    write_jsonl(&file, &events);

    let parsed = parse_codex_file(&file, Some(PARENT_ID.to_string()))?;
    assert_eq!(
        parsed
            .token_events
            .iter()
            .map(|event| event.model.as_str())
            .collect::<Vec<_>>(),
        vec!["gpt-6.1-sol", "gpt-6-astra", "custom-model"]
    );
    Ok(())
}

#[test]
fn test_parse_cumulative_tokens_valid() {
    let json: serde_json::Value = serde_json::json!({
        "input_tokens": 17934,
        "cached_input_tokens": 9600,
        "output_tokens": 454,
        "reasoning_output_tokens": 233,
        "total_tokens": 18388
    });
    let tokens = parse_cumulative_tokens(&json).unwrap();
    assert_eq!(tokens.input, 17934);
    assert_eq!(tokens.cached_input, 9600);
    assert_eq!(tokens.output, 454);
}

#[test]
fn test_parse_cumulative_tokens_null() {
    let json = serde_json::Value::Null;
    assert!(parse_cumulative_tokens(&json).is_none());
}

#[test]
fn test_parse_cumulative_tokens_rejects_empty_object_but_accepts_explicit_zero() {
    assert!(parse_cumulative_tokens(&serde_json::json!({})).is_none());

    let tokens = parse_cumulative_tokens(&serde_json::json!({ "input_tokens": 0 }))
        .expect("an explicit zero is valid usage");
    assert_eq!(tokens.input, 0);
    assert_eq!(tokens.cached_input, 0);
    assert_eq!(tokens.output, 0);
}

#[test]
fn test_parse_cumulative_tokens_alt_field_names() {
    // 某些版本可能使用 cache_read_input_tokens 而非 cached_input_tokens
    let json: serde_json::Value = serde_json::json!({
        "input_tokens": 1000,
        "cache_read_input_tokens": 500,
        "output_tokens": 200
    });
    let tokens = parse_cumulative_tokens(&json).unwrap();
    assert_eq!(tokens.cached_input, 500);
}

#[test]
fn test_collect_codex_session_files_nonexistent() {
    let files = collect_codex_session_files(Path::new("/nonexistent/path"));
    assert!(files.is_empty());
}

#[test]
#[serial_test::serial]
fn test_thread_spawn_parent_strips_replay_and_keeps_live_usage() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:06Z"),
            token_count_at(1_300, 1_050, 150, "2026-07-10T03:00:07Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (result.imported, result.skipped, result.deferred),
        (1, 1, false)
    );

    let conn = lock_conn!(db.conn);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:2")],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (300, 150, 50));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_poisoned_replay_cache_does_not_deadlock_parented_sync() -> Result<(), AppError> {
    clear_codex_replay_caches();
    // 模拟持锁断言失败把缓存锁弄中毒。
    let _ = std::thread::spawn(|| {
        let _guard = replay_caches().lock().unwrap();
        panic!("poison the replay cache");
    })
    .join();
    assert!(replay_caches().is_poisoned());

    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:06Z"),
            token_count_at(1_300, 1_050, 150, "2026-07-10T03:00:07Z"),
        ],
    );

    // 放到子线程里跑：一旦回归成自锁，测试按超时失败而不是挂住整个测试进程。
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = sync_test_file(&db, &child, &[&parent, &child])
            .map(|result| (result.imported, result.skipped, result.deferred));
        let _ = tx.send(result);
        drop(temp);
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("parented sync deadlocked on a poisoned replay cache")?;
    assert_eq!(result, (1, 1, false));

    clear_codex_replay_caches();
    assert!(!replay_caches().is_poisoned());
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_filtered_parent_events_use_subsequence_prefix_alignment() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:02Z"),
            token_count_at(300, 150, 30, "2026-07-10T03:00:03Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
            token_count_at(100, 50, 10, "2026-07-10T03:00:06Z"),
            token_count_at(300, 150, 30, "2026-07-10T03:00:07Z"),
            token_count_at(450, 220, 45, "2026-07-10T03:00:08Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!((result.imported, result.skipped), (1, 2));
    Ok(())
}

/// 文件系统给不出文件身份时（如 Windows 访问 \\wsl.localhost）父时间线按设计不进缓存，
/// 这类环境只校验结果、不校验缓存复用。
fn parent_cache_supported(path: &Path) -> bool {
    let supported = ParentFileStamp::from_file(&fs::File::open(path).unwrap()).is_some();
    if !supported {
        eprintln!(
            "no file identity for {}; skipping parent cache assertions",
            path.display()
        );
    }
    supported
}

/// 能不能区分「大小、修改时间都相同的两个文件」：要文件系统提供文件身份。
/// Windows 访问 9P 共享（`\\wsl.localhost\...`）时没有文件 ID，只比较时间和大小。
fn same_size_same_mtime_replacement_detectable(path: &Path) -> bool {
    let Some(stamp) = ParentFileStamp::from_file(&fs::File::open(path).unwrap()) else {
        eprintln!("no file stamp for {}; skipping", path.display());
        return false;
    };
    #[cfg(windows)]
    if stamp.identity.is_none() {
        eprintln!(
            "no file ID for {}; same-size same-mtime replacement is undetectable, skipping",
            path.display()
        );
        return false;
    }
    let _ = stamp;
    true
}

/// 缓存是进程级的，`serial` 只互斥带标记的测试：不带标记的测试（如续写页
/// 解析会经 `load_parent_timeline` 缓存前序页）可能同时写入别的条目。
/// 数条目时只数本测试临时目录下的键，不数全局总数。
fn cached_parent_timelines_under(dir: &Path) -> usize {
    replay_caches()
        .lock()
        .unwrap()
        .parent_timelines
        .keys()
        .filter(|path| path.starts_with(dir))
        .count()
}

#[test]
#[serial_test::serial]
fn test_parent_malformed_lines_do_not_defer_child_or_prevent_warm_skip() -> Result<(), AppError> {
    let cases: &[(&[u8], &[u8], bool)] = &[
        // A bad record must not hide subsequent valid records, including
        // a complete final record without a trailing newline.
        (b"\xff\n", b"", true),
        (b"", b"\xff\n", false),
        (b"", b"\xff", false),
        // Keep the existing malformed-JSON/blank-line behavior too.
        (b"{malformed\n \t\r\n", b"{malformed", false),
    ];
    for compressed in [false, true] {
        for &(prefix, suffix, remove_final_newline) in cases {
            clear_codex_replay_caches();
            let db = Database::memory()?;
            let temp = tempdir().unwrap();
            let (parent, child) = retryable_parent_and_child(temp.path());
            append_jsonl(&parent, turn_context_at("2026-07-10T03:00:10Z"));
            let mut contents = fs::read(&parent).unwrap();
            if remove_final_newline {
                assert_eq!(contents.pop(), Some(b'\n'));
            }
            fs::write(&parent, [prefix, contents.as_slice(), suffix].concat()).unwrap();
            let parent = if compressed {
                compress_rollout(&parent)
            } else {
                parent
            };

            let result = sync_test_file(&db, &child, &[&parent, &child])?;
            assert_eq!(
                (result.imported, result.skipped, result.deferred),
                (1, 1, false)
            );
            let timeline = load_parent_timeline(&parent).unwrap();
            assert_eq!(timeline.events.len(), 1);
            assert_eq!(
                timeline.max_timestamp,
                Some("2026-07-10T03:00:10Z".parse::<DateTime<Utc>>().unwrap())
            );
            for _ in 0..3 {
                let warm = sync_test_file(&db, &child, &[&parent, &child])?;
                assert_eq!((warm.imported, warm.deferred), (0, false));
            }
            assert_eq!(codex_parse_count(&child), 1);
            if parent_cache_supported(&parent) {
                assert!(Arc::ptr_eq(
                    &timeline,
                    &load_parent_timeline(&parent).unwrap()
                ));
            }
            let conn = lock_conn!(db.conn);
            let usage: (i64, i64, i64) = conn.query_row(
                "SELECT input_tokens, cache_read_tokens, output_tokens FROM proxy_request_logs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(usage, (300, 150, 50));
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_parent_rollout_is_cached_once_across_fork_cutoffs() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:10Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );

    let early = "2026-07-10T03:00:05Z".parse::<DateTime<Utc>>().unwrap();
    let late = "2026-07-10T03:00:15Z".parse::<DateTime<Utc>>().unwrap();
    assert_eq!(parent_signatures_before(&parent, early).unwrap().len(), 1);
    let first_timeline = parent_cache_supported(&parent)
        .then(|| Arc::clone(&replay_caches().lock().unwrap().parent_timelines[&parent].timeline));
    assert_eq!(parent_signatures_before(&parent, late).unwrap().len(), 2);

    match first_timeline {
        Some(first_timeline) => {
            assert_eq!(cached_parent_timelines_under(temp.path()), 1);
            let caches = replay_caches().lock().unwrap();
            assert!(Arc::ptr_eq(
                &first_timeline,
                &caches.parent_timelines[&parent].timeline
            ));
        }
        None => assert_eq!(cached_parent_timelines_under(temp.path()), 0),
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_parent_rollout_cache_invalidates_after_append() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let cutoff = "2026-07-10T03:00:15Z".parse::<DateTime<Utc>>().unwrap();
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );
    assert_eq!(parent_signatures_before(&parent, cutoff).unwrap().len(), 1);

    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:10Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );
    assert_eq!(parent_signatures_before(&parent, cutoff).unwrap().len(), 2);

    let expected_entries = usize::from(parent_cache_supported(&parent));
    assert_eq!(cached_parent_timelines_under(temp.path()), expected_entries);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_parent_rollout_content_error_cache_preserves_open_errors() {
    clear_codex_replay_caches();
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let cutoff = "2026-07-10T03:00:05Z".parse::<DateTime<Utc>>().unwrap();
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_without_timestamp(100, 50, 10),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );

    let first_error = parent_signatures_before(&parent, cutoff).unwrap_err();
    assert!(first_error
        .reason
        .contains("token_count 缺少有效 timestamp"));
    assert!(first_error.cacheable);
    let cached_timeline =
        || Arc::clone(&replay_caches().lock().unwrap().parent_timelines[&parent].timeline);
    let first_timeline = parent_cache_supported(&parent).then(cached_timeline);

    let second_error = parent_signatures_before(&parent, cutoff).unwrap_err();
    assert_eq!(second_error, first_error);
    if let Some(first_timeline) = first_timeline {
        assert!(Arc::ptr_eq(&first_timeline, &cached_timeline()));
    }

    fs::remove_file(&parent).unwrap();
    let open_error = parent_signatures_before(&parent, cutoff).unwrap_err();
    assert!(open_error.reason.contains("无法打开父 rollout"));
    assert!(!open_error.cacheable);
}

#[test]
#[serial_test::serial]
fn test_parent_rollout_nanosecond_cutoffs_are_exact() {
    clear_codex_replay_caches();
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:00.000000500Z"),
            turn_context_at("2026-07-10T03:00:00.000000900Z"),
        ],
    );

    let before = "2026-07-10T03:00:00.000000300Z"
        .parse::<DateTime<Utc>>()
        .unwrap();
    let after = "2026-07-10T03:00:00.000000700Z"
        .parse::<DateTime<Utc>>()
        .unwrap();
    assert!(parent_signatures_before(&parent, before)
        .unwrap()
        .is_empty());
    assert_eq!(parent_signatures_before(&parent, after).unwrap().len(), 1);
    let expected_entries = usize::from(parent_cache_supported(&parent));
    assert_eq!(cached_parent_timelines_under(temp.path()), expected_entries);
}

#[cfg(any(unix, windows))]
#[test]
fn test_parent_file_stamp_distinguishes_same_size_same_mtime_files() {
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let replacement = temp.path().join("replacement.jsonl");
    let values = [session_meta(PARENT_ID), token_count(100, 50, 10)];
    write_jsonl(&parent, &values);
    write_jsonl(&replacement, &values);
    if !same_size_same_mtime_replacement_detectable(&parent) {
        return;
    }
    let original_file = fs::File::open(&parent).unwrap();
    let original_metadata = original_file.metadata().unwrap();
    let replacement_file = fs::OpenOptions::new()
        .write(true)
        .open(&replacement)
        .unwrap();
    replacement_file
        .set_times(fs::FileTimes::new().set_modified(original_metadata.modified().unwrap()))
        .unwrap();
    let original_stamp = ParentFileStamp::from_file(&original_file).unwrap();
    let replacement_stamp = ParentFileStamp::from_file(&replacement_file).unwrap();
    assert_eq!(
        (original_stamp.size, original_stamp.modified_nanos),
        (replacement_stamp.size, replacement_stamp.modified_nanos)
    );
    assert_ne!(original_stamp, replacement_stamp);
}

#[test]
#[serial_test::serial]
fn test_empty_fork_imports_no_parent_usage() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:02Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
            token_count_at(100, 50, 10, "2026-07-10T03:00:06Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:07Z"),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:08Z",
                "type": "event_msg",
                "payload": { "type": "thread_settings_applied" }
            }),
        ],
    );

    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (result.imported, result.skipped, result.deferred),
        (0, 2, false)
    );
    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 0);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_conflicting_explicit_parents_are_deferred() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &child,
        &[
            session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                Some(CHILD_B_ID),
                "2026-07-10T03:00:05Z",
            ),
            token_count_at(100, 50, 10, "2026-07-10T03:00:06Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&child])?;
    assert!(result.deferred);
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_parent_future_signature_cannot_extend_replay_prefix() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:06Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:05Z"),
            token_count_at(200, 100, 20, "2026-07-10T03:00:07Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (result.imported, result.skipped, result.deferred),
        (1, 0, false)
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_duplicate_parent_replica_with_missing_timestamp_rejects_both_orders() -> Result<(), AppError>
{
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let sessions_parent = temp
        .path()
        .join("sessions")
        .join(format!("rollout-2026-07-10T03-00-01-{PARENT_ID}.jsonl"));
    let archived_parent = temp
        .path()
        .join("archived_sessions")
        .join(format!("rollout-2026-07-10T03-00-01-{PARENT_ID}.jsonl"));
    fs::create_dir_all(sessions_parent.parent().unwrap()).unwrap();
    fs::create_dir_all(archived_parent.parent().unwrap()).unwrap();

    write_jsonl(
        &archived_parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    write_jsonl(
        &sessions_parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            token_count(500, 400, 50),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );

    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
            token_count_at(1_500, 1_000, 200, "2026-07-10T03:00:06Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&archived_parent, &sessions_parent, &child])?;
    assert!(
        result.deferred,
        "任一副本含有无效时间戳时，必须保留无效时间戳保护并 deferred"
    );

    let result_swapped =
        sync_test_file(&db, &child, &[&sessions_parent, &archived_parent, &child])?;
    assert!(
        result_swapped.deferred,
        "候选顺序不应改变无效时间戳的防御结果"
    );

    Ok(())
}

/// paginated 线程翻页后，续写页是 `<threadId>_<rolloutId>` 双段文件名，
/// 首页从此不再增长。之后 spawn 的子代理 fork 时刻晚于首页最后事件，
/// 父时间线必须把首页与续写页拼起来看，否则子代理会被永久 deferred。
#[test]
#[serial_test::serial]
fn test_paginated_parent_pages_are_merged_before_fork_cutoff() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent_root = rollout_path(temp.path(), PARENT_ID);
    let parent_page = temp.path().join(format!(
        "rollout-2026-07-10T03-00-20-{PARENT_ID}_{PARENT_PAGE_ID}.jsonl"
    ));
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &parent_root,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
        ],
    );
    write_jsonl(
        &parent_page,
        &[
            session_meta_at(PARENT_ID, None, None, "2026-07-10T03:00:20Z"),
            token_count_at(2_000, 1_800, 200, "2026-07-10T03:00:21Z"),
            turn_context_at("2026-07-10T03:00:40Z"),
        ],
    );
    write_jsonl(
        &child,
        &[
            session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                Some(PARENT_ID),
                "2026-07-10T03:00:30Z",
            ),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:31Z"),
            token_count_at(2_000, 1_800, 200, "2026-07-10T03:00:32Z"),
            token_count_at(2_300, 1_950, 250, "2026-07-10T03:00:33Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&parent_root, &parent_page, &child])?;
    assert_eq!(
        (result.imported, result.skipped, result.deferred),
        (1, 2, false)
    );

    let conn = lock_conn!(db.conn);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:3")],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (300, 150, 50));
    Ok(())
}

/// #8161: 分支线程翻页产生的续写页（`<branchId>_<pageId>` 双段文件名），
/// 其 session_meta 仍保留所属分支会话的 forked_from_id，但翻页时间晚于原始父线程末尾。
/// 续写页应被识别为分支线程自身的延续，不剔除父重放（新格式续写页不复制父用量），
/// 避免因父线程无法达到翻页时刻而被永久 deferred。
#[test]
#[serial_test::serial]
fn test_paginated_continuation_pages_for_forked_branch_thread_are_imported() -> Result<(), AppError>
{
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    // 1. 父线程在 03:00:20 结束
    let parent_root = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &parent_root,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );

    // 2. 分支线程 CHILD_A 首页在 03:00:15 fork 自 PARENT_ID
    // 首页重放了父线程的 1000 tokens，并新产生了 500 tokens
    let branch_root = temp
        .path()
        .join(format!("rollout-2026-07-10T03-00-15-{CHILD_A_ID}.jsonl"));
    write_jsonl(
        &branch_root,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:15Z"),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:16Z"),
            token_count_at(1_500, 1_300, 150, "2026-07-10T03:00:20Z"),
        ],
    );

    // 3. 分支线程 CHILD_A 翻页，产生双段续写页，时间戳为 03:01:00（远晚于父线程末尾 03:00:10）
    // session_meta 仍带 forked_from_id: PARENT_ID，新产生 200 tokens
    let branch_page = temp.path().join(format!(
        "rollout-2026-07-10T03-01-00-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:01:00Z",
            ),
            turn_context_at("2026-07-10T03:01:01Z"),
            token_count_at(200, 150, 50, "2026-07-10T03:01:05Z"),
        ],
    );

    let files = [
        parent_root.as_path(),
        branch_root.as_path(),
        branch_page.as_path(),
    ];

    // 同步分支线程首页：成功导入 1 条（500 tokens），跳过 1 条重放（1000 tokens）
    let root_result = sync_test_file(&db, &branch_root, &files)?;
    assert_eq!(
        (
            root_result.imported,
            root_result.skipped,
            root_result.deferred
        ),
        (1, 1, false)
    );

    // 同步分支线程续写页：识别为自身延续，直接导入 1 条（200 tokens），绝不 deferred！
    let page_result = sync_test_file(&db, &branch_page, &files)?;
    assert_eq!(
        (
            page_result.imported,
            page_result.skipped,
            page_result.deferred
        ),
        (1, 0, false)
    );

    // 验证数据库：session_id 统一为 CHILD_A_ID，且两条记录用量准确
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT request_id, session_id, input_tokens, output_tokens
             FROM proxy_request_logs
             WHERE session_id = ?1
             ORDER BY created_at ASC",
    )?;
    let rows = stmt
        .query_map([CHILD_A_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(rows.len(), 2);
    // 首页记录
    assert_eq!(
        rows[0].0,
        format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:2")
    );
    assert_eq!(rows[0].1, CHILD_A_ID);
    assert_eq!(rows[0].2, 500); // 1500 - 1000
    assert_eq!(rows[0].3, 50); // 150 - 100
                               // 续写页记录
    assert_eq!(
        rows[1].0,
        format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_PAGE_ID}:1")
    );
    assert_eq!(rows[1].1, CHILD_A_ID);
    assert_eq!(rows[1].2, 200); // 1700 - 1500
    assert_eq!(rows[1].3, 50); // 200 - 150

    Ok(())
}

/// 测试分支线程多次翻页（第 3 页续写页，其 history_base 指向上一页 rollout ID），
/// 以及在分支线程多页续写之后再派生子代理的完整链条。
#[test]
#[serial_test::serial]
fn test_paginated_continuation_multiple_pages_and_subagent_on_branch() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    let parent_root = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &parent_root,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:20Z"),
        ],
    );

    // 分支线程 CHILD_A 首页
    let branch_root = temp
        .path()
        .join(format!("rollout-2026-07-10T03-00-15-{CHILD_A_ID}.jsonl"));
    write_jsonl(
        &branch_root,
        &[
            session_meta_at(CHILD_A_ID, Some(PARENT_ID), None, "2026-07-10T03:00:15Z"),
            turn_context(),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:16Z"),
            token_count_at(1_500, 1_300, 150, "2026-07-10T03:00:20Z"),
        ],
    );

    // 分支线程第 2 页（续写页 1）
    let branch_page_1 = temp.path().join(format!(
        "rollout-2026-07-10T03-01-00-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page_1,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:01:00Z",
            ),
            turn_context_at("2026-07-10T03:01:01Z"),
            token_count_at(1_700, 1_450, 200, "2026-07-10T03:01:05Z"),
        ],
    );

    // 分支线程第 3 页（续写页 2），history_base 指向第 2 页的 rollout ID
    let branch_page_2 = temp.path().join(format!(
        "rollout-2026-07-10T03-02-00-{CHILD_A_ID}_{CHILD_A_PAGE_2_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page_2,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_PAGE_ID),
                "2026-07-10T03:02:00Z",
            ),
            turn_context_at("2026-07-10T03:02:01Z"),
            token_count_at(2_000, 1_650, 250, "2026-07-10T03:02:10Z"),
            turn_context_at("2026-07-10T03:02:30Z"),
        ],
    );

    // 在分支线程翻页写完后，从分支线程派生子代理 CHILD_B（fork 时刻 03:02:30）
    let subagent = temp
        .path()
        .join(format!("rollout-2026-07-10T03-02-30-{CHILD_B_ID}.jsonl"));
    write_jsonl(
        &subagent,
        &[
            session_meta_at(
                CHILD_B_ID,
                Some(CHILD_A_ID),
                Some(CHILD_A_ID),
                "2026-07-10T03:02:30Z",
            ),
            turn_context(),
            // 重放 CHILD_A 从头到尾（包括多页）的 token
            token_count_at(1_000, 900, 100, "2026-07-10T03:02:31Z"),
            token_count_at(1_500, 1_300, 150, "2026-07-10T03:02:32Z"),
            token_count_at(1_700, 1_450, 200, "2026-07-10T03:02:33Z"),
            token_count_at(2_000, 1_650, 250, "2026-07-10T03:02:34Z"),
            // 子代理自己消耗 150 tokens
            token_count_at(2_150, 1_750, 280, "2026-07-10T03:02:40Z"),
        ],
    );

    let files = [
        parent_root.as_path(),
        branch_root.as_path(),
        branch_page_1.as_path(),
        branch_page_2.as_path(),
        subagent.as_path(),
    ];

    // 先同步分支线程首页：成功导入 500 tokens（跳过父 1000 重放）
    let r_res = sync_test_file(&db, &branch_root, &files)?;
    assert_eq!(
        (r_res.imported, r_res.skipped, r_res.deferred),
        (1, 1, false)
    );

    // 验证续写页 1 成功导入：基线继承首页 1500，增量仅为 200（1700 - 1500）
    let p1_res = sync_test_file(&db, &branch_page_1, &files)?;
    assert_eq!((p1_res.imported, p1_res.deferred), (1, false));

    // 验证续写页 2 成功导入：基线继承第 2 页 1700，增量仅为 300（2000 - 1700）
    let p2_res = sync_test_file(&db, &branch_page_2, &files)?;
    assert_eq!((p2_res.imported, p2_res.deferred), (1, false));

    // 验证子代理解析父线程 CHILD_A 的时间线时，能合并 CHILD_A 的首页 + 两页续写页，
    // 成功达到 03:02:30 并剔除全部 4 条重放，导入子代理自身的 1 条用量（150 tokens）
    let sub_res = sync_test_file(&db, &subagent, &files)?;
    assert_eq!(
        (sub_res.imported, sub_res.skipped, sub_res.deferred),
        (1, 4, false)
    );

    let conn = lock_conn!(db.conn);
    // 精确断言续写页 1 的入库用量（防止将 1700 历史累计值算为增量）
    let p1_usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!(
            "{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_PAGE_ID}:1"
        )],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(p1_usage, (200, 150, 50));

    // 精确断言续写页 2 的入库用量（防止将 2000 历史累计值算为增量）
    let p2_usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!(
            "{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_PAGE_2_ID}:1"
        )],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(p2_usage, (300, 200, 50));

    // 断言子代理用量
    let sub_usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_B_ID}:5")],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(sub_usage, (150, 100, 30));

    // 精确断言分支线程 CHILD_A 的全生命周期总 input 用量：
    // 首页 500 + 第 2 页 200 + 第 3 页 300 = 1000 tokens
    let branch_total_input: i64 = conn.query_row(
        "SELECT sum(input_tokens) FROM proxy_request_logs WHERE session_id = ?1",
        [CHILD_A_ID],
        |row| row.get(0),
    )?;
    assert_eq!(branch_total_input, 1_000);

    Ok(())
}

/// 确保防御性：单段文件名的新 fork 首页即使其 history_base 指向父会话某页，
/// 也不会被误判为延续页，父线程未达到 cutoff 时依然必须被 deferred。
#[test]
#[serial_test::serial]
fn test_single_segment_fork_is_not_treated_as_continuation() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    let parent_root = rollout_path(temp.path(), PARENT_ID);
    write_jsonl(
        &parent_root,
        &[
            session_meta(PARENT_ID),
            token_count_at(1_000, 900, 100, "2026-07-10T03:00:01Z"),
            // 父线程在 03:00:05 结束
            turn_context_at("2026-07-10T03:00:05Z"),
        ],
    );

    // 子会话首页 fork 时刻是 03:00:20（晚于父线程结束时间 03:00:05）
    // 单段文件名，history_base 指向 PARENT_ID（父会话某页）
    let child = temp
        .path()
        .join(format!("rollout-2026-07-10T03-00-20-{CHILD_A_ID}.jsonl"));
    write_jsonl(
        &child,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(PARENT_ID),
                "2026-07-10T03:00:20Z",
            ),
            turn_context_at("2026-07-10T03:00:21Z"),
            token_count_at(1_200, 1_000, 120, "2026-07-10T03:00:25Z"),
        ],
    );

    let files = [parent_root.as_path(), child.as_path()];
    let result = sync_test_file(&db, &child, &files)?;
    assert!(
        result.deferred,
        "单段文件名的新 fork 首页在父线程未达到 cutoff 时必须被 deferred"
    );

    Ok(())
}

/// 对照测试 1：带有有效 last_token_usage 的续写页直接使用 last 计费，无需回溯基线。
#[test]
#[serial_test::serial]
fn test_paginated_continuation_with_valid_last_token_usage() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    let branch_page = temp.path().join(format!(
        "rollout-2026-07-10T03-01-00-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:01:00Z",
            ),
            turn_context_at("2026-07-10T03:01:01Z"),
            // 带有显式的 last_token_usage
            token_count_with_last_at(
                1_700,
                1_450,
                200,
                120,
                80,
                30,
                "codex",
                "2026-07-10T03:01:05Z",
            ),
        ],
    );

    let files = [branch_page.as_path()];
    let result = sync_test_file(&db, &branch_page, &files)?;
    assert_eq!((result.imported, result.deferred), (1, false));

    let conn = lock_conn!(db.conn);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!(
            "{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_PAGE_ID}:1"
        )],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (120, 80, 30));
    Ok(())
}

/// 对照测试 2：防御机制——当续写页缺少 last_token_usage 且上一分段缺失无法恢复基线时，
/// 必须安全 deferred，绝对不能将累计总值直接当作增量入库。
#[test]
#[serial_test::serial]
fn test_paginated_continuation_deferred_when_prev_segment_missing() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    // 仅存在续写页，其前序首页文件缺失
    let branch_page = temp.path().join(format!(
        "rollout-2026-07-10T03-01-00-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                Some(PARENT_ID),
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:01:00Z",
            ),
            turn_context_at("2026-07-10T03:01:01Z"),
            token_count_at(1_700, 1_450, 200, "2026-07-10T03:01:05Z"),
        ],
    );

    let files = [branch_page.as_path()];
    let result = sync_test_file(&db, &branch_page, &files)?;
    assert!(
        result.deferred,
        "前序分段缺失且无 last_token_usage 时必须 deferred"
    );

    // 验证数据库中绝无错误写入
    let count: i64 = {
        let conn = lock_conn!(db.conn);
        conn.query_row(
            "SELECT count(*) FROM proxy_request_logs WHERE session_id = ?1",
            [CHILD_A_ID],
            |row| row.get(0),
        )?
    };
    assert_eq!(count, 0, "缺失前序基线时绝不应有数据入库");

    // 验证游标未推进
    let sync_state = get_sync_state(&db, &branch_page.to_string_lossy())?;
    assert_eq!(sync_state, (0, 0), "deferred 时游标不应推进");
    Ok(())
}

/// 对照测试 3：续写页计数器重置场景——新页 total_token_usage 从较小数值起算，
/// 自动识别为计数器重置，以当前值作为增量计费，未被饱和减法吞为 0。
#[test]
#[serial_test::serial]
fn test_paginated_continuation_with_counter_reset() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();

    // 首页结束于累计 1500
    let branch_root = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &branch_root,
        &[
            session_meta(CHILD_A_ID),
            token_count_at(1_500, 1_300, 150, "2026-07-10T03:00:20Z"),
        ],
    );

    // 续写页计数器重置为 180 (180 < 1500)
    let branch_page = temp.path().join(format!(
        "rollout-2026-07-10T03-01-00-{CHILD_A_ID}_{CHILD_A_PAGE_ID}.jsonl"
    ));
    write_jsonl(
        &branch_page,
        &[
            paginated_session_meta_at(
                CHILD_A_ID,
                None,
                None,
                Some(CHILD_A_ID),
                "2026-07-10T03:01:00Z",
            ),
            turn_context_at("2026-07-10T03:01:01Z"),
            token_count_at(180, 100, 25, "2026-07-10T03:01:05Z"),
        ],
    );

    let files = [branch_root.as_path(), branch_page.as_path()];
    let root_res = sync_test_file(&db, &branch_root, &files)?;
    assert_eq!((root_res.imported, root_res.deferred), (1, false));

    let page_res = sync_test_file(&db, &branch_page, &files)?;
    assert_eq!((page_res.imported, page_res.deferred), (1, false));

    let conn = lock_conn!(db.conn);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
        [format!(
            "{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_PAGE_ID}:1"
        )],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (180, 100, 25), "计数器重置时应以新页数值作为增量");
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_missing_parent_is_deferred_and_recovered_without_child_change() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let parent = rollout_path(temp.path(), PARENT_ID);
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &child,
        &[
            session_meta_at(CHILD_A_ID, None, Some(PARENT_ID), "2026-07-10T03:00:05Z"),
            token_count_at(900, 400, 90, "2026-07-10T03:00:06Z"),
        ],
    );

    let deferred = sync_test_file(&db, &child, &[&child])?;
    assert!(deferred.deferred);
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));

    write_jsonl(
        &parent,
        &[
            session_meta(PARENT_ID),
            token_count_at(100, 50, 10, "2026-07-10T03:00:01Z"),
            turn_context_at("2026-07-10T03:00:10Z"),
        ],
    );
    let recovered = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!((recovered.imported, recovered.deferred), (1, false));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_skips_child_body_until_parent_append() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    let cached = parent_cache_supported(&parent);

    for _ in 0..4 {
        assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    }
    if cached {
        assert_eq!(codex_parse_count(&child), 1);
    }
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));

    append_jsonl(&parent, turn_context_at("2026-07-10T03:00:10Z"));
    let recovered = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (1, 1, false)
    );
    if cached {
        assert_eq!(codex_parse_count(&child), 2);
    }
    assert_eq!(sync_test_file(&db, &child, &[&parent, &child])?.imported, 0);
    if cached {
        assert_eq!(codex_parse_count(&child), 2);
    }
    let conn = lock_conn!(db.conn);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens FROM proxy_request_logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (300, 150, 50));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_invalidates_when_child_changes() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    let cached = parent_cache_supported(&parent);
    assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    append_jsonl(
        &child,
        token_count_at(1_600, 1_200, 200, "2026-07-10T03:00:08Z"),
    );
    for _ in 0..3 {
        assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    }
    if cached {
        assert_eq!(codex_parse_count(&child), 2);
    }
    append_jsonl(&parent, turn_context_at("2026-07-10T03:00:10Z"));
    let recovered = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (2, 1, false)
    );
    if cached {
        assert_eq!(codex_parse_count(&child), 3);
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_tracks_added_and_removed_parent_candidates() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    let cached = parent_cache_supported(&parent);
    assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    let archived_dir = temp.path().join("archived_sessions");
    fs::create_dir(&archived_dir).unwrap();
    let archived_parent = rollout_path(&archived_dir, PARENT_ID);
    fs::copy(&parent, &archived_parent).unwrap();
    append_jsonl(&archived_parent, turn_context_at("2026-07-10T03:00:10Z"));
    assert!(sync_test_file(&db, &child, &[&parent, &archived_parent, &child])?.deferred);
    if cached {
        assert_eq!(codex_parse_count(&child), 2, "new candidate must retry");
    }
    assert!(sync_test_file(&db, &child, &[&parent, &archived_parent, &child])?.deferred);
    if cached {
        assert_eq!(codex_parse_count(&child), 2);
    }
    fs::remove_file(&parent).unwrap();
    let recovered = sync_test_file(&db, &child, &[&archived_parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (1, 1, false)
    );
    if cached {
        assert_eq!(codex_parse_count(&child), 3, "removed candidate must retry");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_does_not_cache_unknown_parent_metadata() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    let parent_contents = fs::read(&parent).unwrap();
    fs::remove_file(&parent).unwrap();
    // The index can outlive a file removed during a pass. An inaccessible
    // candidate must retry, rather than cache an unknown stamp forever.
    for _ in 0..2 {
        assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    }
    assert_eq!(codex_parse_count(&child), 2);
    fs::write(&parent, parent_contents).unwrap();
    append_jsonl(&parent, turn_context_at("2026-07-10T03:00:10Z"));
    assert!(!sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    assert_eq!(codex_parse_count(&child), 3);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_detects_same_size_same_mtime_parent_replacement() -> Result<(), AppError>
{
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    if !same_size_same_mtime_replacement_detectable(&parent) {
        return Ok(());
    }
    assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    let metadata = fs::metadata(&parent).unwrap();
    let replacement = temp.path().join("replacement.jsonl");
    let contents = fs::read_to_string(&parent)
        .unwrap()
        .replace("03:00:02Z", "03:00:10Z");
    fs::write(&replacement, contents).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    fs::rename(&replacement, &parent).unwrap();
    let replacement_metadata = fs::metadata(&parent).unwrap();
    assert_eq!(metadata.len(), replacement_metadata.len());
    assert_eq!(
        metadata.modified().unwrap(),
        replacement_metadata.modified().unwrap()
    );
    let recovered = sync_test_file(&db, &child, &[&parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (1, 1, false)
    );
    assert_eq!(codex_parse_count(&child), 2);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_retries_parent_decompression_errors() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    let compressed_parent = PathBuf::from(format!("{}.zst", parent.display()));
    fs::remove_file(&parent).unwrap();
    fs::write(&compressed_parent, b"invalid zstd bytes").unwrap();
    // Opening/statting this file succeeds; decoding its body fails. That
    // transient failure must not be suppressed by an unchanged fingerprint.
    for _ in 0..2 {
        assert!(sync_test_file(&db, &child, &[&compressed_parent, &child])?.deferred);
    }
    assert_eq!(codex_parse_count(&child), 2);
    assert!(!replay_caches()
        .lock()
        .unwrap()
        .parent_timelines
        .contains_key(&compressed_parent));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_parent_decompression_error_after_valid_prefix_does_not_cache_partial_timeline(
) -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    append_jsonl(&parent, turn_context_at("2026-07-10T03:00:10Z"));
    let mut contents = fs::read(&parent).unwrap();
    // Fill the rollout reader's buffer before it reaches a corrupt second
    // frame, so valid token events are read before decompression fails.
    contents.extend(vec![b'\n'; 2 << 20]);
    fs::write(&parent, contents).unwrap();
    let compressed_parent = compress_rollout(&parent);
    let valid_frame = fs::read(&compressed_parent).unwrap();
    let mut corrupt_frames = valid_frame.clone();
    corrupt_frames.extend_from_slice(b"invalid zstd bytes");
    fs::write(&compressed_parent, corrupt_frames).unwrap();
    let mut reader = codex_rollout_file::open_reader(&compressed_parent).unwrap();
    let mut first_line = Vec::new();
    assert!(reader.read_until(b'\n', &mut first_line).unwrap() > 0);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&first_line).unwrap(),
        session_meta(PARENT_ID)
    );
    drop(reader);

    for _ in 0..2 {
        let error = load_parent_timeline(&compressed_parent).unwrap_err();
        assert!(!error.cacheable);
        assert!(error.reason.contains("无法读取父 rollout"));
        assert!(sync_test_file(&db, &child, &[&compressed_parent, &child])?.deferred);
        assert!(!replay_caches()
            .lock()
            .unwrap()
            .parent_timelines
            .contains_key(&compressed_parent));
    }
    assert_eq!(codex_parse_count(&child), 2);
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));

    fs::write(&compressed_parent, valid_frame).unwrap();
    let recovered = sync_test_file(&db, &child, &[&compressed_parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (1, 1, false)
    );
    assert_eq!(codex_parse_count(&child), 3);
    assert_eq!(
        sync_test_file(&db, &child, &[&compressed_parent, &child])?.imported,
        0
    );
    assert_eq!(codex_parse_count(&child), 3);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_retryable_pending_detects_same_size_same_mtime_child_replacement() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let (parent, child) = retryable_parent_and_child(temp.path());
    if !same_size_same_mtime_replacement_detectable(&child) {
        return Ok(());
    }
    assert!(sync_test_file(&db, &child, &[&parent, &child])?.deferred);
    let alternate_parent = rollout_path(temp.path(), CHILD_B_ID);
    fs::write(
        &alternate_parent,
        fs::read_to_string(&parent)
            .unwrap()
            .replace(PARENT_ID, CHILD_B_ID),
    )
    .unwrap();
    append_jsonl(&alternate_parent, turn_context_at("2026-07-10T03:00:10Z"));

    let metadata = fs::metadata(&child).unwrap();
    let replacement = temp.path().join("replacement.jsonl");
    fs::write(
        &replacement,
        fs::read_to_string(&child)
            .unwrap()
            .replace(PARENT_ID, CHILD_B_ID),
    )
    .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    fs::rename(&replacement, &child).unwrap();
    let replacement_metadata = fs::metadata(&child).unwrap();
    assert_eq!(metadata.len(), replacement_metadata.len());
    assert_eq!(
        metadata.modified().unwrap(),
        replacement_metadata.modified().unwrap()
    );
    let recovered = sync_test_file(&db, &child, &[&parent, &alternate_parent, &child])?;
    assert_eq!(
        (recovered.imported, recovered.skipped, recovered.deferred),
        (1, 1, false)
    );
    assert_eq!(codex_parse_count(&child), 2);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_billable_file_without_meta_is_deferred_without_cursor() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(&child, &[turn_context(), token_count(100, 50, 10)]);

    let result = sync_test_file(&db, &child, &[&child])?;
    assert!(result.deferred);
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?, (0, 0));

    std::thread::sleep(std::time::Duration::from_millis(2));
    write_jsonl(
        &child,
        &[
            turn_context(),
            token_count(100, 50, 10),
            session_meta_at(CHILD_A_ID, None, None, "2026-07-10T03:00:03Z"),
        ],
    );
    let recovered = sync_test_file(&db, &child, &[&child])?;
    assert_eq!((recovered.imported, recovered.deferred), (1, false));
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_non_billable_file_without_meta_advances_cursor() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let child = rollout_path(temp.path(), CHILD_A_ID);
    write_jsonl(
        &child,
        &[
            turn_context(),
            token_count_at(0, 0, 0, "2026-07-10T03:00:02Z"),
        ],
    );

    let result = sync_test_file(&db, &child, &[&child])?;
    assert!(!result.deferred);
    assert_eq!(get_sync_state(&db, &child.to_string_lossy())?.1, 2);
    Ok(())
}

#[test]
#[serial_test::serial]
fn test_subagents_use_filename_thread_ids() -> Result<(), AppError> {
    clear_codex_replay_caches();
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let child_a = rollout_path(temp.path(), CHILD_A_ID);
    let child_b = rollout_path(temp.path(), CHILD_B_ID);
    write_jsonl(
        &child_a,
        &[
            session_meta(CHILD_A_ID),
            turn_context(),
            token_count(100, 50, 10),
        ],
    );
    write_jsonl(
        &child_b,
        &[
            session_meta(CHILD_B_ID),
            turn_context(),
            token_count(200, 100, 20),
        ],
    );

    assert_eq!(
        sync_test_file(&db, &child_a, &[&child_a, &child_b])?.imported,
        1
    );
    assert_eq!(
        sync_test_file(&db, &child_b, &[&child_a, &child_b])?.imported,
        1
    );

    let conn = lock_conn!(db.conn);
    let request_ids = conn
        .prepare(
            "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        request_ids,
        vec![
            format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_A_ID}:1"),
            format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{CHILD_B_ID}:1")
        ]
    );
    Ok(())
}

#[test]
fn test_archived_log_inherits_cursor_and_only_imports_appended_usage() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let sessions = temp.path().join("sessions");
    let archived = temp.path().join("archived_sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::create_dir_all(&archived).unwrap();
    let source = rollout_path(&sessions, PARENT_ID);
    let archived_file = rollout_path(&archived, PARENT_ID);
    write_jsonl(
        &archived_file,
        &[
            session_meta(PARENT_ID),
            turn_context(),
            token_count(100, 50, 10),
            token_count(200, 100, 20),
        ],
    );

    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens,
                    total_cost_usd, latency_ms, status_code, session_id,
                    created_at, data_source
                ) VALUES ('codex_session:parent:2', '_codex_session', 'codex',
                          'gpt-5.6-sol', 'gpt-5.6-sol', 999, 99, 0, '0', 0,
                          200, 'parent', 1, 'codex_session')",
            [],
        )?;
    }
    let source_path = source.to_string_lossy().to_string();
    update_sync_state(&db, &source_path, 1, 3)?;

    assert_eq!(
        sync_test_file(&db, &archived_file, &[&archived_file])?.imported,
        1
    );
    assert_eq!(
        sync_test_file(&db, &archived_file, &[&archived_file])?.imported,
        0
    );

    let conn = lock_conn!(db.conn);
    let old_row_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id = 'codex_session:parent:2'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(old_row_count, 1);
    let usage: (i64, i64, i64) = conn.query_row(
        "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs
             WHERE request_id = ?1",
        [format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:2")],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(usage, (100, 50, 10));
    drop(conn);
    assert_eq!(get_sync_state(&db, &archived_file.to_string_lossy())?.1, 4);

    Ok(())
}

fn compress_rollout(plain: &Path) -> PathBuf {
    let compressed = PathBuf::from(format!("{}.zst", plain.display()));
    let bytes = fs::read(plain).unwrap();
    fs::write(
        &compressed,
        zstd::stream::encode_all(bytes.as_slice(), 3).unwrap(),
    )
    .unwrap();
    fs::remove_file(plain).unwrap();
    compressed
}

fn decompress_rollout(compressed: &Path) -> PathBuf {
    let plain = codex_rollout_file::logical_path(compressed);
    let text = codex_rollout_file::read_to_string(compressed).unwrap();
    fs::write(&plain, text).unwrap();
    fs::remove_file(compressed).unwrap();
    plain
}

fn prune_detail_rows(db: &Database) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    conn.execute("DELETE FROM proxy_request_logs", [])?;
    Ok(())
}

/// Codex 压缩冷 rollout、恢复对话时再解压回来：两次换路径都靠游标继承防重复，
/// 中间把明细删掉（模拟 30 天裁剪），request_id 去重兜不住。
#[test]
fn test_compressed_rollout_round_trip_never_double_counts() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let sessions = temp.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let plain = rollout_path(&sessions, PARENT_ID);
    let mut lines = vec![
        session_meta(PARENT_ID),
        turn_context(),
        token_count(100, 50, 10),
    ];
    write_jsonl(&plain, &lines);
    assert_eq!(sync_test_file(&db, &plain, &[&plain])?.imported, 1);

    // CC Switch 没赶上第二条用量，文件就被压缩了
    lines.push(token_count(200, 100, 20));
    write_jsonl(&plain, &lines);
    let compressed = compress_rollout(&plain);
    prune_detail_rows(&db)?;
    assert_eq!(
        sync_test_file(&db, &compressed, &[&compressed])?.imported,
        1
    );
    assert_unchanged_codex_file_is_skipped(&db, &compressed)?;

    // 恢复对话：解压回普通形态并追加；普通形态的旧游标落后于压缩形态
    prune_detail_rows(&db)?;
    let plain = decompress_rollout(&compressed);
    lines.push(token_count(300, 150, 30));
    write_jsonl(&plain, &lines);
    assert_eq!(sync_test_file(&db, &plain, &[&plain])?.imported, 1);

    let conn = lock_conn!(db.conn);
    let request_ids: Vec<String> = conn
        .prepare("SELECT request_id FROM proxy_request_logs")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(
        request_ids,
        vec![format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{PARENT_ID}:3")]
    );
    drop(conn);
    assert_eq!(get_sync_state(&db, &plain.to_string_lossy())?.1, 5);
    Ok(())
}

#[test]
fn test_compressed_rollout_filename_parsing() {
    let plain = PathBuf::from(format!("/s/rollout-2026-07-10T03-00-00-{PARENT_ID}.jsonl"));
    let compressed = PathBuf::from(format!("{}.zst", plain.display()));
    assert_eq!(
        thread_id_from_filename(&compressed),
        thread_id_from_filename(&plain)
    );
    assert!(thread_id_from_filename(&compressed).is_some());
    assert!(is_rollout_filename(
        compressed.file_name().unwrap().to_str().unwrap()
    ));

    let two_segment = PathBuf::from(format!(
        "/s/rollout-2026-08-26T17-18-13-{PARENT_ID}_{CHILD_A_ID}.jsonl.zst"
    ));
    assert_eq!(
        leading_thread_id_from_filename(&two_segment).as_deref(),
        Some(PARENT_ID)
    );
    assert_eq!(
        thread_id_from_filename(&two_segment).as_deref(),
        Some(CHILD_A_ID)
    );
}

#[test]
fn test_insert_codex_session_skips_matching_proxy_log() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                "codex-proxy",
                "openai",
                "codex",
                "gpt-5.4",
                "gpt-5.4",
                10,
                2,
                1,
                7,
                "0.01",
                100,
                200,
                1000,
                "proxy"
            ],
        )?;
    }

    // Older rollouts carry no cache-write count; newer ones report the same
    // count the proxy logged. Either way the session row is the proxied request.
    for (request_id, cache_write_input) in
        [("codex-session-dup", 0), ("codex-session-dup-write", 7)]
    {
        let delta = DeltaTokens {
            input: 10,
            cached_input: 1,
            cache_write_input,
            output: 2,
        };
        let mut suspected_duplicates = 0;
        let inserted = insert_codex_session_entry(
            &db,
            request_id,
            &delta,
            "gpt-5.4",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
            &mut suspected_duplicates,
        )?;
        assert!(!inserted, "cache_write_input={cache_write_input}");
    }

    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
        row.get(0)
    })?;
    assert_eq!(count, 1);

    Ok(())
}

#[test]
fn test_codex_session_cache_writes_dedup_against_legacy_proxy_log() -> Result<(), AppError> {
    // v3.17.0 之前的代理不记录 Codex 缓存写入（legacy 语义行写入量恒为 0）。
    // 重建时带写入量的 session 行仍是同一请求；新语义代理行保持精确匹配。
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        for (request_id, created_at, semantics) in [
            ("legacy-proxy", 1000, INPUT_TOKEN_SEMANTICS_LEGACY),
            ("total-proxy", 5000, INPUT_TOKEN_SEMANTICS_TOTAL),
        ] {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        total_cost_usd, latency_ms, status_code, created_at, data_source,
                        input_token_semantics
                    ) VALUES (?1, 'openai', 'codex', 'gpt-5.6', 'gpt-5.6', 1000, 50, 300, 0,
                              '0.01', 100, 200, ?2, 'proxy', ?3)",
                rusqlite::params![request_id, created_at, semantics],
            )?;
        }
    }

    let delta = DeltaTokens {
        input: 1000,
        cached_input: 300,
        cache_write_input: 600,
        output: 50,
    };
    let mut suspected_duplicates = 0;
    for (request_id, timestamp, expect_inserted) in [
        ("session-legacy", "1970-01-01T00:16:45Z", false),
        ("session-total", "1970-01-01T01:23:25Z", true),
    ] {
        let inserted = insert_codex_session_entry(
            &db,
            request_id,
            &delta,
            "gpt-5.6",
            Some("session-1"),
            Some(timestamp),
            &mut suspected_duplicates,
        )?;
        assert_eq!(inserted, expect_inserted, "{request_id}");
    }

    Ok(())
}

#[test]
fn test_codex_session_duplicate_is_observed_but_still_inserted() -> Result<(), AppError> {
    let db = Database::memory()?;
    let delta = DeltaTokens {
        input: 10,
        cached_input: 1,
        cache_write_input: 0,
        output: 2,
    };
    let mut suspected_duplicates = 0;
    assert!(insert_codex_session_entry(
        &db,
        "codex-session-a",
        &delta,
        "gpt-5.4",
        Some("session-a"),
        Some("1970-01-01T00:16:40Z"),
        &mut suspected_duplicates,
    )?);
    assert!(insert_codex_session_entry(
        &db,
        "codex-session-b",
        &delta,
        "gpt-5.4",
        Some("session-b"),
        Some("1970-01-01T00:16:45Z"),
        &mut suspected_duplicates,
    )?);
    assert_eq!(suspected_duplicates, 1);

    let conn = lock_conn!(db.conn);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 2);
    Ok(())
}

#[test]
fn reset_codex_usage_only_removes_codex_rows_and_structural_cursors() -> Result<(), AppError> {
    let db = Database::memory()?;
    let temp = tempdir().unwrap();
    let wide_dir = temp.path();
    let current_codex = rollout_path(&wide_dir.join("sessions"), CHILD_A_ID);
    let legacy_codex = format!("C:\\old-codex\\archived_sessions\\rollout-old-{CHILD_B_ID}.jsonl");
    let gemini_cursor = wide_dir.join("gemini/sessions/session-123.json");
    let claude_cursor = wide_dir.join(format!("projects/rollout-{PARENT_ID}.jsonl"));

    {
        let conn = lock_conn!(db.conn);
        conn.execute_batch(
            "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, cache_read_tokens, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES
                    ('codex-row', '_codex_session', 'codex', 'gpt', 1, 1, 0, 0, 200, 1, 'codex_session'),
                    ('gemini-row', '_gemini_session', 'gemini', 'gemini', 1, 1, 0, 0, 200, 1, 'gemini_session');
                 INSERT INTO usage_daily_rollups (date, app_type, provider_id, model)
                 VALUES
                    ('2026-07-10', 'codex', '_codex_session', 'gpt'),
                    ('2026-07-10', 'gemini', '_gemini_session', 'gemini');",
        )?;
        for path in [
            current_codex.to_string_lossy().to_string(),
            legacy_codex,
            gemini_cursor.to_string_lossy().to_string(),
            claude_cursor.to_string_lossy().to_string(),
        ] {
            conn.execute(
                "INSERT INTO session_log_sync
                     (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES (?1, 1, 1, 1)",
                [path],
            )?;
        }

        reset_codex_usage_on_conn(&conn, wide_dir)?;
        let codex_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        let gemini_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'gemini_session'",
            [],
            |row| row.get(0),
        )?;
        let codex_rollups: i64 = conn.query_row(
            "SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
            [],
            |row| row.get(0),
        )?;
        let remaining_cursors: i64 =
            conn.query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| {
                row.get(0)
            })?;
        assert_eq!((codex_rows, gemini_rows, codex_rollups), (0, 1, 0));
        assert_eq!(remaining_cursors, 2);
    }
    Ok(())
}

// ── 模型名归一化测试 ──

#[test]
fn test_normalize_codex_model_lowercase() {
    assert_eq!(normalize_codex_model("GLM-4.6"), "glm-4.6");
    assert_eq!(normalize_codex_model("DeepSeek-Chat"), "deepseek-chat");
    assert_eq!(normalize_codex_model("GPT-5.4"), "gpt-5.4");
}

#[test]
fn test_normalize_codex_model_strip_prefix() {
    assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
    assert_eq!(
        normalize_codex_model("azure/gpt-5.2-codex"),
        "gpt-5.2-codex"
    );
    assert_eq!(normalize_codex_model("OPENAI/GPT-5.4"), "gpt-5.4");
}

#[test]
fn test_normalize_codex_model_strips_date_suffixes() {
    for (case, dated, dated_pro, expected_pro) in [
        (
            "test_normalize_codex_model_strip_iso_date",
            "gpt-5.4-2026-03-05",
            "gpt-5.4-pro-2026-03-05",
            "gpt-5.4-pro",
        ),
        (
            "test_normalize_codex_model_strip_compact_date",
            "gpt-5.4-20260305",
            "claude-opus-4-6-20260206",
            "claude-opus-4-6",
        ),
        // prefix + uppercase + ISO date
        // prefix + compact date
        (
            "test_normalize_codex_model_combined",
            "openai/GPT-5.4-2026-03-05",
            "openai/gpt-5.4-20260305",
            "gpt-5.4",
        ),
    ] {
        assert_eq!(normalize_codex_model(dated), "gpt-5.4", "{case}");
        assert_eq!(normalize_codex_model(dated_pro), expected_pro, "{case}");
    }
}

#[test]
fn test_normalize_codex_model_no_change() {
    assert_eq!(normalize_codex_model("gpt-5.4"), "gpt-5.4");
    assert_eq!(normalize_codex_model("gpt-5.2-codex"), "gpt-5.2-codex");
    assert_eq!(normalize_codex_model("o3"), "o3");
    assert_eq!(normalize_codex_model("deepseek-chat"), "deepseek-chat");
}

/// 缓存读取是 input 的一部分：上游报出的 cached 比本次 input 还大时（异常数据），
/// 入库的 cache_read 也不超过这次的 input。逐条增量和压缩用量记录都一样。
#[test]
fn cached_tokens_never_exceed_the_input_they_belong_to() -> Result<(), AppError> {
    fn rows(lines: &[serde_json::Value]) -> Result<Vec<(i64, i64)>, AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let path = rollout_path(temp.path(), PARENT_ID);
        let mut all = vec![session_meta(PARENT_ID), turn_context()];
        all.extend_from_slice(lines);
        write_jsonl(&path, &all);
        sync_test_file(&db, &path, &[&path])?;
        let conn = lock_conn!(db.conn);
        let mut stmt = conn
            .prepare(
                "SELECT input_tokens, cache_read_tokens FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| AppError::Database(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(rows)
    }

    // 累计 input 100 → 110（这次 10），累计 cached 0 → 80（这次 80）。
    let increments = rows(&[
        token_count_at(100, 0, 50, "2026-07-10T03:00:02Z"),
        token_count_at(110, 80, 60, "2026-07-10T03:00:03Z"),
    ])?;
    assert_eq!(increments, vec![(100, 0), (10, 10)]);

    let compaction = rows(&[
        token_usage_record_at("resp_compact", 100, 150, 20, "2026-07-10T03:00:02Z"),
        token_count_with_last_at(0, 0, 0, 0, 0, 0, "default", "2026-07-10T03:00:02Z"),
    ])?;
    assert_eq!(compaction, vec![(100, 100)]);
    Ok(())
}

/// 真实语料回放验收 harness（仅手动运行，勿在 CI 跑）。
///
/// 把真实 `~/.codex/sessions` 和 `archived_sessions` 在内存库上全量重导，输出计时与
/// 结果快照。用于性能改动的行为等价验证：改动前后各跑一次，两侧
/// `CODEX_REPLAY_OUT` 文件必须逐字节相同。
/// `CODEX_REPLAY_ROOT` 可指定包含这两个目录的固定语料快照。
///
/// ```bash
/// CODEX_REPLAY_OUT=/tmp/replay.tsv \
///   cargo test --release replay_real_codex_corpus -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn replay_real_codex_corpus() -> Result<(), AppError> {
    let Some(real_home) = dirs::home_dir() else {
        eprintln!("[REPLAY] no home dir, skipping");
        return Ok(());
    };
    let real_codex = std::env::var_os("CODEX_REPLAY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home.join(".codex"));
    let real_sessions = real_codex.join("sessions");
    if !real_sessions.is_dir() {
        eprintln!("[REPLAY] {} not found, skipping", real_sessions.display());
        return Ok(());
    }

    // 临时 HOME 里只放一个指向真实语料的只读 symlink，避免测试
    // 触碰真实 ~/.cc-switch / ~/.codex 下的任何其他内容。
    let temp = tempfile::tempdir().expect("create temp home");
    fs::create_dir_all(temp.path().join(".codex")).expect("mkdir .codex");
    #[cfg(unix)]
    for directory in ["sessions", "archived_sessions"] {
        let source = real_codex.join(directory);
        if source.is_dir() {
            std::os::unix::fs::symlink(&source, temp.path().join(".codex").join(directory))
                .expect("symlink rollout directory");
        }
    }
    let env = crate::test_support::EnvVars::new().set("CC_SWITCH_TEST_HOME", temp.path());

    clear_codex_replay_caches();
    // CODEX_REPLAY_DISK=1 时用临时 HOME 下的磁盘库：逐行 autocommit 的
    // 主要成本是磁盘 journal fsync，内存库测不出真实写库开销。
    let db = if std::env::var("CODEX_REPLAY_DISK").is_ok() {
        Database::init()?
    } else {
        Database::memory()?
    };
    let start = std::time::Instant::now();
    let result = sync_codex_usage(&db)?;
    let full_elapsed = start.elapsed();
    eprintln!(
        "[REPLAY] full reimport: imported={} skipped={} suspected_dup={} deferred={} files={} errors={} elapsed={:.2?}",
        result.imported,
        result.skipped,
        result.suspected_duplicates,
        result.deferred_files,
        result.files_scanned,
        result.errors.len(),
        full_elapsed
    );

    let start = std::time::Instant::now();
    let steady = sync_codex_usage(&db)?;
    eprintln!(
        "[REPLAY] steady pass: imported={} deferred={} elapsed={:.2?}",
        steady.imported,
        steady.deferred_files,
        start.elapsed()
    );

    if let Ok(out_path) = std::env::var("CODEX_REPLAY_OUT") {
        use std::io::Write;
        let conn = lock_conn!(db.conn);
        let mut stmt = conn
            .prepare(
                "SELECT request_id, model, request_model, input_tokens, output_tokens,
                            cache_read_tokens, cache_creation_tokens,
                            input_cost_usd, output_cost_usd, cache_read_cost_usd,
                            cache_creation_cost_usd, total_cost_usd,
                            session_id, provider_id, provider_type, status_code,
                            is_streaming, cost_multiplier, created_at, data_source
                     FROM proxy_request_logs
                     WHERE data_source = 'codex_session'
                     ORDER BY request_id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let mut fields = Vec::with_capacity(20);
                for idx in 0..20 {
                    fields.push(match row.get_ref(idx)? {
                        rusqlite::types::ValueRef::Null => "NULL".to_string(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        rusqlite::types::ValueRef::Real(v) => v.to_string(),
                        rusqlite::types::ValueRef::Text(v) => {
                            String::from_utf8_lossy(v).into_owned()
                        }
                        rusqlite::types::ValueRef::Blob(v) => format!("blob:{}", v.len()),
                    });
                }
                Ok(fields.join("\t"))
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut out = fs::File::create(&out_path).expect("create replay out file");
        for line in &rows {
            writeln!(out, "{line}").expect("write replay row");
        }
        eprintln!("[REPLAY] wrote {} rows to {out_path}", rows.len());
    }

    drop(env);
    Ok(())
}

/// 按 Codex 实际写出的键顺序（timestamp、type、payload）拼一行；`json!` 会把键
/// 按字母排序，行头判断种类靠的是真实顺序
fn raw_line(timestamp: &str, kind: &str, payload: &str) -> String {
    format!(r#"{{"timestamp":"{timestamp}","type":"{kind}","payload":{payload}}}"#)
}

fn raw_item(timestamp: &str, payload: &str) -> String {
    raw_line(timestamp, "response_item", payload)
}

fn raw_usage(input: u64, output: u64) -> String {
    format!(
        r#"{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output},"reasoning_output_tokens":0,"total_tokens":{}}}"#,
        input + output
    )
}

/// `total` 是累计用量，`last` 是这一次请求的用量
fn raw_token_count(timestamp: &str, total: (u64, u64), last: (u64, u64)) -> String {
    raw_line(
        timestamp,
        "event_msg",
        &format!(
            r#"{{"type":"token_count","info":{{"total_token_usage":{},"last_token_usage":{}}},"rate_limits":{{"limit_id":"codex"}}}}"#,
            raw_usage(total.0, total.1),
            raw_usage(last.0, last.1)
        ),
    )
}

fn parsed_timings(lines: &[String]) -> Vec<Option<EstimatedTiming>> {
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    fs::write(&file, lines.join("\n") + "\n").unwrap();
    parse_codex_file(&file, Some(PARENT_ID.to_string()))
        .unwrap()
        .token_events
        .iter()
        .filter(|event| event.event_index.is_some())
        .map(|event| event.timing)
        .collect()
}

fn parsed_latencies(lines: &[String]) -> Vec<Option<i64>> {
    parsed_timings(lines)
        .into_iter()
        .map(|timing| timing.map(|t| t.latency_ms))
        .collect()
}

fn parsed_first_tokens(lines: &[String]) -> Vec<Option<i64>> {
    parsed_timings(lines)
        .into_iter()
        .map(|timing| timing.and_then(|t| t.first_token_ms))
        .collect()
}

/// 新版 Codex 的 `item_completed`，键顺序照实际日志
fn raw_item_completed(timestamp: &str, item_type: &str, started_at_ms: i64) -> String {
    raw_line(
        timestamp,
        "event_msg",
        &format!(
            r#"{{"type":"item_completed","thread_id":"t","turn_id":"u","item":{{"type":"{item_type}","id":"i1","content":[]}},"started_at_ms":{started_at_ms},"completed_at_ms":{started_at_ms}}}"#
        ),
    )
}

const USER_MESSAGE: &str = r#"{"type":"message","role":"user","content":[]}"#;
const ASSISTANT_MESSAGE: &str = r#"{"type":"message","role":"assistant","content":[]}"#;
const REASONING: &str = r#"{"type":"reasoning","summary":[]}"#;
const TOOL_CALL: &str = r#"{"type":"custom_tool_call","call_id":"c1","name":"shell"}"#;
const TOOL_OUTPUT: &str = r#"{"type":"custom_tool_call_output","call_id":"c1","output":"ok"}"#;

#[test]
fn test_latency_ends_at_usage_record_not_at_delayed_token_count() -> Result<(), AppError> {
    let lines = vec![
        raw_line(
            "2026-10-02T03:00:00.000Z",
            "turn_context",
            r#"{"model":"gpt-5"}"#,
        ),
        raw_item("2026-10-02T03:00:01.000Z", USER_MESSAGE),
        raw_item("2026-10-02T03:00:05.000Z", REASONING),
        raw_item("2026-10-02T03:00:09.000Z", TOOL_CALL),
        // 响应在这里结束；token_count 要等工具跑完才写
        raw_line(
            "2026-10-02T03:00:09.500Z",
            "token_usage_record",
            &format!(r#"{{"turn_id":"t1","usage":{}}}"#, raw_usage(100, 600)),
        ),
        raw_item("2026-10-02T03:00:20.000Z", TOOL_OUTPUT),
        raw_token_count("2026-10-02T03:00:20.001Z", (100, 600), (100, 600)),
        // 第二次请求：用量记录对不上这次请求，不采信，退回 token_count 的时刻
        raw_item("2026-10-02T03:00:25.000Z", REASONING),
        raw_item("2026-10-02T03:00:30.000Z", ASSISTANT_MESSAGE),
        raw_line(
            "2026-10-02T03:00:30.200Z",
            "token_usage_record",
            &format!(r#"{{"turn_id":"other","usage":{}}}"#, raw_usage(1, 999)),
        ),
        raw_token_count("2026-10-02T03:00:30.300Z", (300, 1300), (200, 700)),
    ];

    assert_eq!(parsed_latencies(&lines), vec![Some(8_500), Some(10_299)]);

    // 估出来的耗时要落进库里
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let mut contents = session_meta(PARENT_ID).to_string();
    contents.push('\n');
    contents.push_str(&(lines.join("\n") + "\n"));
    fs::write(&file, contents).unwrap();
    let db = Database::memory()?;
    sync_test_file(&db, &file, &[&file])?;
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare("SELECT latency_ms FROM proxy_request_logs ORDER BY request_id")
        .unwrap();
    let stored: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(stored, vec![8_500, 10_299]);
    Ok(())
}

#[test]
fn test_first_token_is_the_earliest_model_output_item_start() -> Result<(), AppError> {
    // 2026-10-02T03:00:00Z 的毫秒时间戳
    let base = 1_790_910_000_000;
    let lines = vec![
        raw_line(
            "2026-10-02T03:00:00.000Z",
            "turn_context",
            r#"{"model":"gpt-5"}"#,
        ),
        raw_item("2026-10-02T03:00:01.000Z", USER_MESSAGE),
        // 第一次请求：思考从 3.2 秒开始，回复晚一些；工具执行项不算模型输出
        raw_item_completed("2026-10-02T03:00:05.000Z", "Reasoning", base + 3_200),
        raw_item("2026-10-02T03:00:05.001Z", REASONING),
        raw_item_completed("2026-10-02T03:00:06.000Z", "AgentMessage", base + 5_500),
        raw_item("2026-10-02T03:00:06.001Z", ASSISTANT_MESSAGE),
        raw_line(
            "2026-10-02T03:00:06.100Z",
            "token_usage_record",
            &format!(r#"{{"turn_id":"t1","usage":{}}}"#, raw_usage(100, 600)),
        ),
        raw_token_count("2026-10-02T03:00:06.101Z", (100, 600), (100, 600)),
        // 第二次请求一开头就是工具调用，没有思考和回复项，推不出首字
        raw_item_completed("2026-10-02T03:00:07.000Z", "CommandExecution", base + 6_500),
        raw_item("2026-10-02T03:00:09.000Z", TOOL_CALL),
        raw_line(
            "2026-10-02T03:00:09.100Z",
            "token_usage_record",
            &format!(r#"{{"turn_id":"t1","usage":{}}}"#, raw_usage(200, 700)),
        ),
        raw_token_count("2026-10-02T03:00:09.101Z", (300, 1300), (200, 700)),
        // 第三次请求：开始时刻比起点还早，说明对不上，不要
        raw_item_completed("2026-10-02T03:00:11.000Z", "AgentMessage", base + 9_000),
        raw_item("2026-10-02T03:00:11.001Z", ASSISTANT_MESSAGE),
        raw_line(
            "2026-10-02T03:00:11.100Z",
            "token_usage_record",
            &format!(r#"{{"turn_id":"t1","usage":{}}}"#, raw_usage(300, 800)),
        ),
        raw_token_count("2026-10-02T03:00:11.101Z", (600, 2100), (300, 800)),
    ];

    assert_eq!(
        parsed_latencies(&lines),
        vec![Some(5_100), Some(2_999), Some(1_999)]
    );
    assert_eq!(parsed_first_tokens(&lines), vec![Some(2_200), None, None]);

    // 推出来的首字要落进库里
    let dir = tempdir().unwrap();
    let file = rollout_path(dir.path(), PARENT_ID);
    let mut contents = session_meta(PARENT_ID).to_string();
    contents.push('\n');
    contents.push_str(&(lines.join("\n") + "\n"));
    fs::write(&file, contents).unwrap();
    let db = Database::memory()?;
    sync_test_file(&db, &file, &[&file])?;
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare("SELECT first_token_ms FROM proxy_request_logs ORDER BY request_id")
        .unwrap();
    let stored: Vec<Option<i64>> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(stored, vec![Some(2_200), None, None]);
    Ok(())
}

#[test]
fn test_latency_without_usage_record_stops_before_tool_execution() {
    let lines = vec![
        raw_item("2026-08-03T03:00:01.000Z", USER_MESSAGE),
        raw_item("2026-08-03T03:00:06.000Z", TOOL_CALL),
        // 工具跑了 10 秒，token_count 紧跟着工具结果写出
        raw_item("2026-08-03T03:00:16.000Z", TOOL_OUTPUT),
        raw_token_count("2026-08-03T03:00:16.001Z", (100, 600), (100, 600)),
        // 限额刷新时重发的快照不是一次请求，不能把下一次请求的起点往后挪
        raw_token_count("2026-08-03T03:00:17.000Z", (100, 600), (100, 600)),
        raw_item("2026-08-03T03:00:20.000Z", REASONING),
        raw_item("2026-08-03T03:00:22.000Z", ASSISTANT_MESSAGE),
        raw_token_count("2026-08-03T03:00:22.100Z", (300, 1300), (200, 700)),
    ];

    assert_eq!(parsed_latencies(&lines), vec![Some(5_000), Some(6_099)]);
}

#[test]
fn test_latency_ignores_items_written_after_their_token_count() {
    // 2025 年的布局：输出项和工具结果补写在它们那次响应的 token_count 之后
    let lines = vec![
        raw_item("2025-10-30T08:00:01.000Z", USER_MESSAGE),
        raw_token_count("2025-10-30T08:00:08.000Z", (100, 600), (100, 600)),
        raw_item("2025-10-30T08:00:08.000Z", REASONING),
        raw_item("2025-10-30T08:00:08.001Z", TOOL_CALL),
        raw_item("2025-10-30T08:00:08.001Z", TOOL_OUTPUT),
        raw_line(
            "2025-10-30T08:00:08.002Z",
            "turn_context",
            r#"{"model":"gpt-5"}"#,
        ),
        // 用户隔了一分多钟才发下一条消息，这段空闲不算进请求耗时
        raw_item("2025-10-30T08:01:40.000Z", USER_MESSAGE),
        raw_token_count("2025-10-30T08:01:50.000Z", (300, 1300), (200, 700)),
    ];

    assert_eq!(parsed_latencies(&lines), vec![Some(7_000), Some(10_000)]);
}

#[test]
fn test_timing_line_is_classified_from_the_line_head() {
    let long_output = format!(
        r#"{{"type":"function_call_output","call_id":"c1","output":"{}"}}"#,
        "x".repeat(4 * LINE_HEAD_BYTES)
    );
    assert_eq!(
        classify_timing_line(&raw_item("2026-10-02T03:00:00.250Z", &long_output)),
        Some((1_790_910_000_250, TimingLine::ToolOutput))
    );
    // 正文里出现的字样不影响判断：种类只看信封上的 type
    let quoted = r#"{"type":"message","role":"user","content":"\"type\":\"token_usage_record\""}"#;
    assert_eq!(
        classify_timing_line(&raw_item("2026-10-02T03:00:00.250Z", quoted)).map(|line| line.1),
        Some(TimingLine::Boundary)
    );
    assert_eq!(
        classify_timing_line(&raw_line(
            "2026-10-02T03:00:00.250Z",
            "event_msg",
            r#"{"type":"task_started"}"#
        ))
        .map(|line| line.1),
        Some(TimingLine::TurnReset)
    );
    assert_eq!(
        classify_timing_line(&raw_line(
            "2026-10-02T03:00:00.250Z",
            "event_msg",
            r#"{"type":"item_completed"}"#
        )),
        None
    );
}

#[test]
fn test_latency_does_not_inherit_the_start_of_an_aborted_request() {
    const DEVELOPER_MESSAGE: &str = r#"{"type":"message","role":"developer","content":[]}"#;
    let lines = vec![
        raw_item("2026-10-02T03:00:01.000Z", USER_MESSAGE),
        // 这次请求出了一段思考就被用户中断，等不到 token_count
        raw_item("2026-10-02T03:00:10.000Z", REASONING),
        raw_line(
            "2026-10-02T03:00:20.000Z",
            "event_msg",
            r#"{"type":"turn_aborted","reason":"interrupted"}"#,
        ),
        raw_line(
            "2026-10-02T03:02:00.000Z",
            "event_msg",
            r#"{"type":"task_started"}"#,
        ),
        raw_item("2026-10-02T03:02:00.010Z", USER_MESSAGE),
        raw_item("2026-10-02T03:02:20.000Z", REASONING),
        raw_item("2026-10-02T03:02:30.000Z", ASSISTANT_MESSAGE),
        raw_token_count("2026-10-02T03:02:30.100Z", (100, 600), (100, 600)),
        // 响应中途插进来的 developer 消息不是新一轮，起点不动
        raw_item("2026-10-02T03:02:35.000Z", REASONING),
        raw_item("2026-10-02T03:02:36.000Z", DEVELOPER_MESSAGE),
        raw_item("2026-10-02T03:02:40.000Z", ASSISTANT_MESSAGE),
        raw_token_count("2026-10-02T03:02:40.200Z", (300, 1300), (200, 700)),
    ];

    assert_eq!(parsed_latencies(&lines), vec![Some(30_090), Some(10_100)]);
}
