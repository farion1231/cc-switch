//! Agents 数据访问对象
//!
//! 提供 Agents 和 Agent Repos 的 CRUD 操作。
//! 结构与 Rules DAO 对称。

use crate::app_config::{AgentApps, InstalledAgent};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::agent::AgentRepo;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    // ========== InstalledAgent CRUD ==========

    pub fn get_all_installed_agents(&self) -> Result<IndexMap<String, InstalledAgent>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
                 FROM agents ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(InstalledAgent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    directory: row.get(3)?,
                    repo_owner: row.get(4)?,
                    repo_name: row.get(5)?,
                    repo_branch: row.get(6)?,
                    readme_url: row.get(7)?,
                    apps: AgentApps {
                        claude: row.get(8)?,
                        codex: row.get(9)?,
                        gemini: row.get(10)?,
                        opencode: row.get(11)?,
                    },
                    installed_at: row.get(12)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut agents = IndexMap::new();
        for res in iter {
            let agent = res.map_err(|e| AppError::Database(e.to_string()))?;
            agents.insert(agent.id.clone(), agent);
        }
        Ok(agents)
    }

    pub fn get_installed_agent(&self, id: &str) -> Result<Option<InstalledAgent>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at
                 FROM agents WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([id], |row| {
            Ok(InstalledAgent {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                directory: row.get(3)?,
                repo_owner: row.get(4)?,
                repo_name: row.get(5)?,
                repo_branch: row.get(6)?,
                readme_url: row.get(7)?,
                apps: AgentApps {
                    claude: row.get(8)?,
                    codex: row.get(9)?,
                    gemini: row.get(10)?,
                    opencode: row.get(11)?,
                },
                installed_at: row.get(12)?,
            })
        });

        match result {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn save_agent(&self, agent: &InstalledAgent) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO agents
             (id, name, description, directory, repo_owner, repo_name, repo_branch,
              readme_url, enabled_claude, enabled_codex, enabled_gemini, enabled_opencode, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                agent.id,
                agent.name,
                agent.description,
                agent.directory,
                agent.repo_owner,
                agent.repo_name,
                agent.repo_branch,
                agent.readme_url,
                agent.apps.claude,
                agent.apps.codex,
                agent.apps.gemini,
                agent.apps.opencode,
                agent.installed_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_agent(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM agents WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    pub fn update_agent_apps(&self, id: &str, apps: &AgentApps) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE agents SET enabled_claude = ?1, enabled_codex = ?2, enabled_gemini = ?3, enabled_opencode = ?4 WHERE id = ?5",
                params![apps.claude, apps.codex, apps.gemini, apps.opencode, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    // ========== AgentRepo CRUD ==========

    pub fn get_agent_repos(&self) -> Result<Vec<AgentRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled FROM agent_repos ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(AgentRepo {
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

    pub fn save_agent_repo(&self, repo: &AgentRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO agent_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn delete_agent_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM agent_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn init_default_agent_repos(&self) -> Result<usize, AppError> {
        let existing = self.get_agent_repos()?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|r| (r.owner.clone(), r.name.clone()))
            .collect();

        let defaults = crate::services::agent::default_agent_repos();
        let mut count = 0;

        for repo in &defaults {
            let key = (repo.owner.clone(), repo.name.clone());
            if !existing_keys.contains(&key) {
                self.save_agent_repo(repo)?;
                count += 1;
                log::info!("补充默认 Agent 仓库: {}/{}", repo.owner, repo.name);
            }
        }

        if count > 0 {
            log::info!("补充默认 Agent 仓库完成，新增 {count} 个");
        }
        Ok(count)
    }
}
