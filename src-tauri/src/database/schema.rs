//! Schema 定义和迁移
//!
//! 负责数据库表结构的创建和版本迁移。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Serialize)]
struct LegacySkillMigrationRow {
    directory: String,
    app_type: String,
}

/// Names for one complete canonical-generation table set. These are internal
/// constants, so the DDL builder never interpolates user-controlled SQL.
#[derive(Clone, Copy)]
struct CanonicalGenerationTables {
    nodes: &'static str,
    rollups: &'static str,
    snapshots: Option<&'static str>,
    coverage: &'static str,
    label: &'static str,
}

const PUBLISHED_CANONICAL_GENERATION_TABLES: CanonicalGenerationTables =
    CanonicalGenerationTables {
        nodes: "agent_session_nodes",
        rollups: "agent_session_usage_rollups",
        snapshots: Some("agent_session_usage_snapshots"),
        coverage: "agent_session_canonical_coverage",
        label: "Agent 会话用量最终",
    };

const CODEX_REPLAY_CANONICAL_GENERATION_TABLES: CanonicalGenerationTables =
    CanonicalGenerationTables {
        nodes: "codex_replay_nodes",
        rollups: "codex_replay_rollups",
        snapshots: None,
        coverage: "codex_replay_coverage",
        label: "Codex 重放影子",
    };

