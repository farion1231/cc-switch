//! 数据库模块 - SQLite 数据持久化
//!
//! 此模块提供应用的核心数据存储功能，包括：
//! - 供应商配置管理
//! - MCP 服务器配置
//! - 提示词管理
//! - Skills 管理
//! - 通用设置存储
//!
//! ## 架构设计
//!
//! ```text
//! database/
//! ├── mod.rs        - Database 结构体 + 初始化
//! ├── schema.rs     - 表结构定义 + Schema 迁移
//! ├── backup.rs     - SQL 导入导出 + 快照备份
//! ├── migration.rs  - JSON → SQLite 数据迁移
//! └── dao/          - 数据访问对象
//!     ├── providers.rs
//!     ├── mcp.rs
//!     ├── prompts.rs
//!     ├── skills.rs
//!     └── settings.rs
//! ```

pub(crate) mod backup;
mod dao;
mod migration;
mod schema;

#[cfg(test)]
mod tests;

// DAO 类型导出供外部使用
pub use dao::FailoverQueueItem;

use crate::config::get_app_config_dir;
use crate::error::AppError;
use rusqlite::{hooks::Action, Connection};
use serde::Serialize;
use std::sync::Mutex;

// DAO 方法通过 impl Database 提供，无需额外导出

/// 当前 Schema 版本号
/// 每次修改表结构时递增，并在 schema.rs 中添加相应的迁移逻辑
pub(crate) const SCHEMA_VERSION: i32 = 6;

/// 安全地序列化 JSON，避免 unwrap panic
pub(crate) fn to_json_string<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))
}

/// 安全地获取 Mutex 锁，避免 unwrap panic
macro_rules! lock_conn {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {}", e)))?
    };
}

// 导出宏供子模块使用
pub(crate) use lock_conn;

/// 数据库连接封装
///
/// 使用 Mutex 包装 Connection 以支持在多线程环境（如 Tauri State）中共享。
/// rusqlite::Connection 本身不是 Sync 的，因此需要这层包装。
pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

fn register_db_change_hook(conn: &Connection) {
    conn.update_hook(Some(
        |action: Action, _database: &str, table: &str, _row_id: i64| match action {
            Action::SQLITE_INSERT | Action::SQLITE_UPDATE | Action::SQLITE_DELETE => {
                crate::services::webdav_auto_sync::notify_db_changed(table);
            }
            _ => {}
        },
    ));
}

impl Database {
    /// 初始化数据库连接并创建表
    ///
    /// 数据库文件位于 `~/.cc-switch/cc-switch.db`
    pub fn init() -> Result<Self, AppError> {
        let db_path = get_app_config_dir().join("cc-switch.db");
        let db_exists = db_path.exists();

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let conn = Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if !db_exists {
            // For a brand-new database, configure incremental auto-vacuum
            // before creating any tables so no rebuild is needed later.
            conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        register_db_change_hook(&conn);

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;

        // Pre-migration backup: only when upgrading from an existing database
        {
            let conn = lock_conn!(db.conn);
            let version = Self::get_user_version(&conn)?;
            drop(conn);
            if version > 0 && version < SCHEMA_VERSION {
                log::info!(
                    "Creating pre-migration database backup (v{version} → v{SCHEMA_VERSION})"
                );
                if let Err(e) = db.backup_database_file() {
                    log::warn!("Pre-migration backup failed, continuing migration: {e}");
                }
            }
        }

        db.apply_schema_migrations()?;
        if let Err(e) = db.ensure_incremental_auto_vacuum() {
            log::warn!("Failed to ensure incremental auto-vacuum: {e}");
        }
        db.ensure_model_pricing_seeded()?;

        // Seed default providers for new database
        if !db_exists {
            if let Err(e) = db.seed_default_providers() {
                log::warn!("Failed to seed default providers: {e}");
            }
        }

        // Startup cleanup: prune old logs and reclaim space
        if let Err(e) = db.cleanup_old_stream_check_logs(7) {
            log::warn!("Startup stream_check_logs cleanup failed: {e}");
        }
        if let Err(e) = db.rollup_and_prune(30) {
            log::warn!("Startup rollup_and_prune failed: {e}");
        }
        // Reclaim disk space after cleanup
        {
            let conn = lock_conn!(db.conn);
            if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum;") {
                log::warn!("Startup incremental vacuum failed: {e}");
            }
        }

        Ok(db)
    }

    /// 创建内存数据库（用于测试）
    pub fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        register_db_change_hook(&conn);

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;
        db.ensure_model_pricing_seeded()?;

