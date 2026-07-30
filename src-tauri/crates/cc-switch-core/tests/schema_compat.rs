use std::path::Path;

use cc_switch_core::{CoreError, HeadlessState, SchemaError, DESKTOP_SCHEMA_VERSION};
use rusqlite::Connection;

const CANONICAL_REQUIRED_SCHEMA: &str = r#"
CREATE TABLE providers (
    id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    name TEXT NOT NULL,
    settings_config TEXT NOT NULL,
    website_url TEXT,
    category TEXT,
    created_at INTEGER,
    sort_index INTEGER,
    notes TEXT,
    icon TEXT,
    icon_color TEXT,
    meta TEXT NOT NULL DEFAULT '{}',
    is_current BOOLEAN NOT NULL DEFAULT 0,
    in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
    PRIMARY KEY (id, app_type)
);
CREATE TABLE provider_endpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    url TEXT NOT NULL,
    added_at INTEGER,
    FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
);
CREATE TABLE proxy_request_logs (
    request_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT,
    pricing_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    input_cost_usd TEXT NOT NULL DEFAULT '0',
    output_cost_usd TEXT NOT NULL DEFAULT '0',
    cache_read_cost_usd TEXT NOT NULL DEFAULT '0',
    cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    latency_ms INTEGER NOT NULL,
    first_token_ms INTEGER,
    duration_ms INTEGER,
    status_code INTEGER NOT NULL,
    error_message TEXT,
    session_id TEXT,
    provider_type TEXT,
    is_streaming INTEGER NOT NULL DEFAULT 0,
    cost_multiplier TEXT NOT NULL DEFAULT '1.0',
    created_at INTEGER NOT NULL,
    data_source TEXT NOT NULL DEFAULT 'proxy'
);
CREATE TABLE model_pricing (
    model_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    input_cost_per_million TEXT NOT NULL,
    output_cost_per_million TEXT NOT NULL,
    cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
    cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
);
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL,
    app_type TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '',
    pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    avg_latency_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
CREATE TABLE session_log_sync (
    file_path TEXT PRIMARY KEY,
    last_modified INTEGER NOT NULL,
    last_line_offset INTEGER NOT NULL DEFAULT 0,
    last_synced_at INTEGER NOT NULL
);
"#;

#[test]
fn opens_existing_v16_schema_without_mutating_it() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let db_path = database_path(home.path());
    let connection = create_database(&db_path);
    connection
        .execute_batch(CANONICAL_REQUIRED_SCHEMA)
        .expect("创建桌面 v16 fixture");
    connection
        .pragma_update(None, "user_version", DESKTOP_SCHEMA_VERSION)
        .expect("设置桌面 schema 版本");
    drop(connection);

    let state = HeadlessState::open(home.path()).expect("打开桌面规范数据库");
    assert_eq!(
        state.schema_version().expect("读取 schema 版本"),
        DESKTOP_SCHEMA_VERSION
    );
    drop(state);

    let reopened = Connection::open(db_path).expect("重新打开 fixture");
    assert!(column_exists(&reopened, "providers", "app_type"));
    assert!(!column_exists(&reopened, "providers", "app"));
    assert!(!table_exists(&reopened, "current_providers"));
}

#[test]
fn rejects_legacy_agent_schema_before_business_writes() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let db_path = database_path(home.path());
    let connection = create_database(&db_path);
    connection
        .execute_batch(
            "CREATE TABLE providers (
                app TEXT NOT NULL,
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                PRIMARY KEY (app, id)
             );
             CREATE TABLE current_providers (
                app TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL
             );
             PRAGMA user_version = 1;",
        )
        .expect("创建旧 Agent schema");
    drop(connection);

    let error = match HeadlessState::open(home.path()) {
        Ok(_) => panic!("旧 Agent schema 必须在业务写入前被拒绝"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CoreError::Schema(SchemaError::Incompatible {
            detected: 1,
            supported: DESKTOP_SCHEMA_VERSION,
            ..
        })
    ));
}

#[test]
fn creates_canonical_required_schema_for_a_new_remote_home() {
    let home = tempfile::tempdir().expect("创建临时 HOME");
    let state = HeadlessState::open(home.path()).expect("初始化全新远端数据库");
    assert_eq!(
        state.schema_version().expect("读取新库版本"),
        DESKTOP_SCHEMA_VERSION
    );
    drop(state);

    let connection = Connection::open(database_path(home.path())).expect("打开新数据库");
    for table in [
        "providers",
        "provider_endpoints",
        "proxy_request_logs",
        "model_pricing",
        "usage_daily_rollups",
        "session_log_sync",
    ] {
        assert!(table_exists(&connection, table), "缺少规范表 {table}");
    }
    assert!(column_exists(&connection, "providers", "app_type"));
    assert!(column_exists(&connection, "providers", "is_current"));
    assert!(!column_exists(&connection, "providers", "app"));
    assert!(!table_exists(&connection, "current_providers"));
}

fn database_path(home: &Path) -> std::path::PathBuf {
    home.join(".cc-switch").join("cc-switch.db")
}

fn create_database(path: &Path) -> Connection {
    std::fs::create_dir_all(path.parent().expect("数据库目录")).expect("创建数据库目录");
    Connection::open(path).expect("创建数据库")
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .expect("检查表")
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> bool {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("读取列信息");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("查询列信息");
    let found = columns
        .map(|name| name.expect("读取列名"))
        .any(|name| name == column);
    drop(statement);
    found
}
