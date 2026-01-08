//! 代理功能数据访问层
//!
//! 处理代理配置、Provider健康状态和使用统计的数据库操作

use crate::error::AppError;
use crate::proxy::types::*;

use super::super::{lock_conn, Database};

impl Database {
    // ==================== Global Proxy Config ====================

    /// 获取全局代理配置（统一字段）
    ///
    /// 从 claude 行读取（三行镜像一致）
    pub async fn get_global_proxy_config(&self) -> Result<GlobalProxyConfig, AppError> {
        // 使用 block 限制 conn 的作用域，避免跨 await 持有锁
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT proxy_enabled, listen_address, listen_port, enable_logging
                 FROM proxy_config WHERE app_type = 'claude'",
                [],
                |row| {
                    Ok(GlobalProxyConfig {
                        proxy_enabled: row.get::<_, i32>(0)? != 0,
                        listen_address: row.get(1)?,
                        listen_port: row.get::<_, i32>(2)? as u16,
                        enable_logging: row.get::<_, i32>(3)? != 0,
                    })
                },
            )
        };
        // conn 已在 block 结束时释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，创建默认配置
                self.init_proxy_config_rows().await?;
                Ok(GlobalProxyConfig {
                    proxy_enabled: false,
                    listen_address: "127.0.0.1".to_string(),
                    listen_port: 15721,
                    enable_logging: true,
                })
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新全局代理配置（镜像写三行）
    pub async fn update_global_proxy_config(
        &self,
        config: GlobalProxyConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE proxy_config SET
                proxy_enabled = ?1,
                listen_address = ?2,
                listen_port = ?3,
                enable_logging = ?4,
                updated_at = datetime('now')",
            rusqlite::params![
                if config.proxy_enabled { 1 } else { 0 },
                config.listen_address,
                config.listen_port as i32,
                if config.enable_logging { 1 } else { 0 },
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 获取应用级代理配置
    pub async fn get_proxy_config_for_app(
        &self,
        app_type: &str,
    ) -> Result<AppProxyConfig, AppError> {
        // 使用 block 限制 conn 的作用域，避免跨 await 持有锁
        let app_type_owned = app_type.to_string();
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT app_type, enabled, auto_failover_enabled,
                        max_retries, streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                        circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                        circuit_error_rate_threshold, circuit_min_requests
                 FROM proxy_config WHERE app_type = ?1",
                [app_type],
                |row| {
                    Ok(AppProxyConfig {
                        app_type: row.get(0)?,
                        enabled: row.get::<_, i32>(1)? != 0,
                        auto_failover_enabled: row.get::<_, i32>(2)? != 0,
                        max_retries: row.get::<_, i32>(3)? as u32,
                        streaming_first_byte_timeout: row.get::<_, i32>(4)? as u32,
                        streaming_idle_timeout: row.get::<_, i32>(5)? as u32,
                        non_streaming_timeout: row.get::<_, i32>(6)? as u32,
                        circuit_failure_threshold: row.get::<_, i32>(7)? as u32,
                        circuit_success_threshold: row.get::<_, i32>(8)? as u32,
                        circuit_timeout_seconds: row.get::<_, i32>(9)? as u32,
                        circuit_error_rate_threshold: row.get(10)?,
                        circuit_min_requests: row.get::<_, i32>(11)? as u32,
                    })
                },
            )
        };
        // conn 已在 block 结束时释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，创建默认配置
                self.init_proxy_config_rows().await?;
                Ok(AppProxyConfig {
                    app_type: app_type_owned,
                    enabled: false,
                    auto_failover_enabled: false,
                    max_retries: 3,
                    streaming_first_byte_timeout: 60,
                    streaming_idle_timeout: 120,
                    non_streaming_timeout: 600,
                    circuit_failure_threshold: 4,
                    circuit_success_threshold: 2,
                    circuit_timeout_seconds: 60,
                    circuit_error_rate_threshold: 0.6,
                    circuit_min_requests: 10,
                })
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新应用级代理配置
    pub async fn update_proxy_config_for_app(
        &self,
        config: AppProxyConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE proxy_config SET
                enabled = ?2,
                auto_failover_enabled = ?3,
                max_retries = ?4,
                streaming_first_byte_timeout = ?5,
                streaming_idle_timeout = ?6,
                non_streaming_timeout = ?7,
                circuit_failure_threshold = ?8,
                circuit_success_threshold = ?9,
                circuit_timeout_seconds = ?10,
                circuit_error_rate_threshold = ?11,
                circuit_min_requests = ?12,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            rusqlite::params![
                config.app_type,
                if config.enabled { 1 } else { 0 },
                if config.auto_failover_enabled { 1 } else { 0 },
                config.max_retries as i32,
                config.streaming_first_byte_timeout as i32,
                config.streaming_idle_timeout as i32,
                config.non_streaming_timeout as i32,
                config.circuit_failure_threshold as i32,
                config.circuit_success_threshold as i32,
                config.circuit_timeout_seconds as i32,
                config.circuit_error_rate_threshold,
                config.circuit_min_requests as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 初始化 proxy_config 表的三行数据
    async fn init_proxy_config_rows(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        for app_type in &["claude", "codex", "gemini"] {
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type) VALUES (?1)",
                [app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    // ==================== Legacy Proxy Config (兼容旧代码) ====================

    /// 获取代理配置（兼容旧接口，返回 claude 行的配置）
    pub async fn get_proxy_config(&self) -> Result<ProxyConfig, AppError> {
        // 使用 block 限制 conn 的作用域，避免跨 await 持有锁
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT listen_address, listen_port, max_retries,
                        enable_logging,
                        streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout
                 FROM proxy_config WHERE app_type = 'claude'",
                [],
                |row| {
                    Ok(ProxyConfig {
                        listen_address: row.get(0)?,
                        listen_port: row.get::<_, i32>(1)? as u16,
                        max_retries: row.get::<_, i32>(2)? as u8,
                        request_timeout: 600, // 废弃字段，返回默认值
                        enable_logging: row.get::<_, i32>(3)? != 0,
                        live_takeover_active: false, // 废弃字段
                        streaming_first_byte_timeout: row.get::<_, i32>(4).unwrap_or(60) as u64,
                        streaming_idle_timeout: row.get::<_, i32>(5).unwrap_or(120) as u64,
                        non_streaming_timeout: row.get::<_, i32>(6).unwrap_or(600) as u64,
                    })
                },
            )
        };
        // conn 已在 block 结束时释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，初始化默认配置
                self.init_proxy_config_rows().await?;
                Ok(ProxyConfig::default())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新代理配置（兼容旧接口，更新所有三行的公共字段）
    pub async fn update_proxy_config(&self, config: ProxyConfig) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 更新所有三行的公共字段
        conn.execute(
            "UPDATE proxy_config SET
                listen_address = ?1,
                listen_port = ?2,
                max_retries = ?3,
                enable_logging = ?4,
                streaming_first_byte_timeout = ?5,
                streaming_idle_timeout = ?6,
                non_streaming_timeout = ?7,
                updated_at = datetime('now')",
            rusqlite::params![
                config.listen_address,
                config.listen_port as i32,
                config.max_retries as i32,
                if config.enable_logging { 1 } else { 0 },
                config.streaming_first_byte_timeout as i32,
                config.streaming_idle_timeout as i32,
                config.non_streaming_timeout as i32,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 设置 Live 接管状态（兼容旧版本，更新 enabled 字段）
    pub async fn set_live_takeover_active(&self, _active: bool) -> Result<(), AppError> {
        // 不再使用此字段，由 enabled 字段替代
        // 保留空实现以兼容旧代码
        Ok(())
    }

    /// 检查是否处于 Live 接管模式
    ///
    /// 检查是否有任一 app 的 enabled = true
    pub async fn is_live_takeover_active(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_config WHERE enabled = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    // ==================== Provider Health ====================

    /// 获取Provider健康状态
    pub async fn get_provider_health(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderHealth, AppError> {
        let result = {
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
        };

        match result {
            Ok(health) => Ok(health),
            // 缺少记录时视为健康（关闭后清空状态，再次打开时默认正常）
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ProviderHealth {
                provider_id: provider_id.to_string(),
                app_type: app_type.to_string(),
                is_healthy: true,
                consecutive_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                updated_at: chrono::Utc::now().to_rfc3339(),
            }),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
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

    /// 清空指定应用的健康状态（关闭单个代理时使用）
    pub async fn clear_provider_health_for_app(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM provider_health WHERE app_type = ?1",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::debug!("Cleared provider health records for app {app_type}");
        Ok(())
    }

    /// 清空所有Provider健康状态（代理停止时调用）
    pub async fn clear_all_provider_health(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute("DELETE FROM provider_health", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        log::debug!("Cleared all provider health records");
        Ok(())
    }

    // ==================== Circuit Breaker Config (Legacy Compatibility) ====================

    /// 获取熔断器配置（兼容旧接口，从 claude 行读取）
    ///
    /// 熔断器配置已合并到 proxy_config 表，每 app 独立
    /// 此方法保留用于兼容旧代码，建议使用 get_proxy_config_for_app
    pub async fn get_circuit_breaker_config(
        &self,
    ) -> Result<crate::proxy::circuit_breaker::CircuitBreakerConfig, AppError> {
        // 使用 block 限制 conn 的作用域，避免跨 await 持有锁
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                        circuit_error_rate_threshold, circuit_min_requests
                 FROM proxy_config WHERE app_type = 'claude'",
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
        };
        // conn 已在 block 结束时释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，初始化默认配置
                self.init_proxy_config_rows().await?;
                Ok(crate::proxy::circuit_breaker::CircuitBreakerConfig::default())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 更新熔断器配置（兼容旧接口，更新所有三行）
    ///
    /// 熔断器配置已合并到 proxy_config 表
    /// 此方法保留用于兼容旧代码，建议使用 update_proxy_config_for_app
    pub async fn update_circuit_breaker_config(
        &self,
        config: &crate::proxy::circuit_breaker::CircuitBreakerConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 更新所有三行的熔断器配置
        conn.execute(
            "UPDATE proxy_config SET
                circuit_failure_threshold = ?1,
                circuit_success_threshold = ?2,
                circuit_timeout_seconds = ?3,
                circuit_error_rate_threshold = ?4,
                circuit_min_requests = ?5,
                updated_at = datetime('now')",
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

    /// 检查是否存在任意 Live 配置备份
    pub async fn has_any_live_backup(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_live_backup", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count > 0)
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
