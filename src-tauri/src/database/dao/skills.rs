//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。
//!
//! v3.10.0+ 统一管理架构：
//! - Skills 使用统一的 id 主键，支持四应用启用标志
//! - 实际文件存储在 ~/.cc-switch/skills/，同步到各应用目录

use crate::app_config::{InstalledSkill, SkillApps, SkillGroup};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use indexmap::IndexMap;
use rusqlite::params;
use std::collections::BTreeSet;

pub const SKILL_GROUP_COLORS: &[&str] = &[
    "blue", "violet", "emerald", "amber", "rose", "cyan", "slate",
];

fn normalize_skill_group_name(name: &str) -> Result<String, AppError> {
    let normalized = name.trim();
    let length = normalized.chars().count();
    if !(1..=50).contains(&length) {
        return Err(AppError::InvalidInput(
            "Skill group name must contain 1 to 50 characters".to_string(),
        ));
    }
    if ["ungrouped", "未分组", "未分組", "グループなし"]
        .iter()
        .any(|reserved| normalized.eq_ignore_ascii_case(reserved))
    {
        return Err(AppError::InvalidInput(
            "This Skill group name is reserved".to_string(),
        ));
    }
    Ok(normalized.to_string())
}

fn validate_skill_group_color(color: &str) -> Result<(), AppError> {
    if SKILL_GROUP_COLORS.contains(&color) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "Unsupported Skill group color: {color}"
        )))
    }
}

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, group_id, enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild,
                        enabled_opencode, enabled_hermes, installed_at, content_hash, updated_at
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
                    group_id: row.get(8)?,
                    apps: SkillApps {
                        claude: row.get(9)?,
                        codex: row.get(10)?,
                        gemini: row.get(11)?,
                        grokbuild: row.get(12)?,
                        opencode: row.get(13)?,
                        hermes: row.get(14)?,
                        pi: false,
                    },
                    installed_at: row.get(15)?,
                    content_hash: row.get(16)?,
                    updated_at: row.get::<_, i64>(17).unwrap_or(0),
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
                        readme_url, group_id, enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild,
                        enabled_opencode, enabled_hermes, installed_at, content_hash, updated_at
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
                group_id: row.get(8)?,
                apps: SkillApps {
                    claude: row.get(9)?,
                    codex: row.get(10)?,
                    gemini: row.get(11)?,
                    grokbuild: row.get(12)?,
                    opencode: row.get(13)?,
                    hermes: row.get(14)?,
                    pi: false,
                },
                installed_at: row.get(15)?,
                content_hash: row.get(16)?,
                updated_at: row.get::<_, i64>(17).unwrap_or(0),
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
              readme_url, group_id, enabled_claude, enabled_codex, enabled_gemini, enabled_grokbuild, enabled_opencode, enabled_hermes,
              installed_at, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.directory,
                skill.repo_owner,
                skill.repo_name,
                skill.repo_branch,
                skill.readme_url,
                skill.group_id,
                skill.apps.claude,
                skill.apps.codex,
                skill.apps.gemini,
                skill.apps.grokbuild,
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

    /// 仅更新已安装 Skill 的元数据，不修改各应用的启用状态。
    ///
    /// 与 [`Self::save_skill`] 不同，本方法不会插入缺失记录。更新操作可能在网络
    /// 下载期间与启用状态切换或卸载并发发生，因此调用方必须保留数据库中的
    /// `enabled_*` 字段，并在记录已被删除时停止后续处理。
    pub fn update_skill_metadata(&self, skill: &InstalledSkill) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills
                 SET name = ?1,
                     description = ?2,
                     directory = ?3,
                     repo_owner = ?4,
                     repo_name = ?5,
                     repo_branch = ?6,
                     readme_url = ?7,
                     installed_at = ?8,
                     content_hash = ?9,
                     updated_at = ?10
                 WHERE id = ?11 AND installed_at = ?12",
                params![
                    skill.name,
                    skill.description,
                    skill.directory,
                    skill.repo_owner,
                    skill.repo_name,
                    skill.repo_branch,
                    skill.readme_url,
                    skill.installed_at,
                    skill.content_hash,
                    skill.updated_at,
                    skill.id,
                    skill.installed_at,
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
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
                "UPDATE skills SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3, enabled_grokbuild = ?4, enabled_opencode = ?5, enabled_hermes = ?6 WHERE id = ?7",
                params![apps.claude, apps.codex, apps.gemini, apps.grokbuild, apps.opencode, apps.hermes, id],
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

    // ========== SkillGroup CRUD ==========

    pub fn get_skill_groups(&self) -> Result<Vec<SkillGroup>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, color, created_at
                 FROM skill_groups ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SkillGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    pub fn create_skill_group(
        &self,
        name: &str,
        color: &str,
        skill_ids: &[String],
    ) -> Result<SkillGroup, AppError> {
        let name = normalize_skill_group_name(name)?;
        validate_skill_group_color(color)?;
        let skill_ids = skill_ids.iter().collect::<BTreeSet<_>>();
        let mut group = SkillGroup {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            color: color.to_string(),
            created_at: 0,
        };

        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction().map_err(AppError::from)?;
        let last_created_at: Option<i64> = tx
            .query_row("SELECT MAX(created_at) FROM skill_groups", [], |row| {
                row.get(0)
            })
            .map_err(AppError::from)?;
        group.created_at = chrono::Utc::now()
            .timestamp_millis()
            .max(last_created_at.unwrap_or(0).saturating_add(1));
        let duplicate: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skill_groups WHERE name = ?1 COLLATE NOCASE)",
                [&group.name],
                |row| row.get(0),
            )
            .map_err(AppError::from)?;
        if duplicate {
            return Err(AppError::InvalidInput(
                "A Skill group with this name already exists".to_string(),
            ));
        }
        tx.execute(
            "INSERT INTO skill_groups (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![group.id, group.name, group.color, group.created_at],
        )
        .map_err(AppError::from)?;
        for skill_id in skill_ids {
            let affected = tx
                .execute(
                    "UPDATE skills SET group_id = ?1 WHERE id = ?2",
                    params![group.id, skill_id],
                )
                .map_err(AppError::from)?;
            if affected == 0 {
                return Err(AppError::InvalidInput(format!(
                    "Skill not found: {skill_id}"
                )));
            }
        }
        tx.commit().map_err(AppError::from)?;
        Ok(group)
    }

    pub fn update_skill_group(
        &self,
        id: &str,
        name: &str,
        color: &str,
    ) -> Result<SkillGroup, AppError> {
        let name = normalize_skill_group_name(name)?;
        validate_skill_group_color(color)?;
        let conn = lock_conn!(self.conn);
        let duplicate: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM skill_groups
                    WHERE name = ?1 COLLATE NOCASE AND id <> ?2
                 )",
                params![name, id],
                |row| row.get(0),
            )
            .map_err(AppError::from)?;
        if duplicate {
            return Err(AppError::InvalidInput(
                "A Skill group with this name already exists".to_string(),
            ));
        }
        let affected = conn
            .execute(
                "UPDATE skill_groups SET name = ?1, color = ?2 WHERE id = ?3",
                params![name, color, id],
            )
            .map_err(AppError::from)?;
        if affected == 0 {
            return Err(AppError::InvalidInput(format!(
                "Skill group not found: {id}"
            )));
        }
        Ok(SkillGroup {
            id: id.to_string(),
            name,
            color: color.to_string(),
            created_at: conn
                .query_row(
                    "SELECT created_at FROM skill_groups WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .map_err(AppError::from)?,
        })
    }

    pub fn delete_skill_group(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skill_groups WHERE id = ?1", [id])
            .map_err(AppError::from)?;
        Ok(affected > 0)
    }

    /// Replace one group's complete member set. Skills selected from another
    /// group are moved atomically; removed members become ungrouped.
    pub fn replace_skill_group_members(
        &self,
        group_id: &str,
        skill_ids: &[String],
    ) -> Result<(), AppError> {
        let skill_ids = skill_ids.iter().collect::<BTreeSet<_>>();
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction().map_err(AppError::from)?;
        let group_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skill_groups WHERE id = ?1)",
                [group_id],
                |row| row.get(0),
            )
            .map_err(AppError::from)?;
        if !group_exists {
            return Err(AppError::InvalidInput(format!(
                "Skill group not found: {group_id}"
            )));
        }
        tx.execute(
            "UPDATE skills SET group_id = NULL WHERE group_id = ?1",
            [group_id],
        )
        .map_err(AppError::from)?;
        for skill_id in skill_ids {
            let affected = tx
                .execute(
                    "UPDATE skills SET group_id = ?1 WHERE id = ?2",
                    params![group_id, skill_id],
                )
                .map_err(AppError::from)?;
            if affected == 0 {
                return Err(AppError::InvalidInput(format!(
                    "Skill not found: {skill_id}"
                )));
            }
        }
        tx.commit().map_err(AppError::from)
    }

    /// Move an arbitrary selection to a group, or pass `None` to ungroup it.
    pub fn move_skills_to_group(
        &self,
        skill_ids: &[String],
        group_id: Option<&str>,
    ) -> Result<(), AppError> {
        let skill_ids = skill_ids.iter().collect::<BTreeSet<_>>();
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction().map_err(AppError::from)?;
        if let Some(group_id) = group_id {
            let group_exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM skill_groups WHERE id = ?1)",
                    [group_id],
                    |row| row.get(0),
                )
                .map_err(AppError::from)?;
            if !group_exists {
                return Err(AppError::InvalidInput(format!(
                    "Skill group not found: {group_id}"
                )));
            }
        }
        for skill_id in skill_ids {
            let affected = tx
                .execute(
                    "UPDATE skills SET group_id = ?1 WHERE id = ?2",
                    params![group_id, skill_id],
                )
                .map_err(AppError::from)?;
            if affected == 0 {
                return Err(AppError::InvalidInput(format!(
                    "Skill not found: {skill_id}"
                )));
            }
        }
        tx.commit().map_err(AppError::from)
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

    /// 初始化默认的 Skill 仓库（启动时调用，每个数据库仅执行一次）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        const INITIALIZED_KEY: &str = "default_skill_repos_initialized";

        if self.get_bool_flag(INITIALIZED_KEY)? {
            return Ok(0);
        }

        // 兼容升级前已经存在的用户选择，并记录初始化状态，避免以后删空后恢复默认值。
        if !self.get_skill_repos()?.is_empty() {
            self.set_setting(INITIALIZED_KEY, "true")?;
            return Ok(0);
        }

        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        for repo in &default_store.repos {
            self.save_skill_repo(repo)?;
            count += 1;
            log::info!("初始化默认 Skill 仓库: {}/{}", repo.owner, repo.name);
        }

        self.set_setting(INITIALIZED_KEY, "true")?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;

    fn skill(id: &str, name: &str, apps: SkillApps) -> InstalledSkill {
        InstalledSkill {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(format!("{name} description")),
            directory: format!("{name}-directory"),
            repo_owner: Some("owner".to_string()),
            repo_name: Some("repo".to_string()),
            repo_branch: Some("main".to_string()),
            readme_url: Some(format!("https://example.com/{name}")),
            group_id: None,
            apps,
            installed_at: 1,
            content_hash: Some(format!("{name}-hash")),
            updated_at: 2,
        }
    }

    #[test]
    fn update_skill_metadata_preserves_enabled_apps() {
        let db = Database::memory().expect("memory db");
        let installed_apps = SkillApps::only(&AppType::Codex);
        let original = skill("owner/repo:skill", "original", installed_apps.clone());
        db.save_skill(&original).expect("seed skill");

        let mut candidate = skill(&original.id, "updated", SkillApps::only(&AppType::Claude));
        candidate.repo_branch = Some("next".to_string());
        candidate.updated_at = 42;

        assert!(db
            .update_skill_metadata(&candidate)
            .expect("update metadata"));

        let stored = db
            .get_installed_skill(&original.id)
            .expect("query skill")
            .expect("skill remains installed");
        assert_eq!(stored.name, candidate.name);
        assert_eq!(stored.description, candidate.description);
        assert_eq!(stored.directory, candidate.directory);
        assert_eq!(stored.repo_branch, candidate.repo_branch);
        assert_eq!(stored.readme_url, candidate.readme_url);
        assert_eq!(stored.content_hash, candidate.content_hash);
        assert_eq!(stored.updated_at, candidate.updated_at);
        assert_eq!(stored.apps, installed_apps);
    }

    #[test]
    fn update_skill_metadata_does_not_insert_missing_skill() {
        let db = Database::memory().expect("memory db");
        let candidate = skill(
            "owner/repo:missing",
            "missing",
            SkillApps::only(&AppType::Claude),
        );

        assert!(!db
            .update_skill_metadata(&candidate)
            .expect("missing update is not an error"));
        assert!(db
            .get_installed_skill(&candidate.id)
            .expect("query skill")
            .is_none());
    }

    #[test]
    fn update_skill_metadata_does_not_touch_reinstalled_generation() {
        let db = Database::memory().expect("memory db");
        let stale_update = skill(
            "owner/repo:skill",
            "stale-update",
            SkillApps::only(&AppType::Claude),
        );

        let mut reinstalled = skill(
            &stale_update.id,
            "reinstalled",
            SkillApps::only(&AppType::Gemini),
        );
        reinstalled.installed_at = stale_update.installed_at + 1;
        db.save_skill(&reinstalled).expect("seed reinstalled skill");

        assert!(!db
            .update_skill_metadata(&stale_update)
            .expect("stale generation update is not an error"));

        let stored = db
            .get_installed_skill(&reinstalled.id)
            .expect("query skill")
            .expect("reinstalled generation remains");
        assert_eq!(stored.name, reinstalled.name);
        assert_eq!(stored.installed_at, reinstalled.installed_at);
        assert_eq!(stored.apps, reinstalled.apps);
    }

    #[test]
    fn skill_groups_move_members_atomically_and_delete_to_ungrouped() {
        let db = Database::memory().expect("memory db");
        let first = skill("owner/repo:first", "first", SkillApps::default());
        let second = skill("owner/repo:second", "second", SkillApps::default());
        db.save_skill(&first).expect("seed first");
        db.save_skill(&second).expect("seed second");

        let work = db
            .create_skill_group(" Work ", "blue", std::slice::from_ref(&first.id))
            .expect("create work group");
        let personal = db
            .create_skill_group("Personal", "violet", &[])
            .expect("create personal group");
        assert_eq!(work.name, "Work");
        assert_eq!(
            db.get_skill_groups().expect("groups"),
            vec![work.clone(), personal.clone()]
        );

        db.replace_skill_group_members(&personal.id, &[first.id.clone(), second.id.clone()])
            .expect("move both to personal");
        assert_eq!(
            db.get_installed_skill(&first.id)
                .expect("query first")
                .expect("first")
                .group_id
                .as_deref(),
            Some(personal.id.as_str())
        );
        assert_eq!(
            db.get_installed_skill(&second.id)
                .expect("query second")
                .expect("second")
                .group_id
                .as_deref(),
            Some(personal.id.as_str())
        );

        assert!(db.delete_skill_group(&personal.id).expect("delete group"));
        assert!(db
            .get_installed_skill(&first.id)
            .expect("query first")
            .expect("first")
            .group_id
            .is_none());
        assert!(db
            .get_installed_skill(&second.id)
            .expect("query second")
            .expect("second")
            .group_id
            .is_none());
    }

    #[test]
    fn skill_group_validation_rejects_reserved_duplicate_and_unknown_color() {
        let db = Database::memory().expect("memory db");
        db.create_skill_group("Work", "emerald", &[])
            .expect("create group");

        assert!(db.create_skill_group("work", "blue", &[]).is_err());
        assert!(db.create_skill_group("未分组", "blue", &[]).is_err());
        assert!(db.create_skill_group("Valid", "magenta", &[]).is_err());
    }
}
