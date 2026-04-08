//! Rules 数据访问对象
//!
//! 提供 Rules 和 Rule Repos 的 CRUD 操作。
//! 结构与 Skills DAO 对称。

use crate::app_config::{InstalledRule, RuleApps};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::rule::RuleRepo;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    // ========== InstalledRule CRUD ==========

    pub fn get_all_installed_rules(&self) -> Result<IndexMap<String, InstalledRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
                 FROM rules ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(InstalledRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    directory: row.get(3)?,
                    repo_owner: row.get(4)?,
                    repo_name: row.get(5)?,
                    repo_branch: row.get(6)?,
                    readme_url: row.get(7)?,
                    apps: RuleApps {
                        claude: row.get(8)?,
                        codex: row.get(9)?,
                        gemini: row.get(10)?,
                        opencode: row.get(11)?,
                    },
                    installed_at: row.get(12)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rules = IndexMap::new();
        for res in iter {
            let rule = res.map_err(|e| AppError::Database(e.to_string()))?;
            rules.insert(rule.id.clone(), rule);
        }
        Ok(rules)
    }

    pub fn get_installed_rule(&self, id: &str) -> Result<Option<InstalledRule>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
                 FROM rules WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([id], |row| {
            Ok(InstalledRule {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                directory: row.get(3)?,
                repo_owner: row.get(4)?,
                repo_name: row.get(5)?,
                repo_branch: row.get(6)?,
                readme_url: row.get(7)?,
                apps: RuleApps {
                    claude: row.get(8)?,
                    codex: row.get(9)?,
                    gemini: row.get(10)?,
                    opencode: row.get(11)?,
                },
                installed_at: row.get(12)?,
            })
        });

        match result {
            Ok(rule) => Ok(Some(rule)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn save_rule(&self, rule: &InstalledRule) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO rules
             (id, name, description, directory, repo_owner, repo_name, repo_branch,
              readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rule.id,
                rule.name,
                rule.description,
                rule.directory,
                rule.repo_owner,
                rule.repo_name,
                rule.repo_branch,
                rule.readme_url,
                rule.apps.claude,
                rule.apps.codex,
                rule.apps.gemini,
                rule.apps.opencode,
                rule.installed_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM rules WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    pub fn update_rule_apps(&self, id: &str, apps: &RuleApps) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE rules SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3, enabled_opencode = ?4 WHERE id = ?5",
                params![apps.claude, apps.codex, apps.gemini, apps.opencode, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    // ========== RuleRepo CRUD ==========

    pub fn get_rule_repos(&self) -> Result<Vec<RuleRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled FROM rule_repos ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(RuleRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut repos = Vec::new();
        for res in iter {
            repos.push(res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(repos)
    }

    pub fn save_rule_repo(&self, repo: &RuleRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO rule_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_rule_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM rule_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn init_default_rule_repos(&self) -> Result<usize, AppError> {
        let existing = self.get_rule_repos()?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|r| (r.owner.clone(), r.name.clone()))
            .collect();

        let defaults = crate::services::rule::default_rule_repos();
        let mut count = 0;

        for repo in &defaults {
            let key = (repo.owner.clone(), repo.name.clone());
            if !existing_keys.contains(&key) {
                self.save_rule_repo(repo)?;
                count += 1;
                log::info!("补充默认 Rule 仓库: {}/{}", repo.owner, repo.name);
            }
        }

        if count > 0 {
            log::info!("补充默认 Rule 仓库完成，新增 {count} 个");
        }
        Ok(count)
    }
}