impl Database {
    /// 创建所有数据库表
    pub(crate) fn create_tables(&self, is_new_database: bool) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::create_tables_on_conn(&conn, is_new_database)
    }

    /// 在指定连接上创建表（供迁移和测试使用）
    pub(crate) fn create_tables_on_conn(
        conn: &Connection,
        is_new_database: bool,
    ) -> Result<(), AppError> {
        // 1. Providers 表
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

        // 2. Provider Endpoints 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                url TEXT NOT NULL,
                added_at INTEGER,
                FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 3. MCP Servers 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_servers (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
            description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 4. Prompts 表
        conn.execute("CREATE TABLE IF NOT EXISTS prompts (
            id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL, content TEXT NOT NULL,
            description TEXT, enabled BOOLEAN NOT NULL DEFAULT 1, created_at INTEGER, updated_at INTEGER,
            PRIMARY KEY (id, app_type)
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 5. Skills 表（v3.10.0+ 统一结构）
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
            enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0,
            installed_at INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 6. Skill Repos 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_repos (
            owner TEXT NOT NULL, name TEXT NOT NULL, branch TEXT NOT NULL DEFAULT 'main',
            enabled BOOLEAN NOT NULL DEFAULT 1, PRIMARY KEY (owner, name)
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 7. Settings 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 8. Proxy Config 表（三行结构，app_type 主键）
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_config (
            app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
            proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 初始化三行数据（每应用不同默认值）
        //
        // 兼容旧数据库：
        // - 老版本 proxy_config 是单例表（没有 app_type 列），此时不能执行三行 seed insert；
        // - 旧表会在 apply_schema_migrations() 中迁移为三行结构后再插入。
        if Self::has_column(conn, "proxy_config", "app_type")? {
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('claude', 6, 90, 180, 600, 8, 3, 90, 0.7, 15)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('codex', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('gemini', 5, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('grokbuild', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        // 9. Provider Health 表
        conn.execute("CREATE TABLE IF NOT EXISTS provider_health (
            provider_id TEXT NOT NULL, app_type TEXT NOT NULL, is_healthy INTEGER NOT NULL DEFAULT 1,
            consecutive_failures INTEGER NOT NULL DEFAULT 0, last_success_at TEXT, last_failure_at TEXT,
            last_error TEXT, updated_at TEXT NOT NULL,
            PRIMARY KEY (provider_id, app_type),
            FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 10. Proxy Request Logs 表
        // pricing_model = 写入时实际用于计价的模型名（pricing_model_source 解析结果），
        // 回填按它重算；NULL 表示 v11 之前的历史行，'' 表示未计价的错误行。
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_request_logs (
            request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
            request_model TEXT,
            pricing_model TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            input_token_semantics INTEGER NOT NULL DEFAULT 0,
            input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
            cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
            total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
            provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
            cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL,
            data_source TEXT NOT NULL DEFAULT 'proxy'
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("CREATE INDEX IF NOT EXISTS idx_request_logs_provider ON proxy_request_logs(provider_id, app_type)", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON proxy_request_logs(created_at)", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_model ON proxy_request_logs(model)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_session ON proxy_request_logs(session_id)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_status ON proxy_request_logs(status_code)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Self::create_request_logs_usage_indexes_if_supported(conn)?;

        // 11. Model Pricing 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
            model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
            cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
            cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 12. Stream Check Logs 表
        conn.execute("CREATE TABLE IF NOT EXISTS stream_check_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, provider_id TEXT NOT NULL, provider_name TEXT NOT NULL,
            app_type TEXT NOT NULL, status TEXT NOT NULL, success INTEGER NOT NULL, message TEXT NOT NULL,
            response_time_ms INTEGER, http_status INTEGER, model_used TEXT,
            retry_count INTEGER DEFAULT 0, tested_at INTEGER NOT NULL
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_stream_check_logs_provider
             ON stream_check_logs(app_type, provider_id, tested_at DESC)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 注意：circuit_breaker_config 已合并到 proxy_config 表中

        // 16. Proxy Live Backup 表 (Live 配置备份)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_live_backup (
            app_type TEXT PRIMARY KEY, original_config TEXT NOT NULL, backed_up_at TEXT NOT NULL
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 17. Usage Daily Rollups 表 (日聚合统计)
        // request_model 保留路由接管的「客户端别名 → 真实模型」映射维度，
        // pricing_model 保留写入时的计价基准（request 计价模式下与 model 分叉），
        // 否则明细被 prune 后接管计费不可审计；历史行迁移时填 ''（未知）。
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
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
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 18. Session Log Sync 表 (会话日志同步状态)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_log_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Session detail rows are pruned after rollup, so request IDs needed
        // for fork/rewrite deduplication live in a compact durable ledger.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_usage_dedup (
                data_source TEXT NOT NULL,
                request_id TEXT NOT NULL,
                semantic_id TEXT NOT NULL,
                has_entry_id INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (data_source, request_id)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_usage_dedup_semantic
             ON session_usage_dedup(data_source, semantic_id, has_entry_id)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 19. Profiles 表（全应用共享的项目实体，payload 按 app 分槽快照
        //     供应商/MCP/Skills/Prompt；各应用分组的 current 标记在 settings 表）
        conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                sort_order INTEGER,
                created_at INTEGER,
                updated_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Keep fresh database creation and the only supported v17 -> v18
        // migration on one final DDL definition. Existing pre-v18 databases
        // must not receive these objects before the migration savepoint; doing
        // so would make a failed migration leave a partial v18 schema behind.
        let version = Self::get_user_version(conn)?;
        if is_new_database || version == SCHEMA_VERSION {
            Self::create_agent_session_usage_tables_on_conn(conn)?;

            // Codex replay staging tables. A replay writes here first and
            // publishes the complete generation atomically, so readers never
            // observe a partially rebuilt set of nodes, rollups, or coverage.
            Self::create_codex_replay_tables_on_conn(conn)?;
        }

        // 修复跑过未发布开发版的库：current 标记曾是全局 key，现按应用分组
        // （随 v12 定稿为 current_profile_id_<scope>，不单独 bump 版本）
        if conn
            .execute(
                "INSERT OR REPLACE INTO settings (key, value)
                 SELECT 'current_profile_id_claude', value FROM settings
                 WHERE key = 'current_profile_id'",
                [],
            )
            .is_ok()
        {
            let _ = conn.execute("DELETE FROM settings WHERE key = 'current_profile_id'", []);
        }

        // 尝试添加 live_takeover_active 列到 proxy_config 表
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN live_takeover_active INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // 尝试添加基础配置列到 proxy_config 表（兼容 v3.9.0-2 升级）
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN proxy_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN listen_address TEXT NOT NULL DEFAULT '127.0.0.1'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN listen_port INTEGER NOT NULL DEFAULT 15721",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN enable_logging INTEGER NOT NULL DEFAULT 1",
            [],
        );

        // 尝试添加超时配置列到 proxy_config 表
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN streaming_idle_timeout INTEGER NOT NULL DEFAULT 120",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN non_streaming_timeout INTEGER NOT NULL DEFAULT 600",
            [],
        );

        // 兼容：若旧版 proxy_config 仍为单例结构（无 app_type），则在启动时直接转换为三行结构
        // 说明：user_version=2 时不会再触发 v1->v2 迁移，但新代码查询依赖 app_type 列。
        if Self::table_exists(conn, "proxy_config")?
            && !Self::has_column(conn, "proxy_config", "app_type")?
        {
            Self::migrate_proxy_config_to_per_app(conn)?;
        }

        // 确保 in_failover_queue 列存在（对于已存在的 v2 数据库）
        Self::add_column_if_missing(
            conn,
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 删除旧的 failover_queue 表（如果存在）
        let _ = conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", []);
        let _ = conn.execute("DROP TABLE IF EXISTS failover_queue", []);

        // 为故障转移队列创建索引（基于 providers 表）
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        );

        Ok(())
    }

    /// 应用 Schema 迁移
    pub(crate) fn apply_schema_migrations(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::apply_schema_migrations_on_conn(&conn)
    }

    /// 在指定连接上应用 Schema 迁移
    pub(crate) fn apply_schema_migrations_on_conn(conn: &Connection) -> Result<(), AppError> {
        conn.execute("SAVEPOINT schema_migration;", [])
            .map_err(|e| AppError::Database(format!("开启迁移 savepoint 失败: {e}")))?;

        let mut version = Self::get_user_version(conn)?;

        if version > SCHEMA_VERSION {
            conn.execute("ROLLBACK TO schema_migration;", []).ok();
            conn.execute("RELEASE schema_migration;", []).ok();
            return Err(AppError::Database(format!(
                "数据库版本过新（{version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再尝试。"
            )));
        }

        let result = (|| {
            while version < SCHEMA_VERSION {
                match version {
                    0 => {
                        log::info!("检测到 user_version=0，迁移到 1（补齐缺失列并设置版本）");
                        Self::migrate_v0_to_v1(conn)?;
                        Self::set_user_version(conn, 1)?;
                    }
                    1 => {
                        log::info!(
                            "迁移数据库从 v1 到 v2（添加使用统计表和完整字段，重构 skills 表）"
                        );
                        Self::migrate_v1_to_v2(conn)?;
                        Self::set_user_version(conn, 2)?;
                    }
                    2 => {
                        log::info!("迁移数据库从 v2 到 v3（Skills 统一管理架构）");
                        Self::migrate_v2_to_v3(conn)?;
                        Self::set_user_version(conn, 3)?;
                    }
                    3 => {
                        log::info!("迁移数据库从 v3 到 v4（OpenCode 支持）");
                        Self::migrate_v3_to_v4(conn)?;
                        Self::set_user_version(conn, 4)?;
                    }
                    4 => {
                        log::info!("迁移数据库从 v4 到 v5（计费模式支持）");
                        Self::migrate_v4_to_v5(conn)?;
                        Self::set_user_version(conn, 5)?;
                    }
                    5 => {
                        log::info!("迁移数据库从 v5 到 v6（使用量聚合表 + Copilot 模板类型统一）");
                        Self::migrate_v5_to_v6(conn)?;
                        Self::set_user_version(conn, 6)?;
                    }
                    6 => {
                        log::info!("迁移数据库从 v6 到 v7（Skills 更新检测支持）");
                        Self::migrate_v6_to_v7(conn)?;
                        Self::set_user_version(conn, 7)?;
                    }
                    7 => {
                        log::info!("迁移数据库从 v7 到 v8（会话日志使用追踪 + 修正模型定价）");
                        Self::migrate_v7_to_v8(conn)?;
                        Self::set_user_version(conn, 8)?;
                    }
                    8 => {
                        log::info!("迁移数据库从 v8 到 v9（全面补充模型定价）");
                        Self::migrate_v8_to_v9(conn)?;
                        Self::set_user_version(conn, 9)?;
                    }
                    9 => {
                        log::info!("迁移数据库从 v9 到 v10（添加 Hermes Agent 支持）");
                        Self::migrate_v9_to_v10(conn)?;
                        Self::set_user_version(conn, 10)?;
                    }
                    10 => {
                        log::info!("迁移数据库从 v10 到 v11（usage_daily_rollups 保留 request_model 维度）");
                        Self::migrate_v10_to_v11(conn)?;
                        Self::set_user_version(conn, 11)?;
                    }
                    11 => {
                        log::info!("迁移数据库从 v11 到 v12（添加项目 Profiles 表）");
                        Self::migrate_v11_to_v12(conn)?;
                        Self::set_user_version(conn, 12)?;
                    }
                    12 => {
                        log::info!("迁移数据库从 v12 到 v13（记录输入 token 缓存语义）");
                        Self::migrate_v12_to_v13(conn)?;
                        Self::set_user_version(conn, 13)?;
                    }
                    13 => {
                        log::info!("迁移数据库从 v13 到 v14（添加 Grok Build 代理配置）");
                        Self::migrate_v13_to_v14(conn)?;
                        Self::set_user_version(conn, 14)?;
                    }
                    14 => {
                        log::info!("迁移数据库从 v14 到 v15（Skills/MCP 添加 Grok Build 支持）");
                        Self::migrate_v14_to_v15(conn)?;
                        Self::set_user_version(conn, 15)?;
                    }
                    15 => {
                        log::info!("迁移数据库从 v15 到 v16（重建 Codex 会话用量）");
                        Self::migrate_v15_to_v16(conn)?;
                        Self::set_user_version(conn, 16)?;
                    }
                    16 => {
                        log::info!("迁移数据库从 v16 到 v17（添加会话用量持久去重账本）");
                        Self::migrate_v16_to_v17(conn)?;
                        Self::set_user_version(conn, 17)?;
                    }
                    17 => {
                        log::info!("迁移数据库从 v17 到 v18（完成 Agent 会话用量 Schema 与 Codex replay 暂存表）");
                        Self::migrate_v17_to_v18(conn)?;
                        Self::set_user_version(conn, 18)?;
                    }
                    _ => {
                        return Err(AppError::Database(format!(
                            "未知的数据库版本 {version}，无法迁移到 {SCHEMA_VERSION}"
                        )));
                    }
                }
                version = Self::get_user_version(conn)?;
            }
            Ok(())
        })();

        match result {
            Ok(_) => {
                conn.execute("RELEASE schema_migration;", [])
                    .map_err(|e| AppError::Database(format!("提交迁移 savepoint 失败: {e}")))?;
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK TO schema_migration;", []).ok();
                conn.execute("RELEASE schema_migration;", []).ok();
                Err(e)
            }
        }
    }

    /// v0 -> v1 迁移：补齐所有缺失列
    fn migrate_v0_to_v1(conn: &Connection) -> Result<(), AppError> {
        // providers 表
        Self::add_column_if_missing(conn, "providers", "category", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "sort_index", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "notes", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon_color", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "meta", "TEXT NOT NULL DEFAULT '{}'")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "is_current",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // provider_endpoints 表
        Self::add_column_if_missing(conn, "provider_endpoints", "added_at", "INTEGER")?;

        // mcp_servers 表
        Self::add_column_if_missing(conn, "mcp_servers", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "homepage", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "docs", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_codex",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_gemini",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // prompts 表
        Self::add_column_if_missing(conn, "prompts", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "prompts", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        Self::add_column_if_missing(conn, "prompts", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "prompts", "updated_at", "INTEGER")?;

        // skills 表
        Self::add_column_if_missing(conn, "skills", "installed_at", "INTEGER NOT NULL DEFAULT 0")?;

        // skill_repos 表
        Self::add_column_if_missing(
            conn,
            "skill_repos",
            "branch",
            "TEXT NOT NULL DEFAULT 'main'",
        )?;
        Self::add_column_if_missing(conn, "skill_repos", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        // 注意: skills_path 字段已被移除，因为现在支持全仓库递归扫描

        Ok(())
    }

    /// v1 -> v2 迁移：添加使用统计表和完整字段，重构 skills 表
    fn migrate_v1_to_v2(conn: &Connection) -> Result<(), AppError> {
        // providers 表字段
        Self::add_column_if_missing(
            conn,
            "providers",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "providers", "limit_daily_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "limit_monthly_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 添加代理超时配置字段
        if Self::table_exists(conn, "proxy_config")? {
            // 兼容旧版本缺失的基础字段
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "proxy_enabled",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_address",
                "TEXT NOT NULL DEFAULT '127.0.0.1'",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_port",
                "INTEGER NOT NULL DEFAULT 15721",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "enable_logging",
                "INTEGER NOT NULL DEFAULT 1",
            )?;

            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_first_byte_timeout",
                "INTEGER NOT NULL DEFAULT 60",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_idle_timeout",
                "INTEGER NOT NULL DEFAULT 120",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "non_streaming_timeout",
                "INTEGER NOT NULL DEFAULT 600",
            )?;
        }

        // 删除旧的 failover_queue 表（如果存在）
        conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 索引失败: {e}")))?;
        conn.execute("DROP TABLE IF EXISTS failover_queue", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 表失败: {e}")))?;

        // 创建 failover 索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 failover 索引失败: {e}")))?;

        // proxy_request_logs 表
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_request_logs (
            request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
            request_model TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            input_token_semantics INTEGER NOT NULL DEFAULT 0,
            input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
            cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
            total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
            provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
            cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL
        )", [])?;

        // 为已存在的表添加新字段
        Self::add_column_if_missing(conn, "proxy_request_logs", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "is_streaming",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "first_token_ms", "INTEGER")?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "duration_ms", "INTEGER")?;

        // model_pricing 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
            model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
            cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
            cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
        )",
            [],
        )?;

        // 清空并重新插入模型定价
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;

        // 重构 skills 表（添加 app_type 字段）
        Self::migrate_skills_table(conn)?;

        // 重构 proxy_config 为三行结构（每应用独立配置）
        Self::migrate_proxy_config_to_per_app(conn)?;

        Ok(())
    }

    /// 将 proxy_config 迁移为三行结构（每应用独立配置）
    fn migrate_proxy_config_to_per_app(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新表结构（幂等性）
        if !Self::table_exists(conn, "proxy_config")? {
            // 表不存在，跳过迁移（新安装）
            return Ok(());
        }

        if Self::has_column(conn, "proxy_config", "app_type")? {
            // 已经是三行结构，跳过迁移
            log::info!("proxy_config 已经是三行结构，跳过迁移");
            return Ok(());
        }

        // 读取旧配置
        let old_config = conn
            .query_row(
                "SELECT listen_address, listen_port, max_retries, enable_logging,
                    streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout
             FROM proxy_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i32>(3)?,
                        row.get::<_, i32>(4).unwrap_or(30),
                        row.get::<_, i32>(5).unwrap_or(60),
                        row.get::<_, i32>(6).unwrap_or(300),
                    ))
                },
            )
            .unwrap_or_else(|_| ("127.0.0.1".to_string(), 5000, 3, 1, 30, 60, 300));

        let old_cb = conn.query_row(
            "SELECT failure_threshold, success_threshold, timeout_seconds, error_rate_threshold, min_requests
             FROM circuit_breaker_config WHERE id = 1", [],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?, row.get::<_, i64>(2)?,
                      row.get::<_, f64>(3)?, row.get::<_, i32>(4)?))
        ).unwrap_or((5, 2, 60, 0.5, 10));

        let get_bool = |key: &str| -> bool {
            conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| {
                r.get::<_, String>(0)
            })
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        };

        let apps = [
            (
                "claude",
                get_bool("proxy_takeover_claude"),
                get_bool("auto_failover_enabled_claude"),
                6,
                45,
                90,
                8,
                3,
                90,
                0.6,
                15,
            ),
            (
                "codex",
                get_bool("proxy_takeover_codex"),
                get_bool("auto_failover_enabled_codex"),
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "gemini",
                get_bool("proxy_takeover_gemini"),
                get_bool("auto_failover_enabled_gemini"),
                5,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "grokbuild",
                false,
                false,
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
        ];

        // 创建新表
        conn.execute("DROP TABLE IF EXISTS proxy_config_new", [])?;
        conn.execute("CREATE TABLE proxy_config_new (
            app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
            proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            live_takeover_active INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )", [])?;

        // 插入三行配置
        for (app, takeover, failover, retries, fb, idle, cb_f, cb_s, cb_t, cb_r, cb_m) in apps {
            conn.execute(
                "INSERT INTO proxy_config_new (app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                 enabled, auto_failover_enabled, max_retries, streaming_first_byte_timeout, streaming_idle_timeout,
                 non_streaming_timeout, circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                 circuit_error_rate_threshold, circuit_min_requests)
                 VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![app, old_config.0, old_config.1, old_config.3,
                    if takeover { 1 } else { 0 }, if failover { 1 } else { 0 },
                    retries, fb, idle, old_config.6, cb_f, cb_s, cb_t, cb_r, cb_m]
            ).map_err(|e| AppError::Database(format!("插入 {app} 配置失败: {e}")))?;
        }

        // 替换表并清理
        conn.execute("DROP TABLE IF EXISTS proxy_config", [])?;
        conn.execute("ALTER TABLE proxy_config_new RENAME TO proxy_config", [])?;
        conn.execute("DROP TABLE IF EXISTS circuit_breaker_config", [])?;
        conn.execute("DELETE FROM settings WHERE key LIKE 'proxy_takeover_%'", [])?;
        conn.execute(
            "DELETE FROM settings WHERE key LIKE 'auto_failover_enabled_%'",
            [],
        )?;

        log::info!("proxy_config 已迁移为三行结构");
        Ok(())
    }

    /// 迁移 skills 表：从单 key 主键改为 (directory, app_type) 复合主键
    fn migrate_skills_table(conn: &Connection) -> Result<(), AppError> {
        // v3 结构（统一管理架构）已经是更高版本的 skills 表：
        // - 主键为 id
        // - 包含 enabled_claude / enabled_codex / enabled_gemini 等列
        // 在这种情况下，不应再执行 v1 -> v2 的迁移逻辑，否则会因列不匹配而失败。
        if Self::has_column(conn, "skills", "enabled_claude")?
            || Self::has_column(conn, "skills", "id")?
        {
            log::info!("skills 表已经是 v3 结构，跳过 v1 -> v2 迁移");
            return Ok(());
        }

        // 检查是否已经是新表结构
        if Self::has_column(conn, "skills", "app_type")? {
            log::info!("skills 表已经包含 app_type 字段，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表...");

        // 1. 重命名旧表
        conn.execute("ALTER TABLE skills RENAME TO skills_old", [])
            .map_err(|e| AppError::Database(format!("重命名旧 skills 表失败: {e}")))?;

        // 2. 创建新表
        conn.execute(
            "CREATE TABLE skills (
                directory TEXT NOT NULL,
                app_type TEXT NOT NULL,
                installed BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (directory, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        // 3. 迁移数据：解析 key 格式（如 "claude:my-skill" 或 "codex:foo"）
        //    旧数据如果没有前缀，默认为 claude
        let mut stmt = conn
            .prepare("SELECT key, installed, installed_at FROM skills_old")
            .map_err(|e| AppError::Database(format!("查询旧 skills 数据失败: {e}")))?;

        let old_skills: Vec<(String, bool, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 数据失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 数据失败: {e}")))?;

        let count = old_skills.len();

        for (key, installed, installed_at) in old_skills {
            // 解析 key: "app:directory" 或 "directory"（默认 claude）
            let (app_type, directory) = if let Some(idx) = key.find(':') {
                let (app, dir) = key.split_at(idx);
                (app.to_string(), dir[1..].to_string()) // 跳过冒号
            } else {
                ("claude".to_string(), key.clone())
            };

            conn.execute(
                "INSERT INTO skills (directory, app_type, installed, installed_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![directory, app_type, installed, installed_at],
            )
            .map_err(|e| {
                AppError::Database(format!("迁移 skill {key} 到新表失败: {e}"))
            })?;
        }

        // 4. 删除旧表
        conn.execute("DROP TABLE skills_old", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        log::info!("skills 表迁移完成，共迁移 {count} 条记录");
        Ok(())
    }

    /// v2 -> v3 迁移：Skills 统一管理架构
    ///
    /// 将 skills 表从 (directory, app_type) 复合主键结构迁移到统一的 id 主键结构，
    /// 支持三应用启用标志（enabled_claude, enabled_codex, enabled_gemini）。
    ///
    /// 迁移策略：
    /// 1. 旧数据库只存储安装记录，真正的 skill 文件在文件系统
    /// 2. 直接重建新表结构，后续由 SkillService 在首次启动时扫描文件系统重建数据
    fn migrate_v2_to_v3(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新结构（通过检查是否有 enabled_claude 列）
        if Self::has_column(conn, "skills", "enabled_claude")? {
            log::info!("skills 表已经是 v3 结构，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表到 v3 结构（统一管理架构）...");

        // 1. 备份旧数据（用于日志和后续启动迁移）
        let old_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .unwrap_or(0);
        log::info!("旧 skills 表有 {old_count} 条记录");

        let mut stmt = conn
            .prepare(
                "SELECT directory, app_type FROM skills
                 WHERE installed = 1",
            )
            .map_err(|e| AppError::Database(format!("查询旧 skills 快照失败: {e}")))?;
        let snapshot_rows: Vec<LegacySkillMigrationRow> = stmt
            .query_map([], |row| {
                Ok(LegacySkillMigrationRow {
                    directory: row.get(0)?,
                    app_type: row.get(1)?,
                })
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 快照失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 快照失败: {e}")))?;
        let snapshot_json = serde_json::to_string(&snapshot_rows)
            .map_err(|e| AppError::Database(format!("序列化旧 skills 快照失败: {e}")))?;

        // 标记：需要在启动后从文件系统扫描并重建 Skills 数据
        // 说明：v3 结构将 Skills 的 SSOT 迁移到 ~/.cc-switch/skills/，
        // 旧表只存“安装记录”，无法直接无损迁移到新结构，因此改为启动后扫描 app 目录导入。
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_pending', 'true')",
            [],
        );
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_snapshot', ?1)",
            [snapshot_json],
        );

        // 2. 删除旧表
        conn.execute("DROP TABLE IF EXISTS skills", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        // 3. 创建新表
        conn.execute(
            "CREATE TABLE skills (
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
                installed_at INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        log::info!(
            "skills 表已迁移到 v3 结构。\n\
             注意：旧的安装记录已清除，首次启动时将自动扫描文件系统重建数据。"
        );

        Ok(())
    }

    /// v3 -> v4 迁移：添加 OpenCode 支持
    ///
    /// 为 mcp_servers 和 skills 表添加 enabled_opencode 列。
    fn migrate_v3_to_v4(conn: &Connection) -> Result<(), AppError> {
        // 为 mcp_servers 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 为 skills 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "skills",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        log::info!("v3 -> v4 迁移完成：已添加 OpenCode 支持");
        Ok(())
    }

    /// v4 -> v5 迁移：新增计费模式配置与请求模型字段
    fn migrate_v4_to_v5(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_config")? {
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
        }
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "request_model", "TEXT")?;
        }

        log::info!("v4 -> v5 迁移完成：已添加计费模式与请求模型字段");
        Ok(())
    }

    /// v5 -> v6 迁移：添加使用量日聚合表 + 统一 Copilot 模板类型
    fn migrate_v5_to_v6(conn: &Connection) -> Result<(), AppError> {
        // 1. 添加使用量日聚合表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
                date TEXT NOT NULL,
                app_type TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                request_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                total_cost_usd TEXT NOT NULL DEFAULT '0',
                avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, app_type, provider_id, model)
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 usage_daily_rollups 表失败: {e}")))?;

        // 2. 统一 Copilot 模板类型为 github_copilot
        let mut stmt = conn
            .prepare("SELECT id, app_type, meta FROM providers")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut updates = Vec::new();
        for row in rows {
            let (id, app_type, meta_str) = row.map_err(|e| AppError::Database(e.to_string()))?;

            if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&meta_str) {
                let mut updated = false;

                if let Some(usage_script) = meta.get_mut("usage_script") {
                    if let Some(template_type) = usage_script.get_mut("template_type") {
                        if template_type == "copilot" {
                            *template_type =
                                serde_json::Value::String("github_copilot".to_string());
                            updated = true;
                        }
                    }
                }

                if updated {
                    let new_meta_str = serde_json::to_string(&meta)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    updates.push((id, app_type, new_meta_str));
                }
            }
        }

        for (id, app_type, new_meta) in updates {
            conn.execute(
                "UPDATE providers SET meta = ?1 WHERE id = ?2 AND app_type = ?3",
                params![new_meta, id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        log::info!("v5 -> v6 迁移完成：已添加使用量日聚合表，统一 copilot 模板类型");
        Ok(())
    }

    /// v6 -> v7: Skills 更新检测支持（content_hash + updated_at）
    fn migrate_v6_to_v7(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(conn, "skills", "content_hash", "TEXT")?;
            Self::add_column_if_missing(
                conn,
                "skills",
                "updated_at",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        log::info!("v6 -> v7 迁移完成：已添加 content_hash 和 updated_at 列");
        Ok(())
    }

    /// v7 -> v8: 会话日志使用追踪（无代理模式统计支持）
    fn migrate_v7_to_v8(conn: &Connection) -> Result<(), AppError> {
        // 1. 为 proxy_request_logs 添加 data_source 列，区分数据来源
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "data_source",
                "TEXT NOT NULL DEFAULT 'proxy'",
            )?;
            Self::create_request_logs_usage_indexes_if_supported(conn)?;
        }

        // 2. 创建会话日志同步状态表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_log_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 session_log_sync 表失败: {e}")))?;

        // 3. 修正国产模型定价：之前误将 CNY 值存为 USD 字段，统一转换为 USD
        if Self::table_exists(conn, "model_pricing")? {
            let pricing_fixes: &[(&str, &str, &str, &str, &str)] = &[
                ("deepseek-v3.2", "0.28", "0.42", "0.028", "0"),
                ("deepseek-v3.1", "0.55", "1.67", "0.055", "0"),
                ("deepseek-v3", "0.28", "1.11", "0.028", "0"),
                ("doubao-seed-code", "0.17", "1.11", "0.02", "0"),
                ("kimi-k2-thinking", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-0905", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-turbo", "1.11", "8.06", "0.14", "0"),
                ("minimax-m2.1", "0.27", "0.95", "0.03", "0"),
                ("minimax-m2.1-lightning", "0.27", "2.33", "0.03", "0"),
                ("minimax-m2", "0.27", "0.95", "0.03", "0"),
                ("glm-4.7", "0.39", "1.75", "0.04", "0"),
                ("glm-4.6", "0.28", "1.11", "0.03", "0"),
                ("mimo-v2-flash", "0.09", "0.29", "0.009", "0"),
            ];
            for (model_id, input, output, cache_read, cache_creation) in pricing_fixes {
                conn.execute(
                    "UPDATE model_pricing SET
                        input_cost_per_million = ?2,
                        output_cost_per_million = ?3,
                        cache_read_cost_per_million = ?4,
                        cache_creation_cost_per_million = ?5
                     WHERE model_id = ?1",
                    rusqlite::params![model_id, input, output, cache_read, cache_creation],
                )
                .map_err(|e| AppError::Database(format!("更新模型 {model_id} 定价失败: {e}")))?;
            }
        }

        log::info!("v7 -> v8 迁移完成：data_source 列、session_log_sync 表、修正 13 个模型定价");
        Ok(())
    }

    /// v8 → v9: 全面补充模型定价（清空 + 重新 seed）
    fn migrate_v8_to_v9(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
                model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 model_pricing 表失败: {e}")))?;
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;
        log::info!("v8 -> v9 迁移完成：已刷新全部模型定价数据");
        Ok(())
    }

    /// v9 -> v10 迁移：添加 Hermes Agent 支持
    fn migrate_v9_to_v10(conn: &Connection) -> Result<(), AppError> {
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_hermes",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // skills table may not exist in databases migrated from very old versions
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_hermes",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }

        log::info!("v9 -> v10 迁移完成：已添加 Hermes Agent 支持");
        Ok(())
    }

    /// v10 -> v11：usage_daily_rollups 增加 request_model 维度（进入主键），
    /// proxy_request_logs 增加 pricing_model 列（写入时的计价基准，回填依据）。
    ///
    /// 路由接管下 model（真实上游模型）≠ request_model（客户端别名），
    /// 旧 rollup 只按 model 聚合，明细 prune 后映射关系永久丢失、计费不可审计。
    /// SQLite 改主键必须重建表；历史行的 request_model 已不可知，填 ''。
    fn migrate_v10_to_v11(conn: &Connection) -> Result<(), AppError> {
        // proxy_request_logs.pricing_model：NULL = v11 前的历史行（回填走
        // model → 占位符回退 request_model 的旧逻辑），'' = 未计价的错误行
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "pricing_model", "TEXT")?;
        }

        if !Self::table_exists(conn, "usage_daily_rollups")? {
            log::info!("v10 -> v11：usage_daily_rollups 不存在，跳过重建");
            return Ok(());
        }

        conn.execute_batch(
            "ALTER TABLE usage_daily_rollups RENAME TO usage_daily_rollups_v10;
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
                 total_cost_usd TEXT NOT NULL DEFAULT '0',
                 avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
             );
             INSERT INTO usage_daily_rollups
                 (date, app_type, provider_id, model, request_model, pricing_model,
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms)
             SELECT date, app_type, provider_id, model, '', '',
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
             FROM usage_daily_rollups_v10;
             DROP TABLE usage_daily_rollups_v10;",
        )
        .map_err(|e| {
            AppError::Database(format!("v10 -> v11 重建 usage_daily_rollups 失败: {e}"))
        })?;

        log::info!(
            "v10 -> v11 迁移完成：usage_daily_rollups 已保留 request_model/pricing_model 维度"
        );
        Ok(())
    }

    /// v11 -> v12 迁移：添加项目 Profiles 表
    /// 与 create_tables_on_conn 中的建表语句保持一致（IF NOT EXISTS 保证幂等）
    fn migrate_v11_to_v12(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                sort_order INTEGER,
                created_at INTEGER,
                updated_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("v11 -> v12 创建 profiles 表失败: {e}")))?;
        Ok(())
    }

    /// v12 -> v13：记录 input_tokens 是否包含缓存写入。
    ///
    /// 默认 0 表示旧版/未知语义；旧 Codex 行只包含 cache read，不包含
    /// cache creation。新代理行会显式写入 1(total-inclusive) 或 2(fresh)。
    fn migrate_v12_to_v13(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "usage_daily_rollups")? {
            Self::add_column_if_missing(
                conn,
                "usage_daily_rollups",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v13 -> v14: allow Grok Build to own an independent proxy configuration row.
    fn migrate_v13_to_v14(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_config")? {
            return Ok(());
        }

        conn.execute("DROP TABLE IF EXISTS proxy_config_v14", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE TABLE proxy_config_v14 (
                app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
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
                live_takeover_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        let copied_columns = [
            ("app_type", "'claude'"),
            ("proxy_enabled", "0"),
            ("listen_address", "'127.0.0.1'"),
            ("listen_port", "15721"),
            ("enable_logging", "1"),
            ("enabled", "0"),
            ("auto_failover_enabled", "0"),
            ("max_retries", "3"),
            ("streaming_first_byte_timeout", "60"),
            ("streaming_idle_timeout", "120"),
            ("non_streaming_timeout", "600"),
            ("circuit_failure_threshold", "4"),
            ("circuit_success_threshold", "2"),
            ("circuit_timeout_seconds", "60"),
            ("circuit_error_rate_threshold", "0.6"),
            ("circuit_min_requests", "10"),
            ("default_cost_multiplier", "'1'"),
            ("pricing_model_source", "'response'"),
            ("live_takeover_active", "0"),
            ("created_at", "datetime('now')"),
            ("updated_at", "datetime('now')"),
        ]
        .into_iter()
        .map(|(column, fallback)| {
            Self::has_column(conn, "proxy_config", column).map(|exists| {
                if exists {
                    format!("\"{column}\"")
                } else {
                    fallback.into()
                }
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

        let copy_sql = format!(
            "INSERT INTO proxy_config_v14 (
                app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                enabled, auto_failover_enabled, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests,
                default_cost_multiplier, pricing_model_source, live_takeover_active,
                created_at, updated_at
            )
            SELECT {copied_columns} FROM proxy_config"
        );
        conn.execute(&copy_sql, [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("DROP TABLE proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("ALTER TABLE proxy_config_v14 RENAME TO proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (app_type) VALUES ('grokbuild')",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// v14 -> v15: persist Grok Build enablement for unified Skills and MCP.
    fn migrate_v14_to_v15(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "mcp_servers")? {
            Self::add_column_if_missing(
                conn,
                "mcp_servers",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v15 -> v16: remove Codex session rows and cursors so startup sync can
    /// rebuild them with fork-history alignment. Must stay connection-level:
    /// schema migration already owns the Database connection mutex.
    fn migrate_v15_to_v16(conn: &Connection) -> Result<(), AppError> {
        let codex_dir = crate::codex_config::get_codex_config_dir();
        crate::services::session_usage_codex::reset_codex_usage_on_conn(conn, &codex_dir)
    }

    /// v16 -> v17: preserve session request identities after detail rollup.
    fn migrate_v16_to_v17(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_usage_dedup (
                data_source TEXT NOT NULL,
                request_id TEXT NOT NULL,
                semantic_id TEXT NOT NULL,
                has_entry_id INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (data_source, request_id)
             );
             CREATE INDEX IF NOT EXISTS idx_session_usage_dedup_semantic
             ON session_usage_dedup(data_source, semantic_id, has_entry_id);",
        )
        .map_err(|error| AppError::Database(format!("创建会话用量去重账本失败: {error}")))?;

        Ok(())
    }

    /// v17 -> v18：一次性建立最终 Agent 会话用量 Schema 与 Codex replay 暂存表。
    ///
    /// v17 是本功能发布前的唯一支持基线。后续开发版本曾经使用过
    /// v18-v24 的中间表形态，但这些版本从未发布，因此不在这里保留兼容
    /// 迁移。Schema savepoint 由调用方提供，确保最终对象和 replay 状态
    /// 要么全部创建，要么全部回滚。
    fn migrate_v17_to_v18(conn: &Connection) -> Result<(), AppError> {
        Self::create_agent_session_usage_tables_on_conn(conn)?;
        Self::create_codex_replay_tables_on_conn(conn)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(|error| AppError::Database(format!("补齐 settings 表失败: {error}")))?;

        let has_codex_raw: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM proxy_request_logs
                    WHERE app_type = 'codex' AND data_source = 'codex_session'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        let has_codex_daily: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM usage_daily_rollups
                    WHERE provider_id = '_codex_session'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        let has_codex_cursor: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM session_log_sync
                    WHERE file_path LIKE '%/sessions/%/rollout-%'
                       OR file_path LIKE '%\\sessions\\%\\rollout-%'
                       OR file_path LIKE '%/archived_sessions/rollout-%'
                       OR file_path LIKE '%\\archived_sessions\\rollout-%'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        let state = if has_codex_raw || has_codex_daily || has_codex_cursor {
            "pending"
        } else {
            "complete"
        };
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value)
             VALUES ('codex_usage_canonical_replay_v1', ?1)",
            [state],
        )
        .map_err(|error| AppError::Database(format!("写入 Codex replay 状态失败: {error}")))?;
        Ok(())
    }

    fn canonical_generation_schema(tables: CanonicalGenerationTables) -> String {
        let snapshots = tables
            .snapshots
            .map(|snapshots| {
                format!(
                    "CREATE TABLE IF NOT EXISTS {snapshots} (
                        app_type TEXT NOT NULL,
                        source_identity TEXT NOT NULL,
                        profile_id TEXT NOT NULL DEFAULT '',
                        database_identity TEXT NOT NULL DEFAULT '',
                        session_id TEXT NOT NULL,
                        model TEXT NOT NULL DEFAULT '',
                        provider_id TEXT NOT NULL DEFAULT '',
                        base_url_digest TEXT NOT NULL DEFAULT '',
                        billing_mode TEXT NOT NULL DEFAULT '',
                        task TEXT NOT NULL DEFAULT '',
                        data_source TEXT NOT NULL DEFAULT '',
                        source_version TEXT NOT NULL DEFAULT '',
                        api_call_count INTEGER NOT NULL DEFAULT 0 CHECK (api_call_count >= 0),
                        input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
                        output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
                        cache_read_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_read_tokens >= 0),
                        cache_write_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0),
                        reasoning_tokens INTEGER NOT NULL DEFAULT 0 CHECK (reasoning_tokens >= 0),
                        first_seen INTEGER,
                        last_seen INTEGER,
                        last_synced_at INTEGER NOT NULL,
                        estimated_cost_usd TEXT,
                        actual_cost_usd TEXT,
                        cost_status TEXT,
                        cost_source TEXT,
                        correction_state TEXT,
                        PRIMARY KEY (
                            app_type, source_identity, profile_id, database_identity,
                            session_id, model, provider_id, base_url_digest,
                            billing_mode, task, data_source, source_version
                        )
                    );
                    CREATE INDEX IF NOT EXISTS idx_{snapshots}_lookup
                        ON {snapshots}(
                            app_type, source_identity, profile_id, database_identity, session_id
                        );"
                )
            })
            .unwrap_or_default();

        format!(
            "CREATE TABLE IF NOT EXISTS {nodes} (
                app_type TEXT NOT NULL,
                session_id TEXT NOT NULL,
                parent_session_id TEXT,
                root_session_id TEXT NOT NULL,
                node_kind TEXT NOT NULL,
                relation_confidence TEXT NOT NULL,
                title TEXT,
                project_dir TEXT,
                source_path TEXT,
                created_at INTEGER,
                last_active_at INTEGER,
                last_synced_at INTEGER NOT NULL,
                PRIMARY KEY (app_type, session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_{nodes}_root
                ON {nodes}(app_type, root_session_id);
            CREATE INDEX IF NOT EXISTS idx_{nodes}_parent
                ON {nodes}(app_type, parent_session_id);
            CREATE TABLE IF NOT EXISTS {rollups} (
                date TEXT NOT NULL,
                app_type TEXT NOT NULL,
                session_id TEXT NOT NULL,
                provider_id TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                request_model TEXT NOT NULL DEFAULT '',
                pricing_model TEXT NOT NULL DEFAULT '',
                data_source TEXT NOT NULL DEFAULT '',
                precision TEXT NOT NULL DEFAULT 'request_exact',
                time_semantics TEXT NOT NULL DEFAULT 'event_time',
                request_count_semantics TEXT NOT NULL DEFAULT 'http_request',
                input_token_semantics INTEGER NOT NULL DEFAULT 0,
                source_identity TEXT NOT NULL DEFAULT '',
                profile_id TEXT NOT NULL DEFAULT '',
                database_identity TEXT NOT NULL DEFAULT '',
                base_url_digest TEXT NOT NULL DEFAULT '',
                billing_mode TEXT NOT NULL DEFAULT '',
                task TEXT NOT NULL DEFAULT '',
                source_version TEXT NOT NULL DEFAULT '',
                sync_window_start INTEGER NOT NULL DEFAULT 0,
                sync_window_end INTEGER NOT NULL DEFAULT 0,
                request_count INTEGER,
                api_call_count INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_creation_tokens INTEGER,
                cache_write_tokens INTEGER,
                reasoning_tokens INTEGER,
                total_cost_usd TEXT,
                cost_status TEXT,
                cost_source TEXT,
                cost_delta_kind TEXT,
                correction_state TEXT,
                first_event_at INTEGER,
                last_event_at INTEGER,
                PRIMARY KEY (
                    date, app_type, session_id, provider_id, model,
                    request_model, pricing_model, data_source, precision,
                    time_semantics, request_count_semantics,
                    input_token_semantics, source_identity, profile_id,
                    database_identity, base_url_digest, billing_mode, task,
                    source_version, sync_window_start, sync_window_end
                )
            );
            CREATE INDEX IF NOT EXISTS idx_{rollups}_session
                ON {rollups}(app_type, session_id, date);
            CREATE INDEX IF NOT EXISTS idx_{rollups}_root_lookup
                ON {rollups}(app_type, date, session_id);
            {snapshots}
            CREATE TABLE IF NOT EXISTS {coverage} (
                app_type TEXT NOT NULL,
                data_source TEXT NOT NULL,
                request_id TEXT NOT NULL,
                canonical_session_id TEXT,
                marked_at INTEGER NOT NULL,
                PRIMARY KEY (app_type, data_source, request_id)
            );
            CREATE INDEX IF NOT EXISTS idx_{coverage}_session
                ON {coverage}(app_type, data_source, canonical_session_id);",
            nodes = tables.nodes,
            rollups = tables.rollups,
            snapshots = snapshots,
            coverage = tables.coverage,
        )
    }

    fn create_agent_session_usage_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
        let schema = Self::canonical_generation_schema(PUBLISHED_CANONICAL_GENERATION_TABLES);
        conn.execute_batch(&schema).map_err(|error| {
            AppError::Database(format!(
                "创建 {} Schema 失败: {error}",
                PUBLISHED_CANONICAL_GENERATION_TABLES.label
            ))
        })
    }

    fn create_codex_replay_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
        let mut schema =
            Self::canonical_generation_schema(CODEX_REPLAY_CANONICAL_GENERATION_TABLES);
        schema.push_str(
            "CREATE TABLE IF NOT EXISTS codex_replay_session_logs (
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
                data_source TEXT NOT NULL DEFAULT 'codex_session',
                input_token_semantics INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS codex_replay_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            );",
        );
        conn.execute_batch(&schema).map_err(|error| {
            AppError::Database(format!(
                "创建 {}表失败: {error}",
                CODEX_REPLAY_CANONICAL_GENERATION_TABLES.label
            ))
        })
    }

    /// 插入默认模型定价数据
    /// 格式: (model_id, display_name, input, output, cache_read, cache_creation)
    /// 注意: model_id 使用短横线格式（如 claude-haiku-4-5），与 API 返回的模型名称标准化后一致
    fn seed_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_data = [
            // Claude Fable 5（Opus 之上的新档）
            (
                "claude-fable-5",
                "Claude Fable 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            (
                "claude-mythos-5",
                "Claude Mythos 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            // Claude Opus 5（与 Opus 4.8 同价位；fast mode $10/$50 不入表）
            ("claude-opus-5", "Claude Opus 5", "5", "25", "0.50", "6.25"),
            // Claude 4.8 系列
            (
                "claude-opus-4-8",
                "Claude Opus 4.8",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude Sonnet 5（list 价，与 Sonnet 4.6 一致；促销 $2/$10 至 2026-08-31 不入表）
            (
                "claude-sonnet-5",
                "Claude Sonnet 5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.7 系列
            (
                "claude-opus-4-7",
                "Claude Opus 4.7",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude 4.6 系列（裸 id 行覆盖无日期后缀的日志变体，与 dated 行同价）
            (
                "claude-opus-4-6",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-opus-4-6-20260206",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-6-20260217",
                "Claude Sonnet 4.6",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.5 系列
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
            // Claude 4 系列 (Legacy Models)
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
            // Claude 3.5 系列
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
            // GPT-5.6 系列（Sol / Terra / Luna，2026-06 发布）
            // 5.6 家族起 cache write 收 1.25× 输入价（此前 GPT 模型写缓存免费，勿回填旧系列）
            ("gpt-5.6-sol", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            // 2026-07-30 OpenAI 降价：luna -80%、terra -20%，sol 不变（Fast mode 2× 价不入表）
            ("gpt-5.6-terra", "GPT-5.6 Terra", "2", "12", "0.20", "2.50"),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
            ),
            // 裸名 gpt-5.6 是 sol 的官方别名；effort 后缀对齐 gpt-5.5 系列的记账形态
            ("gpt-5.6", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-low", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-medium", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-high", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-xhigh", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-minimal", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            // GPT-5.5 系列
            ("gpt-5.5", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-low", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-medium", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-high", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-xhigh", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-minimal", "GPT-5.5", "5", "30", "0.50", "0"),
            // GPT-5.4 系列
            ("gpt-5.4", "GPT-5.4", "2.50", "15", "0.25", "0"),
            ("gpt-5.4-mini", "GPT-5.4 Mini", "0.75", "4.50", "0.075", "0"),
            ("gpt-5.4-nano", "GPT-5.4 Nano", "0.20", "1.25", "0.02", "0"),
            // GPT-5.2 系列
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
            // GPT-5.3 Codex 系列
            ("gpt-5.3-codex", "GPT-5.3 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.3-codex-spark",
                "GPT-5.3 Codex Spark",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
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
            // GPT-5.1 系列
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
            // GPT-5 系列
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
            // OpenAI Reasoning 系列
            ("o3", "OpenAI o3", "2", "8", "0.50", "0"),
            ("o4-mini", "OpenAI o4-mini", "1.10", "4.40", "0.275", "0"),
            // GPT-4.1 系列
            ("gpt-4.1", "GPT-4.1", "2", "8", "0.50", "0"),
            ("gpt-4.1-mini", "GPT-4.1 Mini", "0.40", "1.60", "0.10", "0"),
            ("gpt-4.1-nano", "GPT-4.1 Nano", "0.10", "0.40", "0.025", "0"),
            // Gemini 3.7 系列
            // 录的是介绍价（官方公告 + ai.google.dev 价表 + models.dev 三源一致）。
            // ⚠️ 介绍价 2026-12-31 到期，2027-01-01 起恢复 1.50/7.50/0.15（= 3.6 Flash 现价）。
            // 到期后需走 seed + repair 双写改回；届时 models.dev 会先更新，
            // /jason-update-model 审计的 A 段会自动报出这一行作为提醒——
            // 因此这一行刻意不进 audit-ignore.json，勿加豁免（会屏蔽掉该提醒）。
            (
                "gemini-3.7-flash",
                "Gemini 3.7 Flash",
                "0.75",
                "3.75",
                "0.075",
                "0",
            ),
            // Gemini 3.6 系列
            (
                "gemini-3.6-flash",
                "Gemini 3.6 Flash",
                "1.50",
                "7.50",
                "0.15",
                "0",
            ),
            // Gemini 3.5 系列
            (
                "gemini-3.5-flash",
                "Gemini 3.5 Flash",
                "1.50",
                "9.00",
                "0.15",
                "0",
            ),
            (
                "gemini-3.5-flash-lite",
                "Gemini 3.5 Flash Lite",
                "0.30",
                "2.50",
                "0.03",
                "0",
            ),
            // Gemini 3.1 系列
            (
                "gemini-3.1-pro-preview",
                "Gemini 3.1 Pro Preview",
                "2",
                "12",
                "0.20",
                "0",
            ),
            (
                "gemini-3.1-flash-lite",
                "Gemini 3.1 Flash Lite",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            (
                "gemini-3.1-flash-lite-preview",
                "Gemini 3.1 Flash Lite Preview",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            // Gemini 3 系列
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
            // Gemini 2.5 系列
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
                "gemini-2.5-flash-lite",
                "Gemini 2.5 Flash Lite",
                "0.10",
                "0.40",
                "0.01",
                "0",
            ),
            // Gemini 2.0 系列
            (
                "gemini-2.0-flash",
                "Gemini 2.0 Flash",
                "0.10",
                "0.40",
                "0.025",
                "0",
            ),
            // StepFun 系列
            (
                "step-3.7-flash",
                "Step 3.7 Flash",
                "0.19",
                "1.13",
                "0.04",
                "0",
            ),
            (
                "step-3.5-flash",
                "Step 3.5 Flash",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            (
                "step-3.5-flash-2603",
                "Step 3.5 Flash 2603",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            // ====== 国产模型 (USD/1M tokens) ======
            // Doubao (字节跳动)
            // Seed 2.1 系列（2026-06 火山引擎官方 list 价，CNY 按 ~7.14 折算）：
            //   pro   输入 6 元 / 输出 30 元 / 命中 1.2 元
            //   turbo 输入 3 元 / 输出 15 元 / 命中 0.6 元
            // 「缓存存储 0.017 元/M/小时」是按时长计费的存储费，与本表 cache_creation（按 token 写入价）口径不同，置 0。
            (
                "doubao-seed-2-1-pro",
                "Doubao Seed 2.1 Pro",
                "0.84",
                "4.2",
                "0.17",
                "0",
            ),
            (
                "doubao-seed-2-1-turbo",
                "Doubao Seed 2.1 Turbo",
                "0.42",
                "2.1",
                "0.08",
                "0",
            ),
            (
                "doubao-seed-code",
                "Doubao Seed Code",
                "0.17",
                "1.11",
                "0.02",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
            ),
            // DeepSeek 系列
            (
                "deepseek-v3.2",
                "DeepSeek V3.2",
                "0.28",
                "0.42",
                "0.028",
                "0",
            ),
            (
                "deepseek-v3.1",
                "DeepSeek V3.1",
                "0.55",
                "1.67",
                "0.055",
                "0",
            ),
            ("deepseek-v3", "DeepSeek V3", "0.28", "1.11", "0.028", "0"),
            // ── DeepSeek V4 系列：2026-08-16 16:00 UTC 起改为峰谷双档计价 ──
            // 官方价页（api-docs.deepseek.com/quick_start/pricing，中英一致）直接挂 USD，
            // 不再需要 CNY 折算。高峰时段 = 北京时间 9:00-12:00 与 14:00-18:00
            // （= UTC 01:00-04:00、06:00-10:00），共 7h/天；其余 17h 为空闲档。
            //
            // 🔴 本表每模型仅一行、无时段维度，**统一录高峰档**（Jason 2026-08-18 拍板）：
            //   ① 官方措辞是「空闲价为高峰价的一半」，高峰档才是基准挂牌价；
            //   ② 高峰时段正是中文用户的工作时间，是 AI 编程主力时段。
            //   代价=夜间/凌晨用量高估一倍。勿按「阶梯取低档」惯例改成空闲档。
            //
            // input=缓存未命中价，cache_read=缓存命中价；DeepSeek 不单收 cache write → 0。
            // deepseek-chat / deepseek-reasoner 自 2026-07 起为 V4 Flash 的 legacy 别名（同价）
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.44",
                "1.32",
                "0.014",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.44",
                "1.32",
                "0.014",
                "0",
            ),
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.44",
                "1.32",
                "0.014",
                "0",
            ),
            // 部分上游（如阿里百炼）回传 4 位 MMDD 日期变体。查价的
            // strip_model_date_suffix 只剥 ISO / 8 位 YYYYMMDD / 6 位 YYMMDD，
            // 剥不到裸 id，前缀兜底也只匹配更长的行 —— 不补别名会静默按 0 计费
            (
                "deepseek-v4-flash-0731",
                "DeepSeek V4 Flash",
                "0.44",
                "1.32",
                "0.014",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "1.32",
                "3.96",
                "0.044",
                "0",
            ),
            // Kimi (月之暗面)
            (
                "kimi-k2-thinking",
                "Kimi K2 Thinking",
                "0.55",
                "2.20",
                "0.10",
                "0",
            ),
            ("kimi-k2-0905", "Kimi K2", "0.55", "2.20", "0.10", "0"),
            (
                "kimi-k2-turbo",
                "Kimi K2 Turbo",
                "1.11",
                "8.06",
                "0.14",
                "0",
            ),
            ("kimi-k2.5", "Kimi K2.5", "0.60", "3.00", "0.10", "0"),
            ("kimi-k2.6", "Kimi K2.6", "0.95", "4.00", "0.16", "0"),
            (
                "kimi-k2.7-code",
                "Kimi K2.7 Code",
                "0.95",
                "4.00",
                "0.19",
                "0",
            ),
            // HighSpeed 加速档=本体 2 倍价（Kimi 官方一贯模式，同 K2 Turbo）
            (
                "kimi-k2.7-code-highspeed",
                "Kimi K2.7 Code HighSpeed",
                "1.90",
                "8.00",
                "0.38",
                "0",
            ),
            ("kimi-k3", "Kimi K3", "3.00", "15.00", "0.30", "0"),
            // Kimi For Coding 套餐里 K3 的裸名（无 kimi- 前缀），同标准 list 价
            ("k3", "Kimi K3", "3.00", "15.00", "0.30", "0"),
            // 腾讯混元 (Tencent Hunyuan)（官方 CNY 1/4/0.25 按 1 USD ≈ 7.14 折算；Hy3 阶梯计价取最低档）
            ("hunyuan-hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            ("hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            // MiniMax 系列
            ("minimax-m2.1", "MiniMax M2.1", "0.27", "0.95", "0.03", "0"),
            (
                "minimax-m2.1-lightning",
                "MiniMax M2.1 Lightning",
                "0.27",
                "2.33",
                "0.03",
                "0",
            ),
            ("minimax-m2", "MiniMax M2", "0.27", "0.95", "0.03", "0"),
            ("minimax-m2.5", "MiniMax M2.5", "0.15", "0.95", "0.03", "0"),
            (
                "minimax-m2.5-lightning",
                "MiniMax M2.5 Lightning",
                "0.30",
                "2.40",
                "0.03",
                "0",
            ),
            (
                "minimax-m2.7",
                "MiniMax M2.7",
                "0.30",
                "1.20",
                "0.06",
                "0.375",
            ),
            (
                "minimax-m2.7-highspeed",
                "MiniMax M2.7 Highspeed",
                "0.60",
                "2.40",
                "0.06",
                "0.375",
            ),
            ("minimax-m3", "MiniMax M3", "0.30", "1.20", "0.06", "0"),
            // GLM (智谱)
            ("glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0"),
            ("glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0"),
            ("glm-5", "GLM-5", "1", "3.2", "0.2", "0"),
            ("glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0"),
            ("glm-5.2", "GLM-5.2", "1.4", "4.4", "0.26", "0"),
            ("glm-5-turbo", "GLM-5-Turbo", "1.2", "4", "0.24", "0"),
            ("glm-5v-turbo", "GLM-5V-Turbo", "1.2", "4", "0.24", "0"),
            // MiMo (小米)
            (
                "mimo-v2-flash",
                "MiMo V2 Flash",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            ("mimo-v2-pro", "MiMo V2 Pro", "0.435", "0.87", "0.0036", "0"),
            ("mimo-v2.5", "MiMo V2.5", "0.14", "0.29", "0.0028", "0"),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
            ),
            // Qwen 系列 (阿里巴巴)
            ("qwen3.8-max", "Qwen3.8 Max", "2", "6", "0.25", "2.50"),
            ("qwen3.7-max", "Qwen3.7 Max", "2.50", "7.50", "0.25", "0"),
            ("qwen3.7-plus", "Qwen3.7 Plus", "0.40", "1.60", "0.08", "0"),
            (
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                "0.325",
                "1.95",
                "0.065",
                "0",
            ),
            (
                "qwen3.6-flash",
                "Qwen3.6 Flash",
                "0.1875",
                "1.125",
                "0.0375",
                "0",
            ),
            ("qwen3.5-plus", "Qwen3.5 Plus", "0.26", "1.56", "0.052", "0"),
            ("qwen3-max", "Qwen3 Max", "0.78", "3.90", "0", "0"),
            (
                "qwen3-235b-a22b",
                "Qwen3 235B-A22B",
                "0.70",
                "8.40",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
            ),
            (
                "qwen3-coder-480b",
                "Qwen3 Coder 480B",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-480b-a35b-instruct",
                "Qwen3 Coder 480B-A35B Instruct",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
            ),
            (
                "qwen3-coder-next",
                "Qwen3 Coder Next",
                "0.12",
                "0.75",
                "0",
                "0",
            ),
            ("qwq-plus", "QwQ Plus", "0.80", "2.40", "0", "0"),
            ("qwq-32b", "QwQ 32B", "0.20", "0.60", "0", "0"),
            ("qwen3-32b", "Qwen3 32B", "0.16", "0.64", "0", "0"),
            // Grok 系列 (xAI)
            // 4.5/4.6 均为分档计价：prompt ≥200K 时单价翻倍（4/12，cached 亦翻倍）。
            // 本表无档位列，统一取基础档（<200K），与其它分档厂商口径一致
            ("grok-4.6", "Grok 4.6", "2", "6", "0.50", "0"),
            ("grok-4.5", "Grok 4.5", "2", "6", "0.30", "0"),
            // Grok CLI 官方 OAuth 态 modelUsage 上报的内部别名。定价由
            // costUsdTicks（1 tick = 1e-10 USD）双轮实测反推：input/output 与
            // grok-4.5 同为 2/6，cache read 同为 0.30
            ("grok-4.5-build", "Grok 4.5 Build", "2", "6", "0.30", "0"),
            ("grok-4.3", "Grok 4.3", "1.25", "2.50", "0.20", "0"),
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4-1-fast-reasoning",
                "Grok 4.1 Fast Reasoning",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            (
                "grok-4-1-fast-non-reasoning",
                "Grok 4.1 Fast",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            ("grok-4", "Grok 4", "3", "15", "0.75", "0"),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
            ),
            ("grok-build-0.1", "Grok Build 0.1", "1", "2", "0.20", "0"),
            ("grok-3", "Grok 3", "3", "15", "0.75", "0"),
            ("grok-3-mini", "Grok 3 Mini", "0.25", "0.50", "0.075", "0"),
            // Mistral 系列
            (
                "mistral-medium-3.5",
                "Mistral Medium 3.5",
                "1.50",
                "7.50",
                "0",
                "0",
            ),
            (
                "mistral-small-4",
                "Mistral Small 4",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "devstral-small-2-2512",
                "Devstral Small 2",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "magistral-small",
                "Magistral Small",
                "0.50",
                "1.50",
                "0",
                "0",
            ),
            ("codestral-2508", "Codestral", "0.30", "0.90", "0.03", "0"),
            (
                "devstral-small-1.1",
                "Devstral Small 1.1",
                "0.07",
                "0.28",
                "0.01",
                "0",
            ),
            ("devstral-2-2512", "Devstral 2", "0.40", "2", "0.04", "0"),
            (
                "devstral-medium",
                "Devstral Medium",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-large-3-2512",
                "Mistral Large 3",
                "0.50",
                "1.50",
                "0.05",
                "0",
            ),
            (
                "mistral-medium-3.1",
                "Mistral Medium 3.1",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-small-3.2-24b",
                "Mistral Small 3.2",
                "0.075",
                "0.20",
                "0.01",
                "0",
            ),
            ("magistral-medium", "Magistral Medium", "2", "5", "0", "0"),
            // Cohere 系列
            ("command-a", "Cohere Command A", "2.50", "10", "0", "0"),
            (
                "command-r-plus",
                "Cohere Command R+",
                "2.50",
                "10",
                "0",
                "0",
            ),
            ("command-r", "Cohere Command R", "0.15", "0.60", "0", "0"),
            // OpenAI 补充
            ("o3-pro", "OpenAI o3-pro", "20", "80", "0", "0"),
            ("o3-mini", "OpenAI o3-mini", "0.55", "2.20", "0.55", "0"),
            ("o1", "OpenAI o1", "15", "60", "7.50", "0"),
            ("o1-mini", "OpenAI o1-mini", "0.55", "2.20", "0.55", "0"),
            ("codex-mini", "Codex Mini", "0.75", "3", "0.025", "0"),
            ("gpt-5-mini", "GPT-5 Mini", "0.25", "2", "0.025", "0"),
            ("gpt-5-nano", "GPT-5 Nano", "0.05", "0.40", "0.005", "0"),
        ];

        let mut stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;
        for (model_id, display_name, input, output, cache_read, cache_creation) in pricing_data {
            stmt.execute(rusqlite::params![
                model_id,
                display_name,
                input,
                output,
                cache_read,
                cache_creation
            ])
            .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        }

        log::info!("已插入 {} 条默认模型定价数据", pricing_data.len());
        Ok(())
    }

    fn repair_current_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_fixes = [
            // 2026-08-13 models.dev 审计核价：grok-4.5 的 cached input 官方挂牌为 0.30
            // （docs.x.ai 现行价表），与 grok-4.5-build 的实测计费一致；早先按 0.50
            // 录入的行在此校正。注意 0.50 是 grok-4.6 的 cached 价，勿两者互串
            (
                "grok-4.5", "Grok 4.5", "2", "6", "0.30", "0", "2", "6", "0.50", "0",
            ),
            // 2026-07-30 OpenAI GPT-5.6 降价：luna -80%、terra -20%（sol 不变）。
            // 每档两条守卫：主守卫匹配 ≥v3.19（已跑过 07-12 cache_write 修正），
            // 0 态守卫匹配 <v3.19 直升用户（cache_write 仍为旧 seed 的 0）
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
                "1",
                "6",
                "0.10",
                "1.25",
            ),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
                "1",
                "6",
                "0.10",
                "0",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2",
                "12",
                "0.20",
                "2.50",
                "2.50",
                "15",
                "0.25",
                "3.125",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2",
                "12",
                "0.20",
                "2.50",
                "2.50",
                "15",
                "0.25",
                "0",
            ),
            // 2026-07-31 models.dev 审计核价：DeepSeek V4 发布后 chat/reasoner 降为 V4 Flash
            // 别名价；MiniMax M3 官方 standard 档 0.3/1.2（旧值疑似录了加速档）
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.27",
                "1.10",
                "0.07",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.55",
                "2.19",
                "0.14",
                "0",
            ),
            (
                "minimax-m3",
                "MiniMax M3",
                "0.30",
                "1.20",
                "0.06",
                "0",
                "0.60",
                "2.40",
                "0.12",
                "0",
            ),
            // 2026-07-12 GPT-5.6 家族 cache write=1.25× 输入价（OpenAI 5.6 起的新规），
            // 修正早期 seed 的 0 值；只匹配未被用户改过的行
            (
                "gpt-5.6-sol",
                "GPT-5.6 Sol",
                "5",
                "30",
                "0.50",
                "6.25",
                "5",
                "30",
                "0.50",
                "0",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2.50",
                "15",
                "0.25",
                "3.125",
                "2.50",
                "15",
                "0.25",
                "0",
            ),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "1",
                "6",
                "0.10",
                "1.25",
                "1",
                "6",
                "0.10",
                "0",
            ),
            // 2026-06-10 全量核价（厂商官方 list 价；CNY 按 ~7.14 折算）
            // GLM 4.6/4.7：旧值是中转/OpenRouter 折扣价，统一到 Z.ai 官方（与 glm-5/5.1 一致）
            (
                "glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0", "0.39", "1.75", "0.04", "0",
            ),
            (
                "glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0", "0.28", "1.11", "0.03", "0",
            ),
            // Grok 4.20：xAI 已降价 2/6 → 1.25/2.50
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            // Kimi K2.5 官方 output 3.00
            (
                "kimi-k2.5",
                "Kimi K2.5",
                "0.60",
                "3.00",
                "0.10",
                "0",
                "0.60",
                "2.50",
                "0.10",
                "0",
            ),
            // MiniMax M2.5 input 0.15
            (
                "minimax-m2.5",
                "MiniMax M2.5",
                "0.15",
                "0.95",
                "0.03",
                "0",
                "0.12",
                "0.95",
                "0.03",
                "0",
            ),
            // Mistral Devstral 2 output 0.90 → 2（与同表 devstral-medium 一致）
            (
                "devstral-2-2512",
                "Devstral 2",
                "0.40",
                "2",
                "0.04",
                "0",
                "0.40",
                "0.90",
                "0.04",
                "0",
            ),
            // Doubao Seed 2.0：lite 旧价贵 3-4 倍 + 全系补 cache 命中价
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
                "0.25",
                "2",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
                "0.03",
                "0.31",
                "0",
                "0",
            ),
            // MiMo：5/27 永久降价，旧值是旧价
            (
                "mimo-v2-pro",
                "MiMo V2 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            (
                "mimo-v2.5",
                "MiMo V2.5",
                "0.14",
                "0.29",
                "0.0028",
                "0",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            // Qwen：官方"隐式缓存 = 输入 20%"补 cache 命中价
            (
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                "0.325",
                "1.95",
                "0.065",
                "0",
                "0.325",
                "1.95",
                "0",
                "0",
            ),
            (
                "qwen3.5-plus",
                "Qwen3.5 Plus",
                "0.26",
                "1.56",
                "0.052",
                "0",
                "0.26",
                "1.56",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
                "0.195",
                "0.975",
                "0",
                "0",
            ),
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.14",
                "0.28",
                "0.028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.435",
                "0.87",
                "0.003625",
                "0",
                "1.68",
                "3.36",
                "0.14",
                "0",
            ),
            (
                "glm-5", "GLM-5", "1", "3.2", "0.2", "0", "0.72", "2.30", "0", "0",
            ),
            (
                "glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0", "0.95", "3.15", "0", "0",
            ),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
                "0.20",
                "1.50",
                "0.02",
                "0",
            ),
            // 2026-08-16 16:00 UTC DeepSeek V4 全系改峰谷双档计价（本表统一录高峰档，
            // 理由见 seed_model_pricing 里 DeepSeek V4 段的注释）。涨幅很大：
            // flash 0.14/0.28/0.0028 → 0.44/1.32/0.014；pro 0.435/0.87/0.003625 → 1.32/3.96/0.044。
            //
            // 🔴 这五条必须留在数组末尾：上面 2026-07-31 的 chat/reasoner 条目与
            // 2026-07 的 v4-flash(cache_read 0.028→0.0028) / v4-pro(1.68/3.36→0.435/0.87)
            // 条目会先把各种历史形态收敛到同一个旧值，这里才能单守卫命中。
            // 若把本组挪到它们之前，老库会停在中间价位不再前进。
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.44",
                "1.32",
                "0.014",
                "0",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.44",
                "1.32",
                "0.014",
                "0",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.44",
                "1.32",
                "0.014",
                "0",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-v4-flash-0731",
                "DeepSeek V4 Flash",
                "0.44",
                "1.32",
                "0.014",
                "0",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "1.32",
                "3.96",
                "0.044",
                "0",
                "0.435",
                "0.87",
                "0.003625",
                "0",
            ),
        ];

        for (
            model_id,
            display_name,
            input,
            output,
            cache_read,
            cache_creation,
            old_input,
            old_output,
            old_cache_read,
            old_cache_creation,
        ) in pricing_fixes
        {
            conn.execute(
                "UPDATE model_pricing SET
                    display_name = ?2,
                    input_cost_per_million = ?3,
                    output_cost_per_million = ?4,
                    cache_read_cost_per_million = ?5,
                    cache_creation_cost_per_million = ?6
                 WHERE model_id = ?1
                   AND input_cost_per_million = ?7
                   AND output_cost_per_million = ?8
                   AND cache_read_cost_per_million = ?9
                   AND cache_creation_cost_per_million = ?10",
                rusqlite::params![
                    model_id,
                    display_name,
                    input,
                    output,
                    cache_read,
                    cache_creation,
                    old_input,
                    old_output,
                    old_cache_read,
                    old_cache_creation
                ],
            )
            .map_err(|e| AppError::Database(format!("修复模型 {model_id} 定价失败: {e}")))?;
        }

        Ok(())
    }

    /// 确保模型定价表具备默认数据
    pub fn ensure_model_pricing_seeded(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::ensure_model_pricing_seeded_on_conn(&conn)
    }

    pub(crate) fn ensure_model_pricing_seeded_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 每次启动都执行 INSERT OR IGNORE，增量追加新模型；仅修复仍等于旧内置值的定价。
        Self::seed_model_pricing(conn)?;
        Self::repair_current_model_pricing(conn)
    }

    // --- 辅助方法 ---

    pub(crate) fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 user_version 失败: {e}")))
    }

    pub(crate) fn set_user_version(conn: &Connection, version: i32) -> Result<(), AppError> {
        if version < 0 {
            return Err(AppError::Database("user_version 不能为负数".to_string()));
        }
        let sql = format!("PRAGMA user_version = {version};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("写入 user_version 失败: {e}")))?;
        Ok(())
    }

    fn create_request_logs_usage_indexes_if_supported(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(());
        }

        let has_app_type = Self::has_column(conn, "proxy_request_logs", "app_type")?;
        let has_created_at = Self::has_column(conn, "proxy_request_logs", "created_at")?;
        if has_app_type && has_created_at {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_app_created_at
                 ON proxy_request_logs(app_type, created_at DESC)",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建使用量应用时间索引失败: {e}")))?;
        }

        let required_columns = [
            "app_type",
            "data_source",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "created_at",
            "cache_creation_tokens",
        ];
        for column in required_columns {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(());
            }
        }

        conn.execute("DROP INDEX IF EXISTS idx_request_logs_dedup_lookup", [])
            .map_err(|e| AppError::Database(format!("删除旧使用量去重索引失败: {e}")))?;

        // 查询层为了兼容历史 NULL data_source 行，会使用
        // COALESCE(data_source, 'proxy')。普通 data_source 索引无法匹配该表达式，
        // 会让跨源去重子查询退化成大量扫描；表达式索引让 SQLite 能按同一表达式查找。
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_dedup_lookup_expr
             ON proxy_request_logs(app_type, COALESCE(data_source, 'proxy'), input_tokens,
                                   output_tokens, cache_read_tokens, created_at,
                                   cache_creation_tokens)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建使用量去重表达式索引失败: {e}")))?;
        Ok(())
    }

    fn validate_identifier(s: &str, kind: &str) -> Result<(), AppError> {
        if s.is_empty() {
            return Err(AppError::Database(format!("{kind} 不能为空")));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AppError::Database(format!(
                "非法{kind}: {s}，仅允许字母、数字和下划线"
            )));
        }
        Ok(())
    }

    pub(crate) fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|e| AppError::Database(format!("读取表名失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表名失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(0)
                .map_err(|e| AppError::Database(format!("解析表名失败: {e}")))?;
            if name.eq_ignore_ascii_case(table) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        let sql = format!("PRAGMA table_info(\"{table}\");");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("读取表结构失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表结构失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(1)
                .map_err(|e| AppError::Database(format!("读取列名失败: {e}")))?;
            if name.eq_ignore_ascii_case(column) {
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
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        if !Self::table_exists(conn, table)? {
            return Err(AppError::Database(format!(
                "表 {table} 不存在，无法添加列 {column}"
            )));
        }
        if Self::has_column(conn, table, column)? {
            return Ok(false);
        }

        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("为表 {table} 添加列 {column} 失败: {e}")))?;
        log::info!("已为表 {table} 添加缺失列 {column}");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct TestHomeGuard {
        previous_test_home: Option<std::ffi::OsString>,
        _temp_dir: tempfile::TempDir,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("create isolated test home");
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp_dir.path());
            Self {
                previous_test_home,
                _temp_dir: temp_dir,
            }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match self.previous_test_home.as_ref() {
                Some(previous) => std::env::set_var("CC_SWITCH_TEST_HOME", previous),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    const V18_SESSION_USAGE_OBJECTS: &str = "DROP INDEX IF EXISTS idx_agent_session_nodes_root;
         DROP INDEX IF EXISTS idx_agent_session_nodes_parent;
         DROP TABLE IF EXISTS agent_session_nodes;
         DROP INDEX IF EXISTS idx_agent_session_usage_rollups_session;
         DROP INDEX IF EXISTS idx_agent_session_usage_rollups_root_lookup;
         DROP TABLE IF EXISTS agent_session_usage_rollups;
         DROP INDEX IF EXISTS idx_agent_session_usage_snapshots_lookup;
         DROP TABLE IF EXISTS agent_session_usage_snapshots;
         DROP INDEX IF EXISTS idx_agent_session_canonical_coverage_session;
         DROP TABLE IF EXISTS agent_session_canonical_coverage;
         DROP INDEX IF EXISTS idx_codex_replay_nodes_root;
         DROP INDEX IF EXISTS idx_codex_replay_nodes_parent;
         DROP TABLE IF EXISTS codex_replay_nodes;
         DROP INDEX IF EXISTS idx_codex_replay_rollups_session;
         DROP INDEX IF EXISTS idx_codex_replay_rollups_root_lookup;
         DROP TABLE IF EXISTS codex_replay_rollups;
         DROP INDEX IF EXISTS idx_codex_replay_coverage_session;
         DROP TABLE IF EXISTS codex_replay_coverage;
         DROP TABLE IF EXISTS codex_replay_session_logs;
         DROP TABLE IF EXISTS codex_replay_sync;";

    fn drop_v18_session_usage_objects(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(V18_SESSION_USAGE_OBJECTS)?;
        Ok(())
    }

    fn assert_v18_session_usage_tables(conn: &Connection) -> Result<(), AppError> {
        for table in [
            "agent_session_nodes",
            "agent_session_usage_rollups",
            "agent_session_usage_snapshots",
            "agent_session_canonical_coverage",
            "codex_replay_nodes",
            "codex_replay_rollups",
            "codex_replay_coverage",
            "codex_replay_session_logs",
            "codex_replay_sync",
        ] {
            assert!(Database::table_exists(conn, table)?, "missing {table}");
        }
        Ok(())
    }

    #[derive(Clone, Copy)]
    enum CodexReplayEvidence {
        Fresh,
        ProxyOnly,
        NativeRaw,
        DailyRollup,
        Cursor,
    }

    fn migrate_v17_replay_state(evidence: CodexReplayEvidence) -> Result<String, AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn, true)?;
        match evidence {
            CodexReplayEvidence::Fresh => {}
            CodexReplayEvidence::ProxyOnly => {
                conn.execute(
                    "INSERT INTO proxy_request_logs
                        (request_id, provider_id, app_type, model, latency_ms,
                         status_code, created_at, data_source)
                     VALUES ('proxy-only', 'openai', 'codex', 'gpt-5.6-sol', 0, 200, 1, 'proxy')",
                    [],
                )?;
            }
            CodexReplayEvidence::NativeRaw => {
                conn.execute(
                    "INSERT INTO proxy_request_logs
                        (request_id, provider_id, app_type, model, latency_ms,
                         status_code, created_at, data_source)
                     VALUES ('codex-native', '_codex_session', 'codex', 'gpt-5.6-sol',
                             0, 200, 1, 'codex_session')",
                    [],
                )?;
            }
            CodexReplayEvidence::DailyRollup => {
                conn.execute(
                    "INSERT INTO usage_daily_rollups
                        (date, app_type, provider_id, model, request_count)
                     VALUES ('2026-08-01', 'codex', '_codex_session', 'gpt-5.6-sol', 1)",
                    [],
                )?;
            }
            CodexReplayEvidence::Cursor => {
                conn.execute(
                    "INSERT INTO session_log_sync
                        (file_path, last_modified, last_line_offset, last_synced_at)
                     VALUES (?1, 1, 1, 1)",
                    [r"C:\Users\admin\.codex\sessions\2026\08\rollout-cursor.jsonl"],
                )?;
            }
        }
        Database::set_user_version(&conn, 17)?;
        Database::apply_schema_migrations_on_conn(&conn)?;
        conn.query_row(
            "SELECT value FROM settings WHERE key = 'codex_usage_canonical_replay_v1'",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::from)
    }

    #[test]
    fn migrate_v12_to_v13_adds_input_token_semantics_columns() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE usage_daily_rollups (date TEXT PRIMARY KEY)",
            [],
        )?;
        Database::set_user_version(&conn, 12)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::has_column(
            &conn,
            "proxy_request_logs",
            "input_token_semantics"
        )?);
        assert!(Database::has_column(
            &conn,
            "usage_daily_rollups",
            "input_token_semantics"
        )?);
        let log_default: i64 = conn.query_row(
            "SELECT dflt_value = '0' FROM pragma_table_info('proxy_request_logs')
             WHERE name = 'input_token_semantics'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(log_default, 1);

        Ok(())
    }

    #[test]
    fn migrate_v13_to_v14_adds_grokbuild_proxy_row_and_preserves_values() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn, true)?;
        conn.execute("DELETE FROM proxy_config WHERE app_type = 'grokbuild'", [])?;
        conn.execute(
            "UPDATE proxy_config SET enabled = 1, max_retries = 9 WHERE app_type = 'codex'",
            [],
        )?;
        Database::set_user_version(&conn, 13)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        let grok_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'grokbuild'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(grok_rows, 1);
        let codex_values: (i64, i64) = conn.query_row(
            "SELECT enabled, max_retries FROM proxy_config WHERE app_type = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(codex_values, (1, 9));

        Ok(())
    }

    #[test]
    fn migrate_v14_to_v15_adds_grokbuild_skill_and_mcp_flags() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE mcp_servers (
                id TEXT PRIMARY KEY,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0
            );
            CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0
            );",
        )?;
        conn.execute(
            "INSERT INTO mcp_servers (id, enabled_codex) VALUES ('mcp-1', 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO skills (id, enabled_codex) VALUES ('skill-1', 1)",
            [],
        )?;
        Database::set_user_version(&conn, 14)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::has_column(
            &conn,
            "mcp_servers",
            "enabled_grokbuild"
        )?);
        assert!(Database::has_column(&conn, "skills", "enabled_grokbuild")?);
        let mcp_values: (i64, i64) = conn.query_row(
            "SELECT enabled_codex, enabled_grokbuild FROM mcp_servers WHERE id = 'mcp-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let skill_values: (i64, i64) = conn.query_row(
            "SELECT enabled_codex, enabled_grokbuild FROM skills WHERE id = 'skill-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(mcp_values, (1, 0));
        assert_eq!(skill_values, (1, 0));

        Ok(())
    }

    #[test]
    fn migrate_v15_to_v16_resets_only_codex_session_usage() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn, true)?;
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
                ('2026-07-10', 'gemini', '_gemini_session', 'gemini');
             INSERT INTO session_log_sync
                (file_path, last_modified, last_line_offset, last_synced_at)
             VALUES
                ('/old/sessions/rollout-old-00000000-0000-4000-8000-000000000001.jsonl', 1, 1, 1),
                ('/gemini/tmp/session-123.json', 1, 1, 1);",
        )?;
        Database::set_user_version(&conn, 15)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::table_exists(&conn, "session_usage_dedup")?);
        let counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'gemini_session'),
                (SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = '_codex_session'),
                (SELECT COUNT(*) FROM session_log_sync)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(counts, (0, 1, 0, 1));
        assert_v18_session_usage_tables(&conn)?;
        Ok(())
    }

    #[test]
    #[serial]
    fn init_legacy_v0_keeps_v18_objects_inside_migration_savepoint() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db_dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&db_dir).map_err(|e| AppError::io(&db_dir, e))?;
        let db_path = db_dir.join("cc-switch.db");
        {
            let conn = Connection::open(&db_path)?;
            conn.execute("CREATE TABLE legacy_marker (value TEXT NOT NULL)", [])?;
            conn.execute("INSERT INTO legacy_marker VALUES ('preserved')", [])?;
            // Fail late in v17 -> v18 after the earlier migration steps have
            // run. The conflicting object itself belongs to the legacy DB and
            // must survive the failed migration.
            conn.execute(
                "CREATE TABLE idx_agent_session_nodes_root (marker INTEGER NOT NULL)",
                [],
            )?;
        }

        // An existing database may still have SQLite's default user_version.
        // Startup must not treat it as a fresh database and create v18 objects
        // before the migration savepoint.
        let error = match Database::init() {
            Ok(_) => panic!("conflicting index name should fail v18 migration"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("idx_agent_session_nodes_root")
                || error.to_string().contains("already exists"),
            "unexpected migration error: {error}"
        );
        let conn = Connection::open(&db_path)?;
        assert_eq!(Database::get_user_version(&conn)?, 0);
        assert!(!Database::table_exists(&conn, "agent_session_nodes")?);
        assert!(!Database::table_exists(
            &conn,
            "agent_session_usage_rollups"
        )?);
        assert!(!Database::table_exists(&conn, "codex_replay_sync")?);
        let marker: String =
            conn.query_row("SELECT value FROM legacy_marker", [], |row| row.get(0))?;
        assert_eq!(marker, "preserved");
        Ok(())
    }

    #[test]
    fn migrate_v17_to_v18_creates_session_tables_without_touching_existing_rows(
    ) -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn, true)?;
        // Simulate an actual v17 database: create_tables_on_conn is also used
        // on current databases, so remove the v18 objects before setting the
        // legacy user_version and exercising the migration itself.
        drop_v18_session_usage_objects(&conn)?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES ('p1', 'claude', 'Provider', '{}', '{}')",
            [],
        )?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens,
                output_tokens, latency_ms, status_code, created_at, session_id
             ) VALUES ('raw-1', 'p1', 'claude', 'claude-3', 10, 5, 1, 200, 1, 'root-1')",
            [],
        )?;
        conn.execute(
            "INSERT INTO usage_daily_rollups
                (date, app_type, provider_id, model, request_count, input_tokens)
             VALUES ('2026-01-01', 'claude', 'p1', 'claude-3', 1, 10)",
            [],
        )?;
        Database::set_user_version(&conn, 17)?;

        // Startup calls create_tables before applying migrations. On a real
        // v17 database that call must not materialize any v18 object early.
        Database::create_tables_on_conn(&conn, false)?;
        assert!(!Database::table_exists(&conn, "agent_session_nodes")?);
        assert!(!Database::table_exists(&conn, "codex_replay_sync")?);

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert_v18_session_usage_tables(&conn)?;
        let counts: (i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM providers),
                (SELECT COUNT(*) FROM proxy_request_logs),
                (SELECT COUNT(*) FROM usage_daily_rollups)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(counts, (1, 1, 1));
        Ok(())
    }

    #[test]
    fn migrate_v17_to_v18_rolls_back_all_new_objects_on_failure() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn, true)?;
        drop_v18_session_usage_objects(&conn)?;
        conn.execute(
            "CREATE TABLE idx_agent_session_nodes_root (marker INTEGER NOT NULL)",
            [],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES ('p1', 'claude', 'Provider', '{}', '{}')",
            [],
        )?;
        Database::set_user_version(&conn, 17)?;

        Database::create_tables_on_conn(&conn, false)?;
        assert!(!Database::table_exists(&conn, "agent_session_nodes")?);
        assert!(!Database::table_exists(&conn, "codex_replay_sync")?);

        let error = Database::apply_schema_migrations_on_conn(&conn)
            .expect_err("conflicting index name should fail v18 migration");
        assert!(
            error.to_string().contains("idx_agent_session_nodes_root")
                || error.to_string().contains("already exists"),
            "unexpected migration error: {error}"
        );
        assert_eq!(Database::get_user_version(&conn)?, 17);
        assert!(!Database::table_exists(&conn, "agent_session_nodes")?);
        assert!(!Database::table_exists(
            &conn,
            "agent_session_usage_rollups"
        )?);
        assert!(!Database::table_exists(
            &conn,
            "agent_session_usage_snapshots"
        )?);
        assert!(Database::table_exists(
            &conn,
            "idx_agent_session_nodes_root"
        )?);
        let provider_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM providers WHERE id = 'p1' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(provider_count, 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn init_backups_v17_before_creating_v18_objects() -> Result<(), AppError> {
        let _test_home = TestHomeGuard::new();
        let db_dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&db_dir).map_err(|e| AppError::io(&db_dir, e))?;
        let db_path = db_dir.join("cc-switch.db");
        {
            let conn = Connection::open(&db_path)?;
            Database::create_tables_on_conn(&conn, true)?;
            drop_v18_session_usage_objects(&conn)?;
            Database::set_user_version(&conn, 17)?;
        }

        let db = Database::init()?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            assert_eq!(Database::get_user_version(&conn)?, 18);
            assert!(Database::table_exists(&conn, "agent_session_nodes")?);
        }
        drop(db);

        let backup_dir = db_dir.join("backups");
        let mut found_v17_backup = false;
        for entry in std::fs::read_dir(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))? {
            let path = entry.map_err(|e| AppError::io(&backup_dir, e))?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("db") {
                continue;
            }
            let conn = Connection::open(&path)?;
            if Database::get_user_version(&conn)? == 17
                && !Database::table_exists(&conn, "agent_session_nodes")?
                && !Database::table_exists(&conn, "codex_replay_sync")?
            {
                found_v17_backup = true;
                break;
            }
        }
        assert!(
            found_v17_backup,
            "startup should retain a pre-migration v17 backup without v18 objects"
        );
        Ok(())
    }

    #[test]
    fn migrate_v17_to_v18_marks_replay_only_for_native_codex_history() -> Result<(), AppError> {
        for (label, evidence, expected) in [
            ("fresh", CodexReplayEvidence::Fresh, "complete"),
            ("proxy-only", CodexReplayEvidence::ProxyOnly, "complete"),
            ("native raw", CodexReplayEvidence::NativeRaw, "pending"),
            ("daily rollup", CodexReplayEvidence::DailyRollup, "pending"),
            ("cursor", CodexReplayEvidence::Cursor, "pending"),
        ] {
            assert_eq!(
                migrate_v17_replay_state(evidence)?,
                expected,
                "unexpected replay state for {label} evidence"
            );
        }
        Ok(())
    }

    #[test]
    fn migrate_v16_to_v17_creates_session_usage_dedup_ledger() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::set_user_version(&conn, 16)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::table_exists(&conn, "session_usage_dedup")?);
        conn.execute(
            "INSERT INTO session_usage_dedup
             (data_source, request_id, semantic_id, has_entry_id)
             VALUES ('pi_session', 'request', 'semantic', 1)",
            [],
        )?;

        Ok(())
    }
}
