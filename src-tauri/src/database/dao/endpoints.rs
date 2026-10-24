//! 全局端点 DAO（endpoints 表）
//!
//! 厂商连接信息的单一事实源：分组条目经 `providers.endpoint_ref` 引用。
//! 设计见 docs/design-endpoint-refactor-zh.md。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Endpoint;
use indexmap::IndexMap;
use rusqlite::OptionalExtension;

impl Database {
    /// 全部端点（按 sort_index, created_at, id 稳定排序）
    pub fn get_all_endpoints(&self) -> Result<IndexMap<String, Endpoint>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, category, base_url, api_key, auth_binding,
                        website_url, icon, icon_color, notes, meta, created_at, sort_index
                 FROM endpoints
                 ORDER BY COALESCE(sort_index, 999999), COALESCE(created_at, 0), id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], row_to_endpoint)
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut result = IndexMap::new();
        for endpoint in rows {
            result.insert(endpoint.id.clone(), endpoint);
        }
        Ok(result)
    }

    /// 单个端点
    pub fn get_endpoint(&self, id: &str) -> Result<Option<Endpoint>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, name, category, base_url, api_key, auth_binding,
                    website_url, icon, icon_color, notes, meta, created_at, sort_index
             FROM endpoints WHERE id = ?1",
            [id],
            row_to_endpoint,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 按归一化连接 (base_url 去尾 /, api_key) 精确查端点（归并/查重用）
    pub fn find_endpoint_by_connection(
        &self,
        base_url: &str,
        api_key: &str,
    ) -> Result<Option<Endpoint>, AppError> {
        let conn = lock_conn!(self.conn);
        conn.query_row(
            "SELECT id, name, category, base_url, api_key, auth_binding,
                    website_url, icon, icon_color, notes, meta, created_at, sort_index
             FROM endpoints
             WHERE rtrim(base_url, '/') = rtrim(?1, '/') AND api_key = ?2",
            [base_url, api_key],
            row_to_endpoint,
        )
        .optional()
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 保存端点（新增或整体更新）
    pub fn save_endpoint(&self, endpoint: &Endpoint) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let auth_binding = endpoint
            .auth_binding
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let meta = endpoint
            .meta
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO endpoints
                (id, name, category, base_url, api_key, auth_binding,
                 website_url, icon, icon_color, notes, meta, created_at, sort_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                category = excluded.category,
                base_url = excluded.base_url,
                api_key = excluded.api_key,
                auth_binding = excluded.auth_binding,
                website_url = excluded.website_url,
                icon = excluded.icon,
                icon_color = excluded.icon_color,
                notes = excluded.notes,
                meta = excluded.meta,
                created_at = excluded.created_at,
                sort_index = excluded.sort_index",
            rusqlite::params![
                endpoint.id,
                endpoint.name,
                endpoint.category,
                endpoint.base_url.trim_end_matches('/'),
                endpoint.api_key,
                auth_binding,
                endpoint.website_url,
                endpoint.icon,
                endpoint.icon_color,
                endpoint.notes,
                meta,
                endpoint.created_at,
                endpoint.sort_index,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除端点。被分组条目引用时拒绝（返回引用计数），调用方提示改用停用。
    pub fn delete_endpoint(&self, id: &str) -> Result<Result<(), usize>, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let refs: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE endpoint_ref = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if refs > 0 {
            return Ok(Err(refs as usize));
        }
        tx.execute("DELETE FROM endpoints WHERE id = ?1", [id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(Ok(()))
    }

    /// 引用某端点的分组条目 id 列表（`(app_type, provider_id)`）
    pub fn endpoint_referencing_providers(
        &self,
        endpoint_id: &str,
    ) -> Result<Vec<(String, String)>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT app_type, id FROM providers WHERE endpoint_ref = ?1 ORDER BY app_type, id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([endpoint_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rows)
    }
}

fn row_to_endpoint(row: &rusqlite::Row<'_>) -> rusqlite::Result<Endpoint> {
    let auth_binding_str: Option<String> = row.get(5)?;
    let meta_str: Option<String> = row.get(10)?;
    Ok(Endpoint {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        base_url: row.get(3)?,
        api_key: row.get(4)?,
        auth_binding: auth_binding_str
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(&s).ok()),
        website_url: row.get(6)?,
        icon: row.get(7)?,
        icon_color: row.get(8)?,
        notes: row.get(9)?,
        meta: meta_str
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(&s).ok()),
        created_at: row.get(11)?,
        sort_index: row.get(12)?,
    })
}
