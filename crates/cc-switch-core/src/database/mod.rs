//! Database module - SQLite persistence layer

mod dao;
mod schema;

use crate::config::database_path;
use crate::error::AppError;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

/// Database connection wrapper
pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

impl Database {
    /// Initialize database connection and create tables
    pub fn new() -> Result<Self, AppError> {
        let db_path = database_path();

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let conn = Connection::open(&db_path).map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;

        Ok(db)
    }

    /// Create in-memory database (for testing)
    pub fn memory() -> Result<Self, AppError> {
        let conn = Connection::open_in_memory().map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;

        Ok(db)
    }

    /// Check if MCP servers table is empty
    pub fn is_mcp_table_empty(&self) -> Result<bool, AppError> {
        let conn = self.lock_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_servers", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }

    /// Check if prompts table is empty
    pub fn is_prompts_table_empty(&self) -> Result<bool, AppError> {
        let conn = self.lock_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |row| row.get(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count == 0)
    }

    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("Database mutex lock failed")
    }
}

/// Safely serialize JSON
pub(crate) fn to_json_string<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))
}

/// Safely get Mutex lock
macro_rules! lock_conn {
    ($mutex:expr) => {
        $mutex
            .lock()
            .map_err(|e| AppError::Database(format!("Mutex lock failed: {}", e)))?
    };
}

pub(crate) use lock_conn;
