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
use crate::services::skill::{ManualSkillGroup, SkillRepo};
use indexmap::IndexMap;
use rusqlite::params;
use std::collections::BTreeSet;

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode,
                        enabled_hermes, installed_at, content_hash, updated_at
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
                        enabled_hermes, installed_at, content_hash, updated_at
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
              installed_at, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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

    // ========== Manual Skill Group CRUD ==========

    pub fn get_manual_skill_groups(&self) -> Result<Vec<ManualSkillGroup>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, created_at, updated_at
                 FROM skill_groups ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ManualSkillGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    skill_ids: Vec::new(),
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut groups = Vec::new();
        for row in rows {
            let mut group = row.map_err(|e| AppError::Database(e.to_string()))?;
            group.skill_ids = Self::get_manual_skill_group_members_locked(&conn, &group.id)?;
            groups.push(group);
        }
        Ok(groups)
    }

    pub fn create_manual_skill_group(
        &self,
        id: &str,
        name: &str,
        skill_ids: &[String],
    ) -> Result<ManualSkillGroup, AppError> {
        let now = chrono::Utc::now().timestamp();
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "INSERT INTO skill_groups (id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, name, now, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        let stored_skill_ids = Self::set_manual_skill_group_members_locked(&tx, id, skill_ids)?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(ManualSkillGroup {
            id: id.to_string(),
            name: name.to_string(),
            created_at: now,
            updated_at: now,
            skill_ids: stored_skill_ids,
        })
    }

    pub fn rename_manual_skill_group(&self, id: &str, name: &str) -> Result<bool, AppError> {
        let now = chrono::Utc::now().timestamp();
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skill_groups SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    pub fn delete_manual_skill_group(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skill_groups WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    pub fn set_manual_skill_group_members(
        &self,
        id: &str,
        skill_ids: &[String],
    ) -> Result<bool, AppError> {
        let now = chrono::Utc::now().timestamp();
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let group_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skill_groups WHERE id = ?1)",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if !group_exists {
            return Ok(false);
        }
        Self::set_manual_skill_group_members_locked(&tx, id, skill_ids)?;
        tx.execute(
            "UPDATE skill_groups SET updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(true)
    }

    pub fn get_manual_skill_group_members(
        &self,
        id: &str,
    ) -> Result<Option<Vec<String>>, AppError> {
        let conn = lock_conn!(self.conn);
        let group_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skill_groups WHERE id = ?1)",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if !group_exists {
            return Ok(None);
        }
        Self::get_manual_skill_group_members_locked(&conn, id).map(Some)
    }

    fn get_manual_skill_group_members_locked(
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<Vec<String>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT skill_id FROM skill_group_members
                 WHERE group_id = ?1 ORDER BY skill_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![id], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut skill_ids = Vec::new();
        for row in rows {
            skill_ids.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(skill_ids)
    }

    fn set_manual_skill_group_members_locked(
        conn: &rusqlite::Connection,
        id: &str,
        skill_ids: &[String],
    ) -> Result<Vec<String>, AppError> {
        let mut unique_ids = BTreeSet::new();
        for skill_id in skill_ids {
            if !unique_ids.insert(skill_id.as_str()) {
                continue;
            }
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM skills WHERE id = ?1)",
                    params![skill_id],
                    |row| row.get(0),
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            if !exists {
                return Err(AppError::InvalidInput(format!(
                    "Skill not found: {skill_id}"
                )));
            }
        }

        conn.execute(
            "DELETE FROM skill_group_members WHERE group_id = ?1",
            params![id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        for skill_id in &unique_ids {
            conn.execute(
                "INSERT INTO skill_group_members (group_id, skill_id) VALUES (?1, ?2)",
                params![id, skill_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(unique_ids.into_iter().map(ToOwned::to_owned).collect())
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
