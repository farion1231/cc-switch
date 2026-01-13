//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。
//!
//! v3.10.0+ 统一管理架构：
//! - Skills 使用统一的 id 主键，支持三应用启用标志
//! - 实际文件存储在 ~/.cc-switch/skills/，同步到各应用目录

use crate::app_config::{InstalledSkill, SkillApps};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, installed_at
                 FROM skills ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let skill_iter = stmt
            .query_map([], |row| {
                Ok(InstalledSkill {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    directory: row.get(3)?,
                    repo_owner: row.get(4)?,
                    repo_name: row.get(5)?,
                    repo_branch: row.get(6)?,
                    readme_url: row.get(7)?,
                    apps: SkillApps {
                        claude: row.get(8)?,
                        codex: row.get(9)?,
                        gemini: row.get(10)?,
                    },
                    installed_at: row.get(11)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut skills = IndexMap::new();
        for skill_res in skill_iter {
            let skill = skill_res.map_err(|e| AppError::Database(e.to_string()))?;
            skills.insert(skill.id.clone(), skill);
        }
        Ok(skills)
    }

    /// 获取单个已安装的 Skill
    pub fn get_installed_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, installed_at
                 FROM skills WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([id], |row| {
            Ok(InstalledSkill {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                directory: row.get(3)?,
                repo_owner: row.get(4)?,
                repo_name: row.get(5)?,
                repo_branch: row.get(6)?,
                readme_url: row.get(7)?,
                apps: SkillApps {
                    claude: row.get(8)?,
                    codex: row.get(9)?,
                    gemini: row.get(10)?,
                },
                installed_at: row.get(11)?,
            })
        });

        match result {
            Ok(skill) => Ok(Some(skill)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 保存 Skill（添加或更新）
    pub fn save_skill(&self, skill: &InstalledSkill) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills
             (id, name, description, directory, repo_owner, repo_name, repo_branch,
              readme_url, enabled_claude, enabled_codex, enabled_gemini, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.directory,
                skill.repo_owner,
                skill.repo_name,
                skill.repo_branch,
                skill.readme_url,
                skill.apps.claude,
                skill.apps.codex,
                skill.apps.gemini,
                skill.installed_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skill
    pub fn delete_skill(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skills WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 清空所有 Skills（用于迁移）
    pub fn clear_skills(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM skills", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 更新 Skill 的应用启用状态
    pub fn update_skill_apps(&self, id: &str, apps: &SkillApps) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3 WHERE id = ?4",
                params![apps.claude, apps.codex, apps.gemini, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    // ========== SkillRepo CRUD（保持原有） ==========

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
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
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
