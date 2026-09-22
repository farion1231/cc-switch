//! 隐私替换插件映射表 DAO
//!
//! `privacy_mapping` 表存储敏感信息替换标记（id）到原文的映射。
//! 注意：**明文存储原文是安全边界**，数据库文件本身需要用户妥善保护。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

impl Database {
    /// 全量载入隐私映射，按 created_at 升序（旧记录在前）。
    /// 返回 `(id, original, label, created_at)` 四元组列表。
    pub fn load_all_privacy_mappings(
        &self,
    ) -> Result<Vec<(String, String, String, i64)>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, original, label, created_at FROM privacy_mapping ORDER BY created_at ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 写入（或覆盖）一条映射
    pub fn upsert_privacy_mapping(
        &self,
        id: &str,
        original: &str,
        label: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO privacy_mapping (id, original, label, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, original, label, chrono::Utc::now().timestamp_millis()],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 created_at 早于指定时间戳的映射，返回删除条数
    pub fn delete_privacy_mappings_older_than(&self, created_at: i64) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM privacy_mapping WHERE created_at < ?1",
            params![created_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::database::Database;

    #[test]
    fn test_privacy_mapping_crud_roundtrip() {
        let db = Database::memory().expect("memory db");

        assert!(db.load_all_privacy_mappings().unwrap().is_empty());

        db.upsert_privacy_mapping("abc", "C:\\Users\\a", "PATH")
            .unwrap();
        db.upsert_privacy_mapping("def", "a@b.com", "EMAIL")
            .unwrap();

        let rows = db.load_all_privacy_mappings().unwrap();
        assert_eq!(rows.len(), 2);
        // created_at 同毫秒时排序不稳定，按 id 查找断言
        let abc = rows.iter().find(|r| r.0 == "abc").unwrap();
        let def = rows.iter().find(|r| r.0 == "def").unwrap();
        assert_eq!(abc.1, "C:\\Users\\a");
        assert_eq!(abc.2, "PATH");
        assert_eq!(def.1, "a@b.com");

        // upsert 覆盖同 id（created_at 同毫秒时排序不稳定，按 id 断言）
        db.upsert_privacy_mapping("abc", "C:\\Users\\b", "PATH")
            .unwrap();
        let rows = db.load_all_privacy_mappings().unwrap();
        assert_eq!(rows.len(), 2);
        let abc = rows.iter().find(|r| r.0 == "abc").unwrap();
        assert_eq!(abc.1, "C:\\Users\\b");

        // 按时间淘汰：created_at 晚于全部记录的时间戳时应删 2 条
        let now = chrono::Utc::now().timestamp_millis();
        assert_eq!(db.delete_privacy_mappings_older_than(now + 1).unwrap(), 2);
        assert!(db.load_all_privacy_mappings().unwrap().is_empty());
    }
}