        Ok(db)
    }

    pub(crate) fn get_auto_vacuum_mode(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA auto_vacuum;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 auto_vacuum 失败: {e}")))
    }

    fn has_user_tables(conn: &Connection) -> Result<bool, AppError> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(format!("读取表数量失败: {e}")))?;
        Ok(count > 0)
    }

    pub(crate) fn ensure_incremental_auto_vacuum_on_conn(
        conn: &Connection,
    ) -> Result<bool, AppError> {
        let mode = Self::get_auto_vacuum_mode(conn)?;
        if mode == 2 {
            return Ok(false);
        }

        let has_tables = Self::has_user_tables(conn)?;
        conn.execute("PRAGMA auto_vacuum = INCREMENTAL;", [])
            .map_err(|e| AppError::Database(format!("设置 auto_vacuum 失败: {e}")))?;

        if !has_tables {
            return Ok(false);
        }

        conn.execute("VACUUM;", [])
            .map_err(|e| AppError::Database(format!("执行 VACUUM 失败: {e}")))?;
        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(format!("恢复 foreign_keys 失败: {e}")))?;
        Ok(true)
    }

    pub(crate) fn ensure_incremental_auto_vacuum(&self) -> Result<bool, AppError> {
        let mode = {
            let conn = lock_conn!(self.conn);
            Self::get_auto_vacuum_mode(&conn)?
        };
        if mode == 2 {
            return Ok(false);
        }

        let has_tables = {
            let conn = lock_conn!(self.conn);
            Self::has_user_tables(&conn)?
        };
        if has_tables {
            log::info!(
                "Detected auto_vacuum={mode}, rebuilding database to enable incremental vacuum"
            );
            self.backup_database_file()?;
        }

        let rebuilt = {
            let conn = lock_conn!(self.conn);
            Self::ensure_incremental_auto_vacuum_on_conn(&conn)?
        };

        if rebuilt {
            log::info!("Incremental auto-vacuum enabled after database rebuild");
        } else {
            log::info!("Incremental auto-vacuum configured for new database");
        }

        Ok(rebuilt)
    }

    /// 检查 MCP 服务器表是否为空
    pub fn is_mcp_table_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }

    /// 检查提示词表是否为空
    pub fn is_prompts_table_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }

    /// 种子默认供应商（仅在新数据库创建时调用）
    ///
    /// 添加3个示例供应商用于测试拖拽排序功能
    pub fn seed_default_providers(&self) -> Result<(), AppError> {
        use crate::provider::{Provider, ProviderMeta};
        use rusqlite::params;

        let providers = vec![
            Provider {
                id: "openai-demo".to_string(),
                name: "OpenAI".to_string(),
                settings_config: serde_json::json!({
                    "env": {
                        "OPENAI_API_KEY": "sk-your-api-key-here"
                    }
                }),
                website_url: Some("https://platform.openai.com".to_string()),
                category: Some("third_party".to_string()),
                created_at: Some(chrono::Utc::now().timestamp_millis()),
                sort_index: Some(0),
                notes: Some("演示供应商 - OpenAI".to_string()),
                meta: Some(ProviderMeta::default()),
                icon: Some("openai".to_string()),
                icon_color: Some("#00A67E".to_string()),
                in_failover_queue: false,
            },
            Provider {
                id: "anthropic-demo".to_string(),
                name: "Anthropic".to_string(),
                settings_config: serde_json::json!({
                    "env": {
                        "ANTHROPIC_AUTH_TOKEN": "sk-ant-your-api-key-here"
                    }
                }),
                website_url: Some("https://console.anthropic.com".to_string()),
                category: Some("third_party".to_string()),
                created_at: Some(chrono::Utc::now().timestamp_millis() + 1),
                sort_index: Some(1),
                notes: Some("演示供应商 - Anthropic Claude".to_string()),
                meta: Some(ProviderMeta::default()),
                icon: Some("anthropic".to_string()),
                icon_color: Some("#D4915D".to_string()),
                in_failover_queue: false,
            },
            Provider {
                id: "gemini-demo".to_string(),
                name: "Google Gemini".to_string(),
                settings_config: serde_json::json!({
                    "env": {
                        "GEMINI_API_KEY": "your-api-key-here"
                    }
                }),
                website_url: Some("https://ai.google.dev".to_string()),
                category: Some("third_party".to_string()),
                created_at: Some(chrono::Utc::now().timestamp_millis() + 2),
                sort_index: Some(2),
                notes: Some("演示供应商 - Google Gemini".to_string()),
                meta: Some(ProviderMeta::default()),
                icon: Some("gemini".to_string()),
                icon_color: Some("#4285F4".to_string()),
                in_failover_queue: false,
            },
        ];

        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("Failed to start transaction: {e}")))?;

        for provider in providers {
            let meta_str = serde_json::to_string(&provider.meta)
                .map_err(|e| AppError::Database(format!("Failed to serialize meta: {e}")))?;
            let settings_str = serde_json::to_string(&provider.settings_config)
                .map_err(|e| AppError::Database(format!("Failed to serialize settings: {e}")))?;

            tx.execute(
                "INSERT INTO providers (
                    id, app_type, name, settings_config, website_url, category,
                    created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(id, app_type) DO NOTHING",
                params![
                    provider.id,
                    "claude", // 默认添加到 Claude 应用
                    provider.name,
                    settings_str,
                    provider.website_url,
                    provider.category,
                    provider.created_at,
                    provider.sort_index,
                    provider.notes,
                    provider.icon,
                    provider.icon_color,
                    meta_str,
                    false, // is_current
                    provider.in_failover_queue,
                ],
            )
            .map_err(|e| AppError::Database(format!("Failed to seed provider: {e}")))?;
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("Failed to commit seed transaction: {e}")))?;

        log::info!("Default providers seeded successfully");
        Ok(())
    }
}
