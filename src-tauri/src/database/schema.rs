//! Schema 定义和迁移
//!
//! 负责数据库表结构的创建和版本迁移。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::Connection;

impl Database {
    /// 创建所有数据库表
    pub(crate) fn create_tables(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::create_tables_on_conn(&conn)
    }

    /// 在指定连接上创建表（供迁移和测试使用）
    pub(crate) fn create_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
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
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                server_config TEXT NOT NULL,
                description TEXT,
                homepage TEXT,
                docs TEXT,
                tags TEXT NOT NULL DEFAULT '[]',
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 4. Prompts 表
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

        // 5. Skills 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
                key TEXT PRIMARY KEY,
                installed BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 6. Skill Repos 表
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

        // 7. Settings 表 (通用配置)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

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
                        Self::set_user_version(conn, SCHEMA_VERSION)?;
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
