//! Session 路由数据访问层
//!
//! 管理 session_routes 和 session_routing_config 表的 CRUD 操作

use crate::database::lock_conn;
use crate::database::Database;
use crate::error::AppError;
use crate::proxy::session_router::{RoutingStrategy, SessionRouteInfo, SessionRoutingConfig};
use std::collections::HashMap;

impl Database {
    // ==================== Session Routes ====================

    /// 获取 session 的路由信息
    pub fn get_session_route(
        &self,
        session_id: &str,
        app_type: &str,
    ) -> Result<Option<SessionRouteInfo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT sr.session_id, sr.app_type, sr.provider_id, p.name,
                        sr.assigned_at, sr.last_used_at, sr.request_count, sr.failover_count
                 FROM session_routes sr
                 LEFT JOIN providers p ON p.id = sr.provider_id AND p.app_type = sr.app_type
                 WHERE sr.session_id = ?1 AND sr.app_type = ?2",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([session_id, app_type], |row| {
            Ok(SessionRouteInfo {
                session_id: row.get(0)?,
                session_name: String::new(),
                app_type: row.get(1)?,
                provider_id: row.get(2)?,
                provider_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                assigned_at: row.get(4)?,
                last_used_at: row.get(5)?,
                request_count: row.get::<_, i64>(6)? as u64,
                failover_count: row.get::<_, i64>(7)? as u64,
            })
        });

        match result {
            Ok(route) => Ok(Some(route)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 插入新 session 路由
    pub fn insert_session_route(
        &self,
        session_id: &str,
        app_type: &str,
        provider_id: &str,
        now: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO session_routes (session_id, app_type, provider_id, assigned_at, last_used_at, request_count, failover_count)
             VALUES (?1, ?2, ?3, ?4, ?4, 1, 0)",
            rusqlite::params![session_id, app_type, provider_id, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 更新 session 的最后使用时间和请求计数
    pub fn touch_session_route(&self, session_id: &str, app_type: &str) -> Result<(), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE session_routes SET last_used_at = ?1, request_count = request_count + 1
             WHERE session_id = ?2 AND app_type = ?3",
            rusqlite::params![now, session_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 更新 session 的 provider（故障转移时）
    pub fn update_session_route_provider(
        &self,
        session_id: &str,
        app_type: &str,
        new_provider_id: &str,
        now: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE session_routes SET provider_id = ?1, last_used_at = ?2
             WHERE session_id = ?3 AND app_type = ?4",
            rusqlite::params![new_provider_id, now, session_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 递增 session 的故障转移计数
    pub fn increment_session_failover(
        &self,
        session_id: &str,
        app_type: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE session_routes SET failover_count = failover_count + 1
             WHERE session_id = ?1 AND app_type = ?2",
            rusqlite::params![session_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 session 路由
    pub fn delete_session_route(&self, session_id: &str, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM session_routes WHERE session_id = ?1 AND app_type = ?2",
            rusqlite::params![session_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 获取所有活跃 session 路由（用于 UI 展示）
    pub fn get_all_session_routes(
        &self,
        app_type: &str,
    ) -> Result<Vec<SessionRouteInfo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT sr.session_id, sr.app_type, sr.provider_id, COALESCE(p.name, '(deleted)'),
                        sr.assigned_at, sr.last_used_at, sr.request_count, sr.failover_count
                 FROM session_routes sr
                 LEFT JOIN providers p ON p.id = sr.provider_id AND p.app_type = sr.app_type
                 WHERE sr.app_type = ?1
                 ORDER BY sr.last_used_at DESC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let routes = stmt
            .query_map([app_type], |row| {
                Ok(SessionRouteInfo {
                    session_id: row.get(0)?,
                    session_name: String::new(),
                    app_type: row.get(1)?,
                    provider_id: row.get(2)?,
                    provider_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    assigned_at: row.get(4)?,
                    last_used_at: row.get(5)?,
                    request_count: row.get::<_, i64>(6)? as u64,
                    failover_count: row.get::<_, i64>(7)? as u64,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(routes)
    }

    /// 统计每个 provider 的活跃 session 数
    pub fn count_sessions_per_provider(
        &self,
        app_type: &str,
    ) -> Result<HashMap<String, u64>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, COUNT(*) as cnt
                 FROM session_routes
                 WHERE app_type = ?1
                 GROUP BY provider_id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let counts = stmt
            .query_map([app_type], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(counts)
    }

    /// 删除指定应用下的过期 session 路由
    pub fn delete_expired_session_routes(
        &self,
        app_type: &str,
        cutoff_ms: i64,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let count = conn
            .execute(
                "DELETE FROM session_routes WHERE app_type = ?1 AND last_used_at < ?2",
                rusqlite::params![app_type, cutoff_ms],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count as u64)
    }

    // ==================== Session Routing Config ====================

    /// 获取 session 路由配置
    pub fn get_session_routing_config(
        &self,
        app_type: &str,
    ) -> Result<Option<SessionRoutingConfig>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT enabled, strategy, session_ttl_seconds, max_sessions_per_provider
                 FROM session_routing_config WHERE app_type = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = stmt.query_row([app_type], |row| {
            Ok(SessionRoutingConfig {
                enabled: row.get::<_, i32>(0)? != 0,
                strategy: RoutingStrategy::from_str(&row.get::<_, String>(1)?),
                session_ttl_seconds: row.get::<_, i64>(2)? as u64,
                max_sessions_per_provider: row.get::<_, i32>(3)? as u32,
            })
        });

        match result {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新 session 路由配置
    pub fn update_session_routing_config(
        &self,
        app_type: &str,
        config: &SessionRoutingConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO session_routing_config (app_type, enabled, strategy, session_ttl_seconds, max_sessions_per_provider)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                app_type,
                config.enabled as i32,
                config.strategy.as_str(),
                config.session_ttl_seconds as i64,
                config.max_sessions_per_provider as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
