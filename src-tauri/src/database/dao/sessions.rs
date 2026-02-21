//! 会话别名数据访问对象
//!
//! 管理会话的自定义别名（重命名）。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use std::collections::HashMap;

impl Database {
    /// 获取所有会话别名
    pub fn get_all_session_aliases(&self) -> Result<HashMap<String, String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT session_key, alias FROM session_aliases")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut map = HashMap::new();
        for row in rows {
            let (key, alias) = row.map_err(|e| AppError::Database(e.to_string()))?;
            map.insert(key, alias);
        }
        Ok(map)
    }

    /// 设置会话别名
    pub fn set_session_alias(&self, session_key: &str, alias: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO session_aliases (session_key, alias) VALUES (?1, ?2)",
            params![session_key, alias],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除会话别名（恢复默认名称）
    pub fn delete_session_alias(&self, session_key: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM session_aliases WHERE session_key = ?1",
            params![session_key],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
