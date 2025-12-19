use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::{TemplateRepo, TemplateService};

#[allow(dead_code)]
impl TemplateService {
    /// 列出所有模板仓库
    pub fn list_repos(&self, conn: &Connection) -> Result<Vec<TemplateRepo>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, owner, name, branch, enabled, created_at, updated_at
                 FROM template_repos
                 ORDER BY created_at DESC",
            )
            .context("准备查询模板仓库语句失败")?;

        let repos = stmt
            .query_map([], |row| {
                Ok(TemplateRepo {
                    id: Some(row.get(0)?),
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get::<_, String>(5).ok().and_then(|s| s.parse().ok()),
                    updated_at: row.get::<_, String>(6).ok().and_then(|s| s.parse().ok()),
                })
            })
            .context("查询模板仓库失败")?
            .collect::<Result<Vec<_>, _>>()
            .context("收集模板仓库结果失败")?;

        Ok(repos)
    }

    /// 添加模板仓库
    pub fn add_repo(&self, conn: &Connection, repo: TemplateRepo) -> Result<i64> {
        // 检查是否已存在
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM template_repos WHERE owner = ?1 AND name = ?2",
                params![&repo.owner, &repo.name],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // 更新已存在的仓库
            conn.execute(
                "UPDATE template_repos
                 SET branch = ?1, enabled = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?3",
                params![&repo.branch, repo.enabled as i64, id],
            )
            .context("更新模板仓库失败")?;
            Ok(id)
        } else {
            // 插入新仓库
            conn.execute(
                "INSERT INTO template_repos (owner, name, branch, enabled)
                 VALUES (?1, ?2, ?3, ?4)",
                params![&repo.owner, &repo.name, &repo.branch, repo.enabled as i64],
            )
            .context("插入模板仓库失败")?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// 删除模板仓库
    pub fn remove_repo(&self, conn: &Connection, id: i64) -> Result<()> {
        let rows = conn
            .execute("DELETE FROM template_repos WHERE id = ?1", params![id])
            .context("删除模板仓库失败")?;

        if rows == 0 {
            anyhow::bail!("模板仓库不存在: id={id}");
        }

        Ok(())
    }

    /// 切换仓库启用状态
    pub fn toggle_repo_enabled(&self, conn: &Connection, id: i64) -> Result<bool> {
        // 获取当前状态
        let enabled: i64 = conn
            .query_row(
                "SELECT enabled FROM template_repos WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .context("查询仓库状态失败")?;

        let new_enabled = enabled == 0;

        // 更新状态
        conn.execute(
            "UPDATE template_repos
             SET enabled = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            params![new_enabled as i64, id],
        )
        .context("更新仓库状态失败")?;

        Ok(new_enabled)
    }

    /// 获取单个仓库
    pub fn get_repo(&self, conn: &Connection, id: i64) -> Result<TemplateRepo> {
        conn.query_row(
            "SELECT id, owner, name, branch, enabled, created_at, updated_at
             FROM template_repos
             WHERE id = ?1",
            params![id],
            |row| {
                Ok(TemplateRepo {
                    id: Some(row.get(0)?),
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get::<_, String>(5).ok().and_then(|s| s.parse().ok()),
                    updated_at: row.get::<_, String>(6).ok().and_then(|s| s.parse().ok()),
                })
            },
        )
        .context(format!("查询模板仓库失败: id={id}"))
    }

    /// 获取启用的仓库列表
    pub fn list_enabled_repos(&self, conn: &Connection) -> Result<Vec<TemplateRepo>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, owner, name, branch, enabled, created_at, updated_at
                 FROM template_repos
                 WHERE enabled = 1
                 ORDER BY created_at DESC",
            )
            .context("准备查询启用仓库语句失败")?;

        let repos = stmt
            .query_map([], |row| {
                Ok(TemplateRepo {
                    id: Some(row.get(0)?),
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get::<_, String>(5).ok().and_then(|s| s.parse().ok()),
                    updated_at: row.get::<_, String>(6).ok().and_then(|s| s.parse().ok()),
                })
            })
            .context("查询启用仓库失败")?
            .collect::<Result<Vec<_>, _>>()
            .context("收集启用仓库结果失败")?;

        Ok(repos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS template_repos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                owner TEXT NOT NULL,
                name TEXT NOT NULL,
                branch TEXT NOT NULL DEFAULT 'main',
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(owner, name)
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_add_and_list_repos() {
        let conn = setup_db();
        let service = TemplateService::new().unwrap();

        // 添加仓库
        let repo = TemplateRepo::new(
            "yovinchen".to_string(),
            "claude-code-templates".to_string(),
            "main".to_string(),
        );
        let id = service.add_repo(&conn, repo).unwrap();
        assert!(id > 0);

        // 列出仓库
        let repos = service.list_repos(&conn).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].owner, "yovinchen");
        assert_eq!(repos[0].name, "claude-code-templates");
    }

    #[test]
    fn test_toggle_repo_enabled() {
        let conn = setup_db();
        let service = TemplateService::new().unwrap();

        // 添加仓库
        let repo = TemplateRepo::new("test".to_string(), "repo".to_string(), "main".to_string());
        let id = service.add_repo(&conn, repo).unwrap();

        // 切换状态
        let enabled = service.toggle_repo_enabled(&conn, id).unwrap();
        assert!(!enabled);

        let enabled = service.toggle_repo_enabled(&conn, id).unwrap();
        assert!(enabled);
    }

    #[test]
    fn test_remove_repo() {
        let conn = setup_db();
        let service = TemplateService::new().unwrap();

        // 添加仓库
        let repo = TemplateRepo::new("test".to_string(), "repo".to_string(), "main".to_string());
        let id = service.add_repo(&conn, repo).unwrap();

        // 删除仓库
        service.remove_repo(&conn, id).unwrap();

        // 验证已删除
        let repos = service.list_repos(&conn).unwrap();
        assert_eq!(repos.len(), 0);
    }
}
