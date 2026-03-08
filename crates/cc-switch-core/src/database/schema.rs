//! Database schema and migrations

use super::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::Connection;

/// Current schema version
#[allow(dead_code)]
pub const SCHEMA_VERSION: i32 = 10;

impl Database {
    /// Create all database tables
    pub(crate) fn create_tables(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::create_tables_on_conn(&conn)
    }

    pub(crate) fn create_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 1. Providers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS providers (
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
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 2. Provider endpoints table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                url TEXT NOT NULL,
                added_at INTEGER,
                last_used INTEGER,
                FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 3. Universal Providers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS universal_providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                provider_type TEXT NOT NULL,
                apps TEXT NOT NULL DEFAULT '{}',
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                models TEXT NOT NULL DEFAULT '{}',
                website_url TEXT,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER,
                sort_index INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 4. MCP Servers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                server_config TEXT NOT NULL,
                description TEXT,
                homepage TEXT,
                docs TEXT,
                tags TEXT NOT NULL DEFAULT '[]',
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
                enabled_openclaw BOOLEAN NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 5. Prompts table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS prompts (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                description TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                PRIMARY KEY (id, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 6. Skills table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                directory TEXT NOT NULL,
                repo_owner TEXT,
                repo_name TEXT,
                repo_branch TEXT DEFAULT 'main',
                readme_url TEXT,
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
                enabled_openclaw BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 7. Skill repos table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_repos (
                owner TEXT NOT NULL,
                name TEXT NOT NULL,
                branch TEXT NOT NULL DEFAULT 'main',
                enabled BOOLEAN NOT NULL DEFAULT 1,
                PRIMARY KEY (owner, name)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 8. Settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 9. Proxy Config table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_config (
                app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini')),
                proxy_enabled INTEGER NOT NULL DEFAULT 0,
                listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
                listen_port INTEGER NOT NULL DEFAULT 15721,
                enable_logging INTEGER NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 0,
                auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 3,
                streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
                streaming_idle_timeout INTEGER NOT NULL DEFAULT 120,
                non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
                circuit_failure_threshold INTEGER NOT NULL DEFAULT 4,
                circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
                circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60,
                circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
                circuit_min_requests INTEGER NOT NULL DEFAULT 10,
                default_cost_multiplier TEXT NOT NULL DEFAULT '1',
                pricing_model_source TEXT NOT NULL DEFAULT 'response',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 10. Provider Health table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_health (
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                is_healthy INTEGER NOT NULL DEFAULT 1,
                consecutive_failures INTEGER NOT NULL DEFAULT 0,
                last_success_at TEXT,
                last_failure_at TEXT,
                last_error TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (provider_id, app_type),
                FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 11. Proxy Request Logs table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                model TEXT NOT NULL,
                request_model TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
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
                created_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Indexes for request logs
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_provider ON proxy_request_logs(provider_id, app_type)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON proxy_request_logs(created_at)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 12. Model pricing table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
                model_id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL,
                output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 13. Stream check logs table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS stream_check_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                provider_name TEXT NOT NULL,
                app_type TEXT NOT NULL,
                status TEXT NOT NULL,
                success INTEGER NOT NULL,
                message TEXT NOT NULL,
                response_time_ms INTEGER,
                http_status INTEGER,
                model_used TEXT,
                retry_count INTEGER DEFAULT 0,
                tested_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_stream_check_logs_provider
             ON stream_check_logs(app_type, provider_id, tested_at DESC)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 14. Proxy live backup table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_live_backup (
                app_type TEXT PRIMARY KEY,
                original_config TEXT NOT NULL,
                backed_up_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Self::apply_schema_migrations_on_conn(conn)?;

        // Initialize proxy_config rows
        Self::init_proxy_config_rows(conn)?;
        Self::ensure_model_pricing_seeded_on_conn(conn)?;

        Ok(())
    }

    fn init_proxy_config_rows(conn: &Connection) -> Result<(), AppError> {
        let apps = ["claude", "codex", "gemini"];
        for app in apps {
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type) VALUES (?)",
                [app],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn apply_schema_migrations_on_conn(conn: &Connection) -> Result<(), AppError> {
        Self::add_column_if_missing(conn, "provider_endpoints", "last_used", "INTEGER")?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_openclaw",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "skills",
            "enabled_openclaw",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_repos (
                owner TEXT NOT NULL,
                name TEXT NOT NULL,
                branch TEXT NOT NULL DEFAULT 'main',
                enabled BOOLEAN NOT NULL DEFAULT 1,
                PRIMARY KEY (owner, name)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Self::add_column_if_missing(
            conn,
            "skill_repos",
            "branch",
            "TEXT NOT NULL DEFAULT 'main'",
        )?;
        Self::add_column_if_missing(conn, "skill_repos", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        Self::add_column_if_missing(
            conn,
            "proxy_config",
            "default_cost_multiplier",
            "TEXT NOT NULL DEFAULT '1'",
        )?;
        Self::add_column_if_missing(
            conn,
            "proxy_config",
            "pricing_model_source",
            "TEXT NOT NULL DEFAULT 'response'",
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
                model_id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL,
                output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS stream_check_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                provider_name TEXT NOT NULL,
                app_type TEXT NOT NULL,
                status TEXT NOT NULL,
                success INTEGER NOT NULL,
                message TEXT NOT NULL,
                response_time_ms INTEGER,
                http_status INTEGER,
                model_used TEXT,
                retry_count INTEGER DEFAULT 0,
                tested_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_stream_check_logs_provider
             ON stream_check_logs(app_type, provider_id, tested_at DESC)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_live_backup (
                app_type TEXT PRIMARY KEY,
                original_config TEXT NOT NULL,
                backed_up_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    fn seed_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_data = [
            (
                "claude-opus-4-6-20260206",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-opus-4-5-20251101",
                "Claude Opus 4.5",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-5-20250929",
                "Claude Sonnet 4.5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-haiku-4-5-20251001",
                "Claude Haiku 4.5",
                "1",
                "5",
                "0.10",
                "1.25",
            ),
            (
                "claude-opus-4-20250514",
                "Claude Opus 4",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-opus-4-1-20250805",
                "Claude Opus 4.1",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-sonnet-4-20250514",
                "Claude Sonnet 4",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku",
                "0.80",
                "4",
                "0.08",
                "1",
            ),
            (
                "claude-3-5-sonnet-20241022",
                "Claude 3.5 Sonnet",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            ("gpt-5.2", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-low", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-medium", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-high", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-xhigh", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-codex", "GPT-5.2 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.2-codex-low",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-medium",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-high",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-xhigh",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            ("gpt-5.3-codex", "GPT-5.3 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.3-codex-low",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-medium",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-high",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-xhigh",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            ("gpt-5.1", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-low", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-medium", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-high", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-minimal", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-codex", "GPT-5.1 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5.1-codex-mini",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-high",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-xhigh",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            ("gpt-5", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-low", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-medium", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-high", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-minimal", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex-low", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5-codex-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gemini-3-pro-preview",
                "Gemini 3 Pro Preview",
                "2",
                "12",
                "0.2",
                "0",
            ),
            (
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "0.5",
                "3",
                "0.05",
                "0",
            ),
            (
                "gemini-2.5-pro",
                "Gemini 2.5 Pro",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                "0.3",
                "2.5",
                "0.03",
                "0",
            ),
            (
                "doubao-seed-code",
                "Doubao Seed Code",
                "1.20",
                "8.00",
                "0.24",
                "0",
            ),
            (
                "deepseek-v3.2",
                "DeepSeek V3.2",
                "2.00",
                "3.00",
                "0.40",
                "0",
            ),
            (
                "deepseek-v3.1",
                "DeepSeek V3.1",
                "4.00",
                "12.00",
                "0.80",
                "0",
            ),
            ("deepseek-v3", "DeepSeek V3", "2.00", "8.00", "0.40", "0"),
            (
                "kimi-k2-thinking",
                "Kimi K2 Thinking",
                "4.00",
                "16.00",
                "1.00",
                "0",
            ),
            ("kimi-k2-0905", "Kimi K2", "4.00", "16.00", "1.00", "0"),
            (
                "kimi-k2-turbo",
                "Kimi K2 Turbo",
                "8.00",
                "58.00",
                "1.00",
                "0",
            ),
            ("minimax-m2.1", "MiniMax M2.1", "2.10", "8.40", "0.21", "0"),
            (
                "minimax-m2.1-lightning",
                "MiniMax M2.1 Lightning",
                "2.10",
                "16.80",
                "0.21",
                "0",
            ),
            ("minimax-m2", "MiniMax M2", "2.10", "8.40", "0.21", "0"),
            ("glm-4.7", "GLM-4.7", "2.00", "8.00", "0.40", "0"),
            ("glm-4.6", "GLM-4.6", "2.00", "8.00", "0.40", "0"),
            ("mimo-v2-flash", "Mimo V2 Flash", "0", "0", "0", "0"),
        ];

        for (model_id, display_name, input, output, cache_read, cache_creation) in pricing_data {
            conn.execute(
                "INSERT OR IGNORE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    model_id,
                    display_name,
                    input,
                    output,
                    cache_read,
                    cache_creation
                ],
            )
            .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        }

        Ok(())
    }

    pub fn ensure_model_pricing_seeded(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::ensure_model_pricing_seeded_on_conn(&conn)
    }

    pub fn apply_schema_migrations(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::apply_schema_migrations_on_conn(&conn)
    }

    fn ensure_model_pricing_seeded_on_conn(conn: &Connection) -> Result<(), AppError> {
        Self::seed_model_pricing(conn)
    }

    fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
        let sql = format!("PRAGMA table_info(\"{table}\")");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(e.to_string()))?;

        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row.get(1).map_err(|e| AppError::Database(e.to_string()))?;
            if name == column {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<(), AppError> {
        if Self::has_column(conn, table, column)? {
            return Ok(());
        }

        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition}");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
