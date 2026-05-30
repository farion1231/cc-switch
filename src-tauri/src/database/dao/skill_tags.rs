//! Skill Tags 数据访问对象
//!
//! 提供技能标签和标签分配的 CRUD 操作。
//! 标签用于在 UI 层对 Skills 进行分组管理，不影响底层文件存储和同步逻辑。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// 技能标签
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTag {
    pub id: i64,
    pub name: String,
    pub sort_index: i32,
    pub created_at: i64,
}

impl Database {
    // ========== SkillTag CRUD ==========

    /// 获取所有标签（按 sort_index 排序）
    pub fn get_all_skill_tags(&self) -> Result<Vec<SkillTag>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id, name, sort_index, created_at FROM skill_tags ORDER BY sort_index ASC, id ASC")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let tags = stmt
            .query_map([], |row| {
                Ok(SkillTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    sort_index: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(tags)
    }

    /// 创建标签
    pub fn create_skill_tag(&self, name: &str) -> Result<SkillTag, AppError> {
        let conn = lock_conn!(self.conn);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // 获取当前最大 sort_index
        let max_index: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_index), -1) FROM skill_tags",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        conn.execute(
            "INSERT INTO skill_tags (name, sort_index, created_at) VALUES (?1, ?2, ?3)",
            params![name, max_index + 1, now],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::Database(format!("标签 \"{name}\" 已存在"))
            } else {
                AppError::Database(e.to_string())
            }
        })?;

        let id = conn.last_insert_rowid();
        Ok(SkillTag {
            id,
            name: name.to_string(),
            sort_index: max_index + 1,
            created_at: now,
        })
    }

    /// 更新标签名称
    pub fn update_skill_tag(&self, id: i64, name: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skill_tags SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    AppError::Database(format!("标签 \"{name}\" 已存在"))
                } else {
                    AppError::Database(e.to_string())
                }
            })?;
        Ok(affected > 0)
    }

    /// 删除标签（关联的分配记录会通过 CASCADE 自动删除）
    pub fn delete_skill_tag(&self, id: i64) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skill_tags WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 批量更新标签排序
    pub fn reorder_skill_tags(&self, ordered_ids: &[i64]) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("UPDATE skill_tags SET sort_index = ?1 WHERE id = ?2")
            .map_err(|e| AppError::Database(e.to_string()))?;

        for (index, id) in ordered_ids.iter().enumerate() {
            stmt.execute(params![index as i32, id])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(())
    }

    // ========== Tag Assignments ==========

    /// 为 skill 分配标签（替换现有分配）
    pub fn set_skill_tags(&self, skill_id: &str, tag_ids: &[i64]) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 先删除该 skill 的所有现有分配
        conn.execute(
            "DELETE FROM skill_tag_assignments WHERE skill_id = ?1",
            params![skill_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 插入新分配
        let mut stmt = conn
            .prepare("INSERT INTO skill_tag_assignments (skill_id, tag_id) VALUES (?1, ?2)")
            .map_err(|e| AppError::Database(e.to_string()))?;

        for tag_id in tag_ids {
            stmt.execute(params![skill_id, tag_id])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// 为 skill 添加单个标签
    pub fn assign_skill_tag(&self, skill_id: &str, tag_id: i64) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR IGNORE INTO skill_tag_assignments (skill_id, tag_id) VALUES (?1, ?2)",
            params![skill_id, tag_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 移除 skill 的单个标签
    pub fn unassign_skill_tag(&self, skill_id: &str, tag_id: i64) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "DELETE FROM skill_tag_assignments WHERE skill_id = ?1 AND tag_id = ?2",
                params![skill_id, tag_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 获取单个 skill 的标签 ID 列表
    pub fn get_skill_tag_ids(&self, skill_id: &str) -> Result<Vec<i64>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT tag_id FROM skill_tag_assignments WHERE skill_id = ?1 ORDER BY tag_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let ids = stmt
            .query_map([skill_id], |row| row.get::<_, i64>(0))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(ids)
    }

    /// 获取所有标签分配关系（用于前端批量加载）
    pub fn get_all_skill_tag_assignments(&self) -> Result<Vec<(String, i64)>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT skill_id, tag_id FROM skill_tag_assignments ORDER BY skill_id, tag_id")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let assignments = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(assignments)
    }

    /// 获取指定标签下的所有 skill ID
    pub fn get_skills_by_tag(&self, tag_id: i64) -> Result<Vec<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT skill_id FROM skill_tag_assignments WHERE tag_id = ?1 ORDER BY skill_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let ids = stmt
            .query_map([tag_id], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(ids)
    }
}
