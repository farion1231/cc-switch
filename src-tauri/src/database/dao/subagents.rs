//! Claude Code 子代理（subagent）数据访问对象
//!
//! 数据库里存完整原始 markdown（唯一事实源），`name` / `description` 只是从
//! frontmatter 解析出来的冗余列，方便排序和展示。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension, Row};

/// subagents 表的一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRecord {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub description: Option<String>,
    pub content: String,
    pub enabled: bool,
    pub source: String,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub repo_path: Option<String>,
    /// 最近一次由 CC Switch 写入（或导入时读到）的磁盘文件 sha256；
    /// 停用后文件已删除，置空
    pub written_hash: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

const SELECT_COLUMNS: &str = "id, name, file_name, description, content, enabled, source,
    repo_owner, repo_name, repo_path, written_hash, created_at, updated_at";

// 不能用 INSERT OR REPLACE：file_name 有 UNIQUE 约束，REPLACE 会把占用同名的
// 另一条记录静默删掉。ON CONFLICT(id) 只合并同 id，file_name 冲突照常报错。
const UPSERT_SQL: &str = "INSERT INTO subagents (
        id, name, file_name, description, content, enabled, source,
        repo_owner, repo_name, repo_path, written_hash, created_at, updated_at
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
    ON CONFLICT(id) DO UPDATE SET
        name = excluded.name,
        file_name = excluded.file_name,
        description = excluded.description,
        content = excluded.content,
        enabled = excluded.enabled,
        source = excluded.source,
        repo_owner = excluded.repo_owner,
        repo_name = excluded.repo_name,
        repo_path = excluded.repo_path,
        written_hash = excluded.written_hash,
        created_at = excluded.created_at,
        updated_at = excluded.updated_at";

fn map_row(row: &Row<'_>) -> rusqlite::Result<SubagentRecord> {
    Ok(SubagentRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        file_name: row.get(2)?,
        description: row.get(3)?,
        content: row.get(4)?,
        enabled: row.get(5)?,
        source: row.get(6)?,
        repo_owner: row.get(7)?,
        repo_name: row.get(8)?,
        repo_path: row.get(9)?,
        written_hash: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn upsert_on(conn: &rusqlite::Connection, record: &SubagentRecord) -> Result<(), AppError> {
    conn.execute(
        UPSERT_SQL,
        params![
            record.id,
            record.name,
            record.file_name,
            record.description,
            record.content,
            record.enabled,
            record.source,
            record.repo_owner,
            record.repo_name,
            record.repo_path,
            record.written_hash,
            record.created_at,
            record.updated_at,
        ],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

impl Database {
    /// 全部子代理，按 name 排序（同名再按 file_name，保证顺序稳定）
    pub fn list_subagents(&self) -> Result<Vec<SubagentRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM subagents ORDER BY name ASC, file_name ASC"
            ))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], map_row)
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_subagent(&self, id: &str) -> Result<Option<SubagentRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM subagents WHERE id = ?1"),
            params![id],
            map_row,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn get_subagent_by_file_name(
        &self,
        file_name: &str,
    ) -> Result<Option<SubagentRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM subagents WHERE file_name = ?1"),
            params![file_name],
            map_row,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn save_subagent(&self, record: &SubagentRecord) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        upsert_on(&conn, record)
    }

    /// 批量导入要么全部落库要么全部不落，避免导入一半留下难以解释的状态
    pub fn save_subagents(&self, records: &[SubagentRecord]) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        for record in records {
            upsert_on(&tx, record)?;
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn delete_subagent(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM subagents WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
