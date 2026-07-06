//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。
//!
//! v3.10.0+ 统一管理架构：
//! - Skills 使用统一的 id 主键，支持四应用启用标志
//! - 实际文件存储在 ~/.cc-switch/skills/，同步到各应用目录

use crate::app_config::{InstalledSkill, SkillApps};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::{SkillAppUpdate, SkillCategory, SkillMode, SkillRepo};
use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension};

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode,
                        enabled_hermes, installed_at, content_hash, updated_at, category
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
                        opencode: row.get(11)?,
                        hermes: row.get(12)?,
                    },
                    installed_at: row.get(13)?,
                    content_hash: row.get(14)?,
                    updated_at: row.get::<_, i64>(15).unwrap_or(0),
                    category: row.get(16)?,
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
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode,
                        enabled_hermes, installed_at, content_hash, updated_at, category
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
                    opencode: row.get(11)?,
                    hermes: row.get(12)?,
                },
                installed_at: row.get(13)?,
                content_hash: row.get(14)?,
                updated_at: row.get::<_, i64>(15).unwrap_or(0),
                category: row.get(16)?,
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
              readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, enabled_hermes,
              installed_at, content_hash, updated_at, category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                skill.apps.opencode,
                skill.apps.hermes,
                skill.installed_at,
                skill.content_hash,
                skill.updated_at,
                skill.category,
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
                "UPDATE skills SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3, enabled_opencode = ?4, enabled_hermes = ?5 WHERE id = ?6",
                params![apps.claude, apps.codex, apps.gemini, apps.opencode, apps.hermes, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 批量更新 Skill 的应用启用状态。
    pub fn bulk_update_skill_apps(&self, updates: &[SkillAppUpdate]) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let mut affected = 0usize;
        for update in updates {
            affected += conn
                .execute(
                    "UPDATE skills SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3, enabled_opencode = ?4, enabled_hermes = ?5 WHERE id = ?6",
                    params![
                        update.apps.claude,
                        update.apps.codex,
                        update.apps.gemini,
                        update.apps.opencode,
                        update.apps.hermes,
                        update.id,
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(affected)
    }

    /// 更新 Skill 分类。
    pub fn update_skill_category(
        &self,
        id: &str,
        category: Option<String>,
    ) -> Result<bool, AppError> {
        let normalized = category.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills SET category = ?1 WHERE id = ?2",
                params![normalized, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 获取所有自定义 Skills 分类。
    pub fn get_skill_categories(&self) -> Result<Vec<SkillCategory>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, created_at, updated_at FROM skill_categories ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let category_iter = stmt
            .query_map([], |row| {
                Ok(SkillCategory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut categories = Vec::new();
        for category_res in category_iter {
            categories.push(category_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(categories)
    }

    /// 获取单个 Skills 分类。
    pub fn get_skill_category(&self, id: &str) -> Result<Option<SkillCategory>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, name, created_at, updated_at FROM skill_categories WHERE id = ?1",
            params![id],
            |row| {
                Ok(SkillCategory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 保存 Skills 分类。
    pub fn save_skill_category(&self, category: &SkillCategory) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_categories (id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                category.id,
                category.name,
                category.created_at,
                category.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skills 分类，分类下的 skills 回到默认分类。
    pub fn delete_skill_category(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE skills SET category = NULL WHERE category = ?1",
            params![id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let affected = conn
            .execute("DELETE FROM skill_categories WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 获取所有 Skills 模式。
    pub fn get_skill_modes(&self) -> Result<Vec<SkillMode>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, matrix, created_at, updated_at FROM skill_modes ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mode_iter = stmt
            .query_map([], |row| {
                let matrix_json: String = row.get(2)?;
                let matrix = serde_json::from_str(&matrix_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok(SkillMode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    matrix,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut modes = Vec::new();
        for mode_res in mode_iter {
            modes.push(mode_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(modes)
    }

    /// 获取单个 Skills 模式。
    pub fn get_skill_mode(&self, id: &str) -> Result<Option<SkillMode>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, name, matrix, created_at, updated_at FROM skill_modes WHERE id = ?1",
            params![id],
            |row| {
                let matrix_json: String = row.get(2)?;
                let matrix = serde_json::from_str(&matrix_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                Ok(SkillMode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    matrix,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 保存 Skills 模式。
    pub fn save_skill_mode(&self, mode: &SkillMode) -> Result<(), AppError> {
        let matrix = serde_json::to_string(&mode.matrix)
            .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_modes (id, name, matrix, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![mode.id, mode.name, matrix, mode.created_at, mode.updated_at],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skills 模式。
    pub fn delete_skill_mode(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skill_modes WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 更新 Skill 的内容哈希和更新时间
    pub fn update_skill_hash(
        &self,
        id: &str,
        content_hash: &str,
        updated_at: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills SET content_hash = ?1, updated_at = ?2 WHERE id = ?3",
                params![content_hash, updated_at, id],
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

    /// 初始化默认的 Skill 仓库（启动时调用，补充缺失的默认仓库）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        // 获取已有仓库列表
        let existing = self.get_skill_repos()?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|r| (r.owner.clone(), r.name.clone()))
            .collect();

        // 获取默认仓库列表
        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        // 仅插入缺失的默认仓库
        for repo in &default_store.repos {
            let key = (repo.owner.clone(), repo.name.clone());
            if !existing_keys.contains(&key) {
                self.save_skill_repo(repo)?;
                count += 1;
                log::info!("补充默认 Skill 仓库: {}/{}", repo.owner, repo.name);
            }
        }

        if count > 0 {
            log::info!("补充默认 Skill 仓库完成，新增 {count} 个");
        }
        Ok(count)
    }
}
