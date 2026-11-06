//! 提示词数据访问对象
//!
//! 提供提示词（Prompt）的 CRUD 操作。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::prompt::{GlobalPrompt, Prompt};
use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension};

impl Database {
    /// 获取指定应用类型的所有提示词
    pub fn get_prompts(&self, app_type: &str) -> Result<IndexMap<String, Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE app_type = ?1
             ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let prompt_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let content: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let enabled: bool = row.get(4)?;
                let created_at: Option<i64> = row.get(5)?;
                let updated_at: Option<i64> = row.get(6)?;

                Ok((
                    id.clone(),
                    Prompt {
                        id,
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

    // Backfill and activation must commit together with the managed file.
    pub(crate) fn save_app_prompts(
        &self,
        app_type: &str,
        prompts: &IndexMap<String, Prompt>,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let transaction = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        for prompt in prompts.values() {
            transaction
                .execute(
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
                        prompt.updated_at
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        transaction
            .commit()
            .map_err(|e| AppError::Database(e.to_string()))
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

    // ========== 全局提示词 ==========

    /// 所有全局提示词，按创建时间排序
    pub fn get_global_prompts(&self) -> Result<IndexMap<String, GlobalPrompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, description, created_at, updated_at
                 FROM global_prompts ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], global_prompt_from_row)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut prompts = IndexMap::new();
        for row in rows {
            let prompt = row.map_err(|e| AppError::Database(e.to_string()))?;
            prompts.insert(prompt.id.clone(), prompt);
        }
        Ok(prompts)
    }

    pub fn get_global_prompt(&self, id: &str) -> Result<Option<GlobalPrompt>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, name, content, description, created_at, updated_at
             FROM global_prompts WHERE id = ?1",
            params![id],
            global_prompt_from_row,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn save_global_prompt(&self, prompt: &GlobalPrompt) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO global_prompts
                (id, name, content, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                prompt.id,
                prompt.name,
                prompt.content,
                prompt.description,
                prompt.created_at,
                prompt.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除全局提示词，连同各应用里对应的记录（同一事务）。
    /// 调用方负责先确认它在任何应用里都没有启用。
    pub(crate) fn delete_global_prompt_with_links(
        &self,
        id: &str,
        linked_id: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute("DELETE FROM prompts WHERE id = ?1", params![linked_id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute("DELETE FROM global_prompts WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))
    }

    /// 把某个应用里的一条提示词转成全局提示词：新建全局记录，并把原记录换成
    /// 关联 id（其余字段、启用状态原样保留），同一事务提交，不碰目标文件。
    pub(crate) fn promote_prompt_to_global(
        &self,
        app_type: &str,
        old_id: &str,
        linked: &Prompt,
        global: &GlobalPrompt,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "INSERT INTO global_prompts
                (id, name, content, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                global.id,
                global.name,
                global.content,
                global.description,
                global.created_at,
                global.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "INSERT INTO prompts (
                id, app_type, name, content, description, enabled, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                linked.id,
                app_type,
                linked.name,
                linked.content,
                linked.description,
                linked.enabled,
                linked.created_at,
                linked.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM prompts WHERE id = ?1 AND app_type = ?2",
            params![old_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))
    }
}

fn global_prompt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GlobalPrompt> {
    Ok(GlobalPrompt {
        id: row.get(0)?,
        name: row.get(1)?,
        content: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}
