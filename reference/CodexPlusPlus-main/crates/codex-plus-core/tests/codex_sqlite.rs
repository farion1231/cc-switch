use codex_plus_core::codex_sqlite::sanitize_historical_model_suffixes;
use rusqlite::Connection;

fn create_threads_table(conn: &Connection) {
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model TEXT,
            updated_at INTEGER
        )",
        [],
    )
    .unwrap();
}

#[test]
fn sanitize_strips_suffix_from_thread_model() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let db_path = home.join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    create_threads_table(&conn);
    conn.execute(
        "INSERT INTO threads (id, model, updated_at) VALUES (?1, ?2, ?3)",
        ["t1", "deepseek/deepseek-v4-flash[1M]", "1000"],
    )
    .unwrap();
    drop(conn);

    let result = sanitize_historical_model_suffixes(&home).unwrap();
    assert_eq!(result.scanned, 1);
    assert_eq!(result.updated, 1);

    let conn = Connection::open(&db_path).unwrap();
    let model: String = conn
        .query_row("SELECT model FROM threads WHERE id = 't1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(model, "deepseek/deepseek-v4-flash");
}

#[test]
fn sanitize_skips_models_without_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let db_path = home.join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    create_threads_table(&conn);
    conn.execute(
        "INSERT INTO threads (id, model, updated_at) VALUES (?1, ?2, ?3)",
        ["t1", "gpt-5.5", "1000"],
    )
    .unwrap();
    drop(conn);

    let result = sanitize_historical_model_suffixes(&home).unwrap();
    assert_eq!(result.scanned, 0);
    assert_eq!(result.updated, 0);
}

#[test]
fn sanitize_skips_invalid_suffixes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let db_path = home.join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    create_threads_table(&conn);
    conn.execute(
        "INSERT INTO threads (id, model, updated_at) VALUES (?1, ?2, ?3)",
        ["t1", "foo[bar]", "1000"],
    )
    .unwrap();
    drop(conn);

    let result = sanitize_historical_model_suffixes(&home).unwrap();
    assert_eq!(result.scanned, 1);
    assert_eq!(result.updated, 0);
}

#[test]
fn sanitize_handles_null_model() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let db_path = home.join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    create_threads_table(&conn);
    conn.execute(
        "INSERT INTO threads (id, model, updated_at) VALUES (?1, ?2, ?3)",
        rusqlite::params!["t1", rusqlite::types::Null, "1000"],
    )
    .unwrap();
    drop(conn);

    let result = sanitize_historical_model_suffixes(&home).unwrap();
    assert_eq!(result.scanned, 0);
    assert_eq!(result.updated, 0);
}

#[test]
fn sanitize_cleans_suffix_from_logs() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();

    // logs_2.sqlite 不需要 threads 表，只需要 logs 表。
    let logs_path = home.join("logs_2.sqlite");
    let conn = Connection::open(&logs_path).unwrap();
    conn.execute(
        "CREATE TABLE logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            ts_nanos INTEGER NOT NULL,
            level TEXT NOT NULL,
            target TEXT NOT NULL,
            feedback_log_body TEXT,
            module_path TEXT,
            file TEXT,
            line INTEGER,
            thread_id TEXT,
            process_uuid TEXT,
            estimated_bytes INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        [
            "1",
            "1",
            "INFO",
            "codex_models_manager::cache",
            r#"session_loop{model="deepseek-v4-flash[1M]"}: Unknown model deepseek-v4-flash[1M] is used."#,
        ],
    )
    .unwrap();
    drop(conn);

    let result = sanitize_historical_model_suffixes(&home).unwrap();
    // threads 表为空，所以 scanned/updated 都是 0；但日志应被清理。
    assert_eq!(result.scanned, 0);
    assert_eq!(result.updated, 0);

    let conn = Connection::open(&logs_path).unwrap();
    let body: String = conn
        .query_row(
            "SELECT feedback_log_body FROM logs WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !body.contains("[1M]"),
        "expected suffix to be stripped from logs, got: {body}"
    );
    assert!(body.contains("deepseek-v4-flash"));
}
