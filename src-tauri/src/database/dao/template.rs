//! Template 数据访问对象
//!
//! 提供 Template Repos、Template Components 和 Installed Components 的 CRUD 操作。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::template::{
    ComponentType, InstalledComponent, TemplateComponent, TemplateRepo,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};

impl Database {
    // ==================== TemplateRepo 相关 ====================

    /// 插入模板仓库
    pub fn insert_repo(&self, repo: &TemplateRepo) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO template_repos (owner, name, branch, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![repo.owner, repo.name, repo.branch, repo.enabled, now, now],
        )
        .map_err(|e| AppError::Database(format!("插入模板仓库失败: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    /// 获取单个模板仓库
    pub fn get_repo(&self, id: i64) -> Result<Option<TemplateRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, owner, name, branch, enabled, created_at, updated_at
                 FROM template_repos
                 WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(format!("准备查询模板仓库失败: {e}")))?;

        let repo = stmt
            .query_row(params![id], |row| {
                Ok(TemplateRepo {
                    id: Some(row.get(0)?),
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    updated_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })
            .optional()
            .map_err(|e| AppError::Database(format!("查询模板仓库失败: {e}")))?;

        Ok(repo)
    }

    /// 获取所有模板仓库
    pub fn list_repos(&self) -> Result<Vec<TemplateRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, owner, name, branch, enabled, created_at, updated_at
                 FROM template_repos
                 ORDER BY created_at DESC",
            )
            .map_err(|e| AppError::Database(format!("准备查询模板仓库列表失败: {e}")))?;

        let repo_iter = stmt
            .query_map([], |row| {
                Ok(TemplateRepo {
                    id: Some(row.get(0)?),
                    owner: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    enabled: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    updated_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })
            .map_err(|e| AppError::Database(format!("查询模板仓库列表失败: {e}")))?;

        let mut repos = Vec::new();
        for repo_res in repo_iter {
            repos.push(repo_res.map_err(|e| AppError::Database(format!("解析模板仓库失败: {e}")))?);
        }

        Ok(repos)
    }

    /// 更新模板仓库
    pub fn update_repo(&self, repo: &TemplateRepo) -> Result<(), AppError> {
        let repo_id = repo
            .id
            .ok_or_else(|| AppError::Database("仓库 ID 不能为空".to_string()))?;
        let conn = lock_conn!(self.conn);
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE template_repos
             SET owner = ?1, name = ?2, branch = ?3, enabled = ?4, updated_at = ?5
             WHERE id = ?6",
            params![
                repo.owner,
                repo.name,
                repo.branch,
                repo.enabled,
                now,
                repo_id
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模板仓库失败: {e}")))?;

        Ok(())
    }

    /// 删除模板仓库
    pub fn delete_repo(&self, id: i64) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute("DELETE FROM template_repos WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(format!("删除模板仓库失败: {e}")))?;

        Ok(())
    }

    /// 切换仓库启用状态
    pub fn toggle_repo_enabled(&self, id: i64, enabled: bool) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE template_repos SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![enabled, now, id],
        )
        .map_err(|e| AppError::Database(format!("切换仓库启用状态失败: {e}")))?;

        Ok(())
    }

    // ==================== TemplateComponent 相关 ====================

    /// 插入模板组件
    pub fn insert_component(&self, component: &TemplateComponent) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO template_components
             (repo_id, component_type, category, name, path, description, content_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                component.repo_id,
                component.component_type.as_str(),
                component.category,
                component.name,
                component.path,
                component.description,
                component.content_hash,
                now,
                now
            ],
        )
        .map_err(|e| AppError::Database(format!("插入模板组件失败: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    /// 获取单个模板组件
    pub fn get_component(&self, id: i64) -> Result<Option<TemplateComponent>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, repo_id, component_type, category, name, path, description, content_hash
                 FROM template_components
                 WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(format!("准备查询模板组件失败: {e}")))?;

        let component = stmt
            .query_row(params![id], |row| {
                let component_type_str: String = row.get(2)?;
                let component_type = ComponentType::from_str(&component_type_str)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                Ok(TemplateComponent {
                    id: Some(row.get(0)?),
                    repo_id: row.get(1)?,
                    component_type,
                    category: row.get(3)?,
                    name: row.get(4)?,
                    path: row.get(5)?,
                    description: row.get(6)?,
                    content_hash: row.get(7)?,
                    installed: false, // 需要单独查询
                })
            })
            .optional()
            .map_err(|e| AppError::Database(format!("查询模板组件失败: {e}")))?;

        Ok(component)
    }

    /// 获取组件列表（支持过滤和分页）
    pub fn list_components(
        &self,
        component_type: Option<&str>,
        category: Option<&str>,
        search: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> Result<(Vec<TemplateComponent>, u32), AppError> {
        let conn = lock_conn!(self.conn);

        // 构建 WHERE 子句
        let mut where_clauses = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ct) = component_type {
            where_clauses.push("component_type = ?");
            params_vec.push(Box::new(ct.to_string()));
        }

        if let Some(cat) = category {
            where_clauses.push("category = ?");
            params_vec.push(Box::new(cat.to_string()));
        }

        if let Some(s) = search {
            where_clauses.push("(name LIKE ? OR description LIKE ?)");
            let pattern = format!("%{s}%");
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        // 查询总数
        let count_sql = format!("SELECT COUNT(*) FROM template_components {where_sql}");

        let total: u32 = {
            let mut stmt = conn
                .prepare(&count_sql)
                .map_err(|e| AppError::Database(format!("准备统计组件数量失败: {e}")))?;

            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();

            stmt.query_row(&params_refs[..], |row| row.get(0))
                .map_err(|e| AppError::Database(format!("统计组件数量失败: {e}")))?
        };

        // 查询数据
        let offset = (page.saturating_sub(1)) * page_size;
        let query_sql = format!(
            "SELECT id, repo_id, component_type, category, name, path, description, content_hash
             FROM template_components
             {where_sql}
             ORDER BY name ASC
             LIMIT ? OFFSET ?"
        );

        let mut stmt = conn
            .prepare(&query_sql)
            .map_err(|e| AppError::Database(format!("准备查询组件列表失败: {e}")))?;

        params_vec.push(Box::new(page_size));
        params_vec.push(Box::new(offset));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let component_iter = stmt
            .query_map(&params_refs[..], |row| {
                let component_type_str: String = row.get(2)?;
                let component_type = ComponentType::from_str(&component_type_str)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                Ok(TemplateComponent {
                    id: Some(row.get(0)?),
                    repo_id: row.get(1)?,
                    component_type,
                    category: row.get(3)?,
                    name: row.get(4)?,
                    path: row.get(5)?,
                    description: row.get(6)?,
                    content_hash: row.get(7)?,
                    installed: false, // 需要单独查询
                })
            })
            .map_err(|e| AppError::Database(format!("查询组件列表失败: {e}")))?;

        let mut components = Vec::new();
        for component_res in component_iter {
            components
                .push(component_res.map_err(|e| AppError::Database(format!("解析组件失败: {e}")))?);
        }

        Ok((components, total))
    }

    /// 删除仓库的所有组件
    pub fn delete_components_by_repo(&self, repo_id: i64) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM template_components WHERE repo_id = ?1",
            params![repo_id],
        )
        .map_err(|e| AppError::Database(format!("删除仓库组件失败: {e}")))?;

        Ok(())
    }

    /// Upsert 模板组件（根据 repo_id + component_type + path 判断是否已存在）
    pub fn upsert_component(&self, component: &TemplateComponent) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO template_components
             (repo_id, component_type, category, name, path, description, content_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(repo_id, component_type, path) DO UPDATE SET
                 category = excluded.category,
                 name = excluded.name,
                 description = excluded.description,
                 content_hash = excluded.content_hash,
                 updated_at = excluded.updated_at",
            params![
                component.repo_id,
                component.component_type.as_str(),
                component.category,
                component.name,
                component.path,
                component.description,
                component.content_hash,
                now,
                now
            ],
        )
        .map_err(|e| AppError::Database(format!("Upsert 模板组件失败: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    // ==================== InstalledComponent 相关 ====================

    /// 插入已安装组件
    pub fn insert_installed(&self, installed: &InstalledComponent) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        let installed_at = installed.installed_at.to_rfc3339();

        conn.execute(
            "INSERT INTO installed_components
             (component_id, component_type, name, path, app_type, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                installed.component_id,
                installed.component_type.as_str(),
                installed.name,
                installed.path,
                installed.app_type,
                installed_at
            ],
        )
        .map_err(|e| AppError::Database(format!("插入已安装组件失败: {e}")))?;

        Ok(conn.last_insert_rowid())
    }

    /// 删除已安装组件
    pub fn delete_installed(
        &self,
        component_type: &str,
        path: &str,
        app_type: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM installed_components
             WHERE component_type = ?1 AND path = ?2 AND app_type = ?3",
            params![component_type, path, app_type],
        )
        .map_err(|e| AppError::Database(format!("删除已安装组件失败: {e}")))?;

        Ok(())
    }

    /// 获取已安装组件列表
    pub fn list_installed(
        &self,
        app_type: Option<&str>,
    ) -> Result<Vec<InstalledComponent>, AppError> {
        let conn = lock_conn!(self.conn);

        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(at) = app_type
        {
            (
                "SELECT id, component_id, component_type, name, path, app_type, installed_at
                 FROM installed_components
                 WHERE app_type = ?
                 ORDER BY installed_at DESC"
                    .to_string(),
                vec![Box::new(at.to_string())],
            )
        } else {
            (
                "SELECT id, component_id, component_type, name, path, app_type, installed_at
                 FROM installed_components
                 ORDER BY installed_at DESC"
                    .to_string(),
                vec![],
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("准备查询已安装组件失败: {e}")))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let installed_iter = stmt
            .query_map(&params_refs[..], |row| {
                let component_type_str: String = row.get(2)?;
                let component_type = ComponentType::from_str(&component_type_str)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                let installed_at_str: String = row.get(6)?;
                let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;

                Ok(InstalledComponent {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    component_type,
                    name: row.get(3)?,
                    path: row.get(4)?,
                    app_type: row.get(5)?,
                    installed_at,
                })
            })
            .map_err(|e| AppError::Database(format!("查询已安装组件失败: {e}")))?;

        let mut installed = Vec::new();
        for installed_res in installed_iter {
            installed.push(
                installed_res
                    .map_err(|e| AppError::Database(format!("解析已安装组件失败: {e}")))?,
            );
        }

        Ok(installed)
    }

    /// 获取已安装组件列表（支持 app_type 和 component_type 过滤）
    pub fn list_installed_components(
        &self,
        app_type: Option<&str>,
        component_type: Option<&str>,
    ) -> Result<Vec<InstalledComponent>, AppError> {
        let conn = lock_conn!(self.conn);

        // 构建 WHERE 子句
        let mut where_clauses = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(at) = app_type {
            where_clauses.push("app_type = ?");
            params_vec.push(Box::new(at.to_string()));
        }

        if let Some(ct) = component_type {
            where_clauses.push("component_type = ?");
            params_vec.push(Box::new(ct.to_string()));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let sql = format!(
            "SELECT id, component_id, component_type, name, path, app_type, installed_at
             FROM installed_components
             {where_sql}
             ORDER BY installed_at DESC"
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("准备查询已安装组件失败: {e}")))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let installed_iter = stmt
            .query_map(&params_refs[..], |row| {
                let component_type_str: String = row.get(2)?;
                let component_type = ComponentType::from_str(&component_type_str)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                let installed_at_str: String = row.get(6)?;
                let installed_at = DateTime::parse_from_rfc3339(&installed_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;

                Ok(InstalledComponent {
                    id: Some(row.get(0)?),
                    component_id: row.get(1)?,
                    component_type,
                    name: row.get(3)?,
                    path: row.get(4)?,
                    app_type: row.get(5)?,
                    installed_at,
                })
            })
            .map_err(|e| AppError::Database(format!("查询已安装组件失败: {e}")))?;

        let mut installed = Vec::new();
        for installed_res in installed_iter {
            installed.push(
                installed_res
                    .map_err(|e| AppError::Database(format!("解析已安装组件失败: {e}")))?,
            );
        }

        Ok(installed)
    }

    /// 检查组件是否已安装
    pub fn is_installed(
        &self,
        component_type: &str,
        path: &str,
        app_type: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM installed_components
                 WHERE component_type = ?1 AND path = ?2 AND app_type = ?3",
                params![component_type, path, app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(format!("检查组件安装状态失败: {e}")))?;

        Ok(count > 0)
    }

    /// 获取指定应用已安装的组件 ID 列表
    pub fn get_installed_component_ids(&self, app_type: &str) -> Result<Vec<i64>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT component_id FROM installed_components WHERE app_type = ?1 AND component_id IS NOT NULL",
            )
            .map_err(|e| AppError::Database(format!("准备查询已安装组件失败: {e}")))?;

        let ids: Vec<i64> = stmt
            .query_map(params![app_type], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("查询已安装组件失败: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(ids)
    }
}
