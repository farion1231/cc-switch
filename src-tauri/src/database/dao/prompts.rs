//! 提示词数据访问对象
//!
//! 提供提示词（Prompt）的 CRUD 操作。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::prompt::Prompt;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    /// 获取指定应用类型的所有提示词
    pub fn get_prompts(&self, app_type: &str) -> Result<IndexMap<String, Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, app_type, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE app_type = ?1
             ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let prompt_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let app_type: String = row.get(1)?;
                let name: String = row.get(2)?;
                let content: String = row.get(3)?;
                let description: Option<String> = row.get(4)?;
                let enabled: bool = row.get(5)?;
                let created_at: Option<i64> = row.get(6)?;
                let updated_at: Option<i64> = row.get(7)?;

                Ok((
                    id.clone(),
                    Prompt {
                        id,
                        app_type,
                        name,
                        content,
                        description,
                        enabled,
                        created_at,
                        updated_at,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut prompts = IndexMap::new();
        for prompt_res in prompt_iter {
            let (id, prompt) = prompt_res.map_err(|e| AppError::Database(e.to_string()))?;
            prompts.insert(id, prompt);
        }
        Ok(prompts)
    }

    /// 根据 id 查找提示词（任意 app_type 下的唯一一条）
    pub fn get_prompt_by_id(&self, id: &str) -> Result<Option<Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, app_type, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE id = ?1
             LIMIT 1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query_map(params![id], |row| {
                Ok(Prompt {
                    id: row.get(0)?,
                    app_type: row.get(1)?,
                    name: row.get(2)?,
                    content: row.get(3)?,
                    description: row.get(4)?,
                    enabled: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        match rows.next() {
            Some(row_res) => row_res.map(Some).map_err(|e| AppError::Database(e.to_string())),
            None => Ok(None),
        }
    }

    /// 保存提示词
    pub fn save_prompt(&self, app_type: &str, prompt: &Prompt) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO prompts (
                id, app_type, name, content, description, enabled, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                prompt.id,
                app_type,
                prompt.name,
                prompt.content,
                prompt.description,
                prompt.enabled,
                prompt.created_at,
                prompt.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除提示词
    pub fn delete_prompt(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM prompts WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
