//! 提示词数据访问对象
//!
//! 提供提示词（Prompt）的 CRUD 操作。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::prompt::Prompt;
use indexmap::IndexMap;
use rusqlite::{params, Transaction};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PromptSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: i64,
}

impl Database {
    /// 获取指定应用类型的所有提示词
    pub fn get_prompts(&self, app_type: &str) -> Result<IndexMap<String, Prompt>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, description, enabled, created_at, updated_at
             FROM prompts WHERE app_type = ?1
             ORDER BY sort_index IS NULL, sort_index, created_at ASC, id ASC",
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
            "INSERT INTO prompts (
                id, app_type, name, content, description, enabled, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id, app_type) DO UPDATE SET
                name = excluded.name,
                content = excluded.content,
                description = excluded.description,
                enabled = excluded.enabled,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at",
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

    /// 原子更新单个提示词的启用状态。
    pub fn set_prompt_enabled(
        &self,
        app_type: &str,
        id: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE prompts SET enabled = ?1 WHERE id = ?2 AND app_type = ?3",
                params![enabled, id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if affected == 0 {
            return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
        }
        Ok(())
    }

    /// 原子替换指定应用的完整启用集合。
    pub fn set_enabled_prompts(
        &self,
        app_type: &str,
        enabled_ids: &[String],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "UPDATE prompts SET enabled = 0 WHERE app_type = ?1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        for id in enabled_ids {
            let affected = tx
                .execute(
                    "UPDATE prompts SET enabled = 1 WHERE id = ?1 AND app_type = ?2",
                    params![id, app_type],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            if affected == 0 {
                return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
            }
        }
        commit(tx)
    }

    /// 原子更新提示词显示及合并顺序。
    pub fn update_prompts_sort_order(
        &self,
        app_type: &str,
        updates: &[PromptSortUpdate],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        for update in updates {
            tx.execute(
                "UPDATE prompts SET sort_index = ?1 WHERE id = ?2 AND app_type = ?3",
                params![update.sort_index, update.id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        commit(tx)
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

fn commit(tx: Transaction<'_>) -> Result<(), AppError> {
    tx.commit().map_err(|e| AppError::Database(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(id: &str, created_at: i64) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: format!("content-{id}"),
            description: None,
            enabled: false,
            created_at: Some(created_at),
            updated_at: Some(created_at),
        }
    }

    #[test]
    fn prompt_order_and_enabled_set_are_persisted() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_prompt("codex", &prompt("a", 1))?;
        db.save_prompt("codex", &prompt("b", 2))?;
        db.save_prompt("codex", &prompt("c", 3))?;

        db.update_prompts_sort_order(
            "codex",
            &[
                PromptSortUpdate {
                    id: "c".into(),
                    sort_index: 0,
                },
                PromptSortUpdate {
                    id: "a".into(),
                    sort_index: 1,
                },
                PromptSortUpdate {
                    id: "b".into(),
                    sort_index: 2,
                },
            ],
        )?;
        let prompts = db.get_prompts("codex")?;
        assert_eq!(
            prompts.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );

        db.set_enabled_prompts("codex", &["c".into(), "b".into()])?;
        let prompts = db.get_prompts("codex")?;
        assert!(prompts["c"].enabled);
        assert!(!prompts["a"].enabled);
        assert!(prompts["b"].enabled);
        Ok(())
    }
}
