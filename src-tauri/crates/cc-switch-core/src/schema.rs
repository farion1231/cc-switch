use std::collections::HashSet;
use std::time::Duration;

use rusqlite::Connection;

/// 与桌面数据库 `src/database/mod.rs` 保持一致；Agent 不再维护独立版本号。
pub const DESKTOP_SCHEMA_VERSION: i32 = 16;

const CANONICAL_REQUIRED_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
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
CREATE TABLE IF NOT EXISTS provider_endpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    url TEXT NOT NULL,
    added_at INTEGER,
    FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS proxy_request_logs (
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
CREATE INDEX IF NOT EXISTS idx_request_logs_provider
    ON proxy_request_logs(provider_id, app_type);
CREATE INDEX IF NOT EXISTS idx_request_logs_created_at
    ON proxy_request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_request_logs_model
    ON proxy_request_logs(model);
CREATE INDEX IF NOT EXISTS idx_request_logs_session
    ON proxy_request_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_request_logs_status
    ON proxy_request_logs(status_code);
CREATE TABLE IF NOT EXISTS model_pricing (
    model_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    input_cost_per_million TEXT NOT NULL,
    output_cost_per_million TEXT NOT NULL,
    cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
    cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
);
CREATE TABLE IF NOT EXISTS usage_daily_rollups (
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
CREATE TABLE IF NOT EXISTS session_log_sync (
    file_path TEXT PRIMARY KEY,
    last_modified INTEGER NOT NULL,
    last_line_offset INTEGER NOT NULL DEFAULT 0,
    last_synced_at INTEGER NOT NULL
);
"#;

const REQUIRED_COLUMNS: &[(&str, &[&str])] = &[
    (
        "providers",
        &[
            "id",
            "app_type",
            "name",
            "settings_config",
            "website_url",
            "category",
            "created_at",
            "sort_index",
            "notes",
            "icon",
            "icon_color",
            "meta",
            "is_current",
            "in_failover_queue",
        ],
    ),
    (
        "provider_endpoints",
        &["id", "provider_id", "app_type", "url", "added_at"],
    ),
    (
        "proxy_request_logs",
        &[
            "request_id",
            "provider_id",
            "app_type",
            "model",
            "request_model",
            "pricing_model",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_creation_tokens",
            "input_token_semantics",
            "input_cost_usd",
            "output_cost_usd",
            "cache_read_cost_usd",
            "cache_creation_cost_usd",
            "total_cost_usd",
            "latency_ms",
            "first_token_ms",
            "duration_ms",
            "status_code",
            "error_message",
            "session_id",
            "provider_type",
            "is_streaming",
            "cost_multiplier",
            "created_at",
            "data_source",
        ],
    ),
    (
        "model_pricing",
        &[
            "model_id",
            "display_name",
            "input_cost_per_million",
            "output_cost_per_million",
            "cache_read_cost_per_million",
            "cache_creation_cost_per_million",
        ],
    ),
    (
        "usage_daily_rollups",
        &[
            "date",
            "app_type",
            "provider_id",
            "model",
            "request_model",
            "pricing_model",
            "request_count",
            "success_count",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_creation_tokens",
            "input_token_semantics",
            "total_cost_usd",
            "avg_latency_ms",
        ],
    ),
    (
        "session_log_sync",
        &[
            "file_path",
            "last_modified",
            "last_line_offset",
            "last_synced_at",
        ],
    ),
];

/// 配置每条 Core 连接的并发行为。busy timeout 有界等待另一个 CC Switch 进程释放写锁，
/// 避免瞬时竞争直接失败，也避免 SSH 会话无限挂起。
pub(crate) fn configure_connection(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

/// 全新远端没有数据库时只创建本阶段使用的规范表，列名和约束与桌面 v16 一致。
/// 已有数据库绝不能经过此入口，防止 `CREATE TABLE IF NOT EXISTS` 掩盖缺列或旧私有 schema。
pub(crate) fn initialize_new_database(connection: &Connection) -> Result<(), SchemaError> {
    connection.execute_batch(CANONICAL_REQUIRED_SCHEMA)?;
    connection.pragma_update(None, "user_version", DESKTOP_SCHEMA_VERSION)?;
    Ok(())
}

/// 对已有数据库执行只读兼容检查；Agent 不负责迁移用户数据库。
pub(crate) fn validate_existing_database(connection: &Connection) -> Result<(), SchemaError> {
    let detected = read_schema_version(connection)?;
    if detected != DESKTOP_SCHEMA_VERSION {
        return Err(SchemaError::Incompatible {
            detected,
            supported: DESKTOP_SCHEMA_VERSION,
            reason: "user_version 与 Agent 支持版本不一致".to_string(),
        });
    }

    for (table, required) in REQUIRED_COLUMNS {
        let actual = table_columns(connection, table)?;
        if actual.is_empty() {
            return Err(SchemaError::Incompatible {
                detected,
                supported: DESKTOP_SCHEMA_VERSION,
                reason: format!("缺少必需表 {table}"),
            });
        }
        if let Some(column) = required.iter().find(|column| !actual.contains(**column)) {
            return Err(SchemaError::Incompatible {
                detected,
                supported: DESKTOP_SCHEMA_VERSION,
                reason: format!("表 {table} 缺少必需列 {column}"),
            });
        }
    }
    Ok(())
}

pub(crate) fn read_schema_version(connection: &Connection) -> Result<i32, rusqlite::Error> {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn table_columns(connection: &Connection, table: &str) -> Result<HashSet<String>, rusqlite::Error> {
    // 表名来自上方编译期常量，不接收 RPC 或用户输入；SQLite 的 PRAGMA 表名无法参数绑定。
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get(1))?;
    rows.collect()
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("远端数据库结构不兼容: detected={detected}, supported={supported}, reason={reason}")]
    Incompatible {
        detected: i32,
        supported: i32,
        reason: String,
    },
    #[error("远端数据库结构检查失败: {0}")]
    Database(#[from] rusqlite::Error),
}
