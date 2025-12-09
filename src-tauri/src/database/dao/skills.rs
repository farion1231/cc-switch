//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::{SkillRepo, SkillState};
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    /// 获取所有 Skills 状态
    pub fn get_skills(&self) -> Result<IndexMap<String, SkillState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT key, installed, installed_at FROM skills ORDER BY key ASC")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let skill_iter = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let installed: bool = row.get(1)?;
                let installed_at_ts: i64 = row.get(2)?;

                let installed_at =
                    chrono::DateTime::from_timestamp(installed_at_ts, 0).unwrap_or_default();

                Ok((
                    key,
                    SkillState {
                        installed,
                        installed_at,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut skills = IndexMap::new();
        for skill_res in skill_iter {
            let (key, skill) = skill_res.map_err(|e| AppError::Database(e.to_string()))?;
            skills.insert(key, skill);
        }
        Ok(skills)
    }

    /// 更新 Skill 状态
    pub fn update_skill_state(&self, key: &str, state: &SkillState) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills (key, installed, installed_at) VALUES (?1, ?2, ?3)",
            params![key, state.installed, state.installed_at.timestamp()],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 获取所有 Skill 仓库
    pub fn get_skill_repos(&self) -> Result<Vec<SkillRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled FROM skill_repos ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let repo_iter = stmt
            .query_map([], |row| {
                Ok(SkillRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut repos = Vec::new();
        for repo_res in repo_iter {
            repos.push(repo_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(repos)
    }

    /// 保存 Skill 仓库
    pub fn save_skill_repo(&self, repo: &SkillRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        ).map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skill 仓库
    pub fn delete_skill_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM skill_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 初始化默认的 Skill 仓库（首次启动时调用）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        // 检查是否已有仓库
        let existing = self.get_skill_repos()?;
        if !existing.is_empty() {
            return Ok(0);
        }

        // 获取默认仓库列表
        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        for repo in &default_store.repos {
            self.save_skill_repo(repo)?;
            count += 1;
        }

        log::info!("初始化默认 Skill 仓库完成，共 {count} 个");
        Ok(count)
    }
}
