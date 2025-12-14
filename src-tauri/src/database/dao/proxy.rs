//! 代理功能数据访问层
//!
//! 处理代理配置、Provider健康状态和使用统计的数据库操作

use crate::error::AppError;
use crate::proxy::types::*;

use super::super::{lock_conn, Database};

impl Database {
    // ==================== Proxy Config ====================

    /// 获取代理配置
    pub async fn get_proxy_config(&self) -> Result<ProxyConfig, AppError> {
        // 在一个作用域内获取锁并查询，确保锁在await之前释放
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT enabled, listen_address, listen_port, max_retries,
                        request_timeout, enable_logging, live_takeover_active
                 FROM proxy_config WHERE id = 1",
                [],
                |row| {
                    Ok(ProxyConfig {
                        enabled: row.get::<_, i32>(0)? != 0,
                        listen_address: row.get(1)?,
                        listen_port: row.get::<_, i32>(2)? as u16,
                        max_retries: row.get::<_, i32>(3)? as u8,
                        request_timeout: row.get::<_, i32>(4)? as u64,
                        enable_logging: row.get::<_, i32>(5)? != 0,
                        live_takeover_active: row.get::<_, i32>(6).unwrap_or(0) != 0,
                    })
                },
            )
        }; // conn锁在这里释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，插入默认配置
                let default_config = ProxyConfig::default();
                self.update_proxy_config(default_config.clone()).await?;
                Ok(default_config)
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新代理配置
    pub async fn update_proxy_config(&self, config: ProxyConfig) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "INSERT OR REPLACE INTO proxy_config
             (id, enabled, listen_address, listen_port, max_retries, request_timeout, enable_logging, live_takeover_active, target_app, created_at, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                     COALESCE((SELECT created_at FROM proxy_config WHERE id = 1), datetime('now')),
                     datetime('now'))",
            rusqlite::params![
                if config.enabled { 1 } else { 0 },
                config.listen_address,
                config.listen_port as i32,
                config.max_retries as i32,
                config.request_timeout as i32,
                if config.enable_logging { 1 } else { 0 },
                if config.live_takeover_active { 1 } else { 0 },
                "claude", // 兼容旧字段，写入默认值
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 设置 Live 接管状态
    pub async fn set_live_takeover_active(&self, active: bool) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET live_takeover_active = ?1, updated_at = datetime('now') WHERE id = 1",
            rusqlite::params![if active { 1 } else { 0 }],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_live_takeover_active(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let active: i32 = conn
            .query_row(
                "SELECT COALESCE(live_takeover_active, 0) FROM proxy_config WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(active != 0)
    }

    // ==================== Provider Health ====================

    /// 获取Provider健康状态
    pub async fn get_provider_health(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderHealth, AppError> {
        let conn = lock_conn!(self.conn);

        conn.query_row(
            "SELECT provider_id, app_type, is_healthy, consecutive_failures,
                    last_success_at, last_failure_at, last_error, updated_at
             FROM provider_health
             WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
            |row| {
                Ok(ProviderHealth {
                    provider_id: row.get(0)?,
                    app_type: row.get(1)?,
                    is_healthy: row.get::<_, i64>(2)? != 0,
                    consecutive_failures: row.get::<_, i64>(3)? as u32,
                    last_success_at: row.get(4)?,
                    last_failure_at: row.get(5)?,
                    last_error: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 更新Provider健康状态
    ///
    /// 使用默认阈值（5）判断是否健康，建议使用 `update_provider_health_with_threshold` 传入配置的阈值
    pub async fn update_provider_health(
        &self,
        provider_id: &str,
        app_type: &str,
        success: bool,
        error_msg: Option<String>,
    ) -> Result<(), AppError> {
        // 默认阈值与 CircuitBreakerConfig::default() 保持一致
        self.update_provider_health_with_threshold(provider_id, app_type, success, error_msg, 5)
            .await
    }

    /// 更新Provider健康状态（带阈值参数）
    ///
    /// # Arguments
    /// * `failure_threshold` - 连续失败多少次后标记为不健康
    pub async fn update_provider_health_with_threshold(
        &self,
        provider_id: &str,
        app_type: &str,
        success: bool,
        error_msg: Option<String>,
        failure_threshold: u32,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        let now = chrono::Utc::now().to_rfc3339();

        // 先查询当前状态
        let current = conn.query_row(
            "SELECT consecutive_failures FROM provider_health
             WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
            |row| Ok(row.get::<_, i64>(0)? as u32),
        );

        let (is_healthy, consecutive_failures) = if success {
            // 成功：重置失败计数
            (1, 0)
        } else {
            // 失败：增加失败计数
            let failures = current.unwrap_or(0) + 1;
            // 使用传入的阈值而非硬编码
            let healthy = if failures >= failure_threshold { 0 } else { 1 };
            (healthy, failures)
        };

        let (last_success_at, last_failure_at) = if success {
            (Some(now.clone()), None)
        } else {
            (None, Some(now.clone()))
        };

        // UPSERT
        conn.execute(
            "INSERT OR REPLACE INTO provider_health
             (provider_id, app_type, is_healthy, consecutive_failures,
              last_success_at, last_failure_at, last_error, updated_at)
             VALUES (?1, ?2, ?3, ?4,
                     COALESCE(?5, (SELECT last_success_at FROM provider_health
                                   WHERE provider_id = ?1 AND app_type = ?2)),
                     COALESCE(?6, (SELECT last_failure_at FROM provider_health
                                   WHERE provider_id = ?1 AND app_type = ?2)),
                     ?7, ?8)",
            rusqlite::params![
                provider_id,
                app_type,
                is_healthy,
                consecutive_failures as i64,
                last_success_at,
                last_failure_at,
                error_msg,
                &now,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 重置Provider健康状态
    pub async fn reset_provider_health(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM provider_health WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::debug!("Reset health status for provider {provider_id} (app: {app_type})");

        Ok(())
    }

    // ==================== Circuit Breaker Config ====================

    /// 获取熔断器配置
    pub async fn get_circuit_breaker_config(
        &self,
    ) -> Result<crate::proxy::circuit_breaker::CircuitBreakerConfig, AppError> {
        let conn = lock_conn!(self.conn);

        let config = conn
            .query_row(
                "SELECT failure_threshold, success_threshold, timeout_seconds,
                        error_rate_threshold, min_requests
                 FROM circuit_breaker_config WHERE id = 1",
                [],
                |row| {
                    Ok(crate::proxy::circuit_breaker::CircuitBreakerConfig {
                        failure_threshold: row.get::<_, i32>(0)? as u32,
                        success_threshold: row.get::<_, i32>(1)? as u32,
                        timeout_seconds: row.get::<_, i64>(2)? as u64,
                        error_rate_threshold: row.get(3)?,
                        min_requests: row.get::<_, i32>(4)? as u32,
                    })
                },
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(config)
    }

    /// 更新熔断器配置
    pub async fn update_circuit_breaker_config(
        &self,
        config: &crate::proxy::circuit_breaker::CircuitBreakerConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE circuit_breaker_config
             SET failure_threshold = ?1,
                 success_threshold = ?2,
                 timeout_seconds = ?3,
                 error_rate_threshold = ?4,
                 min_requests = ?5,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = 1",
            rusqlite::params![
                config.failure_threshold as i32,
                config.success_threshold as i32,
                config.timeout_seconds as i64,
                config.error_rate_threshold,
                config.min_requests as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    // ==================== Live Backup ====================

    /// 保存 Live 配置备份
    pub async fn save_live_backup(
        &self,
        app_type: &str,
        config_json: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO proxy_live_backup (app_type, original_config, backed_up_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![app_type, config_json, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已备份 {app_type} Live 配置");
        Ok(())
    }

    /// 获取 Live 配置备份
    pub async fn get_live_backup(&self, app_type: &str) -> Result<Option<LiveBackup>, AppError> {
        let conn = lock_conn!(self.conn);

        let result = conn.query_row(
            "SELECT app_type, original_config, backed_up_at FROM proxy_live_backup WHERE app_type = ?1",
            rusqlite::params![app_type],
            |row| {
                Ok(LiveBackup {
                    app_type: row.get(0)?,
                    original_config: row.get(1)?,
                    backed_up_at: row.get(2)?,
                })
            },
        );

        match result {
            Ok(backup) => Ok(Some(backup)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 删除 Live 配置备份
    pub async fn delete_live_backup(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM proxy_live_backup WHERE app_type = ?1",
            rusqlite::params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已删除 {app_type} Live 配置备份");
        Ok(())
    }

    /// 删除所有 Live 配置备份
    pub async fn delete_all_live_backups(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute("DELETE FROM proxy_live_backup", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已删除所有 Live 配置备份");
        Ok(())
    }
}
