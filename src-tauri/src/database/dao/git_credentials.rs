//! Git 凭据元数据的数据访问对象
//!
//! 这里只碰元数据（id / host / 名称 / 时间）。token 本体存在系统密钥链里，
//! 见 `services::git_credential`。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::git_credential::GitCredential;
use rusqlite::params;

impl Database {
    /// 列出所有凭据，按名称排序——下拉框直接照这个顺序展示。
    pub fn list_git_credentials(&self) -> Result<Vec<GitCredential>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, host, name, created_at, updated_at FROM git_credentials
                 ORDER BY name ASC, created_at ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(GitCredential {
                    id: row.get(0)?,
                    host: row.get(1)?,
                    name: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut credentials = Vec::new();
        for row in rows {
            credentials.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(credentials)
    }

    /// 按 id 取一枚凭据的元数据。
    pub fn get_git_credential(&self, id: &str) -> Result<Option<GitCredential>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, host, name, created_at, updated_at FROM git_credentials WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok(GitCredential {
                    id: row.get(0)?,
                    host: row.get(1)?,
                    name: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        match rows.next() {
            Some(row) => Ok(Some(row.map_err(|e| AppError::Database(e.to_string()))?)),
            None => Ok(None),
        }
    }

    /// 新增或覆盖一枚凭据的元数据。
    pub fn save_git_credential(&self, credential: &GitCredential) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO git_credentials (id, host, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                credential.id,
                credential.host,
                credential.name,
                credential.created_at,
                credential.updated_at
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除一枚凭据的元数据，并解除引用它的仓库的绑定。
    ///
    /// 两件事必须在同一个事务里：只删元数据会把仓库留在一个指向不存在凭据的状态，
    /// 那之后它每次下载都报"未配置令牌"，而用户明明配置过。
    ///
    /// 返回被清空引用的仓库坐标，供调用方提示影响面。
    pub fn delete_git_credential(&self, id: &str) -> Result<Vec<(String, String)>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let affected = {
            let mut stmt = tx
                .prepare(
                    "SELECT owner, name FROM skill_repo_credentials WHERE credential_id = ?1 ORDER BY owner, name",
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| AppError::Database(e.to_string()))?;
            let mut affected = Vec::new();
            for row in rows {
                affected.push(row.map_err(|e| AppError::Database(e.to_string()))?);
            }
            affected
        };

        tx.execute(
            "DELETE FROM skill_repo_credentials WHERE credential_id = ?1",
            params![id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute("DELETE FROM git_credentials WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;

        Ok(affected)
    }
}
