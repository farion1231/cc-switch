//! 代理功能数据访问层
//!
//! 处理代理配置、Provider健康状态和使用统计的数据库操作

use crate::app_config::AppType;
use crate::error::AppError;
use crate::proxy::types::*;

use super::super::{lock_conn, lock_recovering, Database};

pub(crate) const PRICING_SOURCE_RESPONSE: &str = "response";
pub(crate) const PRICING_SOURCE_REQUEST: &str = "request";

fn validate_pricing_source(value: &str) -> Result<&str, AppError> {
    let trimmed = value.trim();
    if trimmed == PRICING_SOURCE_RESPONSE || trimmed == PRICING_SOURCE_REQUEST {
        Ok(trimmed)
    } else {
        Err(AppError::localized(
            "error.invalidPricingMode",
            format!("无效计费模式: {value}"),
            format!("Invalid pricing mode: {value}"),
        ))
    }
}

impl Database {
    // ==================== Global Proxy Config ====================

    /// 获取全局代理配置（统一字段）
    ///
    /// 从 claude 行读取（三行镜像一致）
    pub async fn get_global_proxy_config(&self) -> Result<GlobalProxyConfig, AppError> {
        self.run_blocking(move |db| db.get_global_proxy_config_sync())
            .await
    }

    fn get_global_proxy_config_sync(&self) -> Result<GlobalProxyConfig, AppError> {
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
                self.init_proxy_config_rows()?;
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
        self.run_blocking(move |db| db.update_global_proxy_config_sync(config))
            .await
    }

    fn update_global_proxy_config_sync(&self, config: GlobalProxyConfig) -> Result<(), AppError> {
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

    /// 获取计费模式来源
    pub async fn get_pricing_model_source(&self, app_type: &AppType) -> Result<String, AppError> {
        if let Some(value) = self.read_cache.pricing_source.get(app_type.as_str()) {
            return Ok(value);
        }
        let app_type = app_type.clone();
        self.run_blocking(move |db| db.get_pricing_model_source_sync(&app_type))
            .await
    }

    /// Already-blocking usage workers use this directly, avoiding nested worker waits.
    pub(crate) fn get_pricing_model_source_sync(
        &self,
        app_type: &AppType,
    ) -> Result<String, AppError> {
        self.read_cache
            .pricing_source
            .get_or_load(app_type.as_str(), || {
                self.load_pricing_model_source(app_type)
            })
    }

    fn load_pricing_model_source(&self, app_type: &AppType) -> Result<String, AppError> {
        let result = {
            let conn = lock_conn!(self.conn);
            conn.query_row(
                "SELECT pricing_model_source FROM proxy_config WHERE app_type = ?1",
                [app_type],
                |row| row.get(0),
            )
        };

        match result {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.init_proxy_config_rows()?;
                Ok(PRICING_SOURCE_RESPONSE.to_string())
            }
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 设置计费模式来源
    pub async fn set_pricing_model_source(
        &self,
        app_type: &AppType,
        value: &str,
    ) -> Result<(), AppError> {
        let app_type = app_type.clone();
        let value = value.to_owned();
        self.run_blocking(move |db| db.set_pricing_model_source_sync(&app_type, &value))
            .await
    }

    fn set_pricing_model_source_sync(
        &self,
        app_type: &AppType,
        value: &str,
    ) -> Result<(), AppError> {
        let trimmed = validate_pricing_source(value)?;

        // 确保行存在
        self.ensure_proxy_config_row_exists(app_type)?;

        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config SET
                pricing_model_source = ?2,
                updated_at = datetime('now')
             WHERE app_type = ?1",
            rusqlite::params![app_type, trimmed],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 获取应用级代理配置
    pub async fn get_proxy_config_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<AppProxyConfig, AppError> {
        if let Some(value) = self.read_cache.proxy.get(app_type.as_str()) {
            return Ok(value);
        }
        let app_type = app_type.clone();
        self.run_blocking(move |db| {
            db.read_cache.proxy.get_or_load(app_type.as_str(), || {
                db.get_proxy_config_for_app_sync(&app_type)
            })
        })
        .await
    }

    fn get_proxy_config_for_app_sync(
        &self,
        app_type: &AppType,
    ) -> Result<AppProxyConfig, AppError> {
        // 使用 block 限制 conn 的作用域，避免跨 await 持有锁
        let result = {
            let conn = lock_conn!(self.conn);
            let mut stmt = conn.prepare_cached(
                "SELECT app_type, enabled, auto_failover_enabled,
                        max_retries, streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                        circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                        circuit_error_rate_threshold, circuit_min_requests
                 FROM proxy_config WHERE app_type = ?1"
            ).map_err(|e| AppError::Database(e.to_string()))?;
            stmt.query_row([app_type], |row| {
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
            })
        };
        // conn 已在 block 结束时释放

        match result {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // 如果不存在，创建默认配置
                self.init_proxy_config_rows()?;
                Ok(AppProxyConfig {
                    app_type: app_type.to_string(),
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
        self.run_blocking(move |db| db.update_proxy_config_for_app_sync(config))
            .await
    }

    fn update_proxy_config_for_app_sync(&self, config: AppProxyConfig) -> Result<(), AppError> {
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

    /// 确保指定 app_type 的 proxy_config 行存在（同步版本，用于 set_* 函数）
    ///
    /// 使用与 schema.rs seed 相同的 per-app 默认值
    fn ensure_proxy_config_row_exists(&self, app_type: &AppType) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 根据 app_type 使用不同的默认值（与 schema.rs seed 保持一致）
        let (retries, fb_timeout, idle_timeout, cb_fail, cb_succ, cb_timeout, cb_rate, cb_min) =
            match app_type {
                AppType::Claude => (6, 90, 180, 8, 3, 90, 0.7, 15),
                AppType::Codex => (3, 60, 120, 4, 2, 60, 0.6, 10),
                AppType::Gemini => (5, 60, 120, 4, 2, 60, 0.6, 10),
                AppType::GrokBuild => (3, 60, 120, 4, 2, 60, 0.6, 10),
                _ => (3, 60, 120, 4, 2, 60, 0.6, 10), // 默认值
            };

        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES (?1, ?2, ?3, ?4, 600, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                app_type,
                retries,
                fb_timeout,
                idle_timeout,
                cb_fail,
                cb_succ,
                cb_timeout,
                cb_rate,
                cb_min
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 初始化 proxy_config 表的三行数据
    ///
    /// 使用与 schema.rs seed 相同的 per-app 默认值
    fn init_proxy_config_rows(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 使用与 schema.rs seed 相同的 per-app 默认值
        // claude: 更激进的重试和超时配置
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES ('claude', 6, 90, 180, 600, 8, 3, 90, 0.7, 15)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // codex: 默认配置
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES ('codex', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // gemini: 稍高的重试次数
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES ('gemini', 5, 60, 120, 600, 4, 2, 60, 0.6, 10)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // grokbuild: Responses protocol, same timeout defaults as Codex.
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (
                app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests
            ) VALUES ('grokbuild', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 代理服务器的监听配置（取自全局配置）
    pub async fn get_listen_config(&self) -> Result<ProxyConfig, AppError> {
        let global = self.get_global_proxy_config().await?;
        Ok(ProxyConfig {
            listen_address: global.listen_address,
            listen_port: global.listen_port,
            enable_logging: global.enable_logging,
        })
    }

    // ==================== Provider Health ====================

    /// 获取Provider健康状态
    pub async fn get_provider_health(
        &self,
        provider_id: &str,
        app_type: &AppType,
    ) -> Result<ProviderHealth, AppError> {
        let provider_id = provider_id.to_owned();
        let app_type = app_type.clone();
        self.run_blocking(move |db| db.get_provider_health_sync(&provider_id, &app_type))
            .await
    }

    fn get_provider_health_sync(
        &self,
        provider_id: &str,
        app_type: &AppType,
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

    /// 更新Provider健康状态（带阈值参数）
    ///
    /// # Arguments
    /// * `failure_threshold` - 连续失败多少次后标记为不健康
    pub async fn update_provider_health_with_threshold(
        &self,
        provider_id: &str,
        app_type: &AppType,
        success: bool,
        error_msg: Option<String>,
        failure_threshold: u32,
    ) -> Result<(), AppError> {
        let provider_id = provider_id.to_owned();
        let app_type = app_type.clone();
        self.run_blocking(move |db| {
            db.update_provider_health_with_threshold_sync(
                &provider_id,
                &app_type,
                success,
                error_msg,
                failure_threshold,
            )
        })
        .await
    }

    fn update_provider_health_with_threshold_sync(
        &self,
        provider_id: &str,
        app_type: &AppType,
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

    /// 清空指定应用的健康状态（关闭单个代理时使用）
    pub async fn clear_provider_health_for_app(&self, app_type: &AppType) -> Result<(), AppError> {
        let app_type = app_type.clone();
        self.run_blocking(move |db| db.clear_provider_health_for_app_sync(&app_type))
            .await
    }

    fn clear_provider_health_for_app_sync(&self, app_type: &AppType) -> Result<(), AppError> {
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
        self.run_blocking(move |db| db.clear_all_provider_health_sync())
            .await
    }

    fn clear_all_provider_health_sync(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute("DELETE FROM provider_health", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        log::debug!("Cleared all provider health records");
        Ok(())
    }

    // ==================== Live Backup ====================

    /// 保存 Live 配置备份
    pub async fn save_live_backup(
        &self,
        app_type: &AppType,
        config_json: &str,
    ) -> Result<(), AppError> {
        let app_type = app_type.clone();
        let config_json = config_json.to_owned();
        self.run_blocking(move |db| db.save_live_backup_sync(&app_type, &config_json))
            .await
    }

    fn save_live_backup_sync(&self, app_type: &AppType, config_json: &str) -> Result<(), AppError> {
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
    pub async fn get_live_backup(
        &self,
        app_type: &AppType,
    ) -> Result<Option<LiveBackup>, AppError> {
        let app_type = app_type.clone();
        self.run_blocking(move |db| db.get_live_backup_sync(&app_type))
            .await
    }

    fn get_live_backup_sync(&self, app_type: &AppType) -> Result<Option<LiveBackup>, AppError> {
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
    pub async fn delete_live_backup(&self, app_type: &AppType) -> Result<(), AppError> {
        let app_type = app_type.clone();
        self.run_blocking(move |db| db.delete_live_backup_sync(&app_type))
            .await
    }

    fn delete_live_backup_sync(&self, app_type: &AppType) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "DELETE FROM proxy_live_backup WHERE app_type = ?1",
            rusqlite::params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已删除 {app_type} Live 配置备份");
        Ok(())
    }

    // ==================== Sync Methods for Tray Menu ====================

    /// 同步获取应用的 proxy 启用状态和自动故障转移状态
    ///
    /// 用于托盘菜单构建等同步场景
    /// 返回 (enabled, auto_failover_enabled)
    /// 同步读客户端连代理用的主机和端口（读不到时用默认值）：本机设了客户端地址
    /// （settings.json）就用它，否则是监听地址（交给 `proxy_origin` 处理 `0.0.0.0`）。
    /// 写进客户端文件的代理地址都按这个生成，不需要代理在运行。
    pub fn get_proxy_client_endpoint_sync(&self) -> (String, u16) {
        let fallback = || {
            let defaults = crate::proxy::types::ProxyConfig::default();
            (defaults.listen_address, defaults.listen_port)
        };
        let (listen_address, port) = match lock_recovering(&self.conn) {
            Ok(conn) => conn
                .query_row(
                    "SELECT listen_address, listen_port FROM proxy_config WHERE app_type = 'claude'",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? as u16)),
                )
                .unwrap_or_else(|_| fallback()),
            Err(_) => fallback(),
        };
        let host = crate::settings::get_proxy_client_host().unwrap_or(listen_address);
        (host, port)
    }

    pub fn get_proxy_flags_sync(&self, app_type: &AppType) -> (bool, bool) {
        let conn = match lock_recovering(&self.conn) {
            Ok(c) => c,
            Err(_) => return (false, false),
        };

        conn.query_row(
            "SELECT enabled, auto_failover_enabled FROM proxy_config WHERE app_type = ?1",
            [app_type],
            |row| Ok((row.get::<_, i32>(0)? != 0, row.get::<_, i32>(1)? != 0)),
        )
        .unwrap_or((false, false))
    }

    /// 同步设置应用的 proxy 启用状态和自动故障转移状态
    ///
    /// 用于托盘菜单点击等同步场景
    pub fn set_proxy_flags_sync(
        &self,
        app_type: &AppType,
        enabled: bool,
        auto_failover_enabled: bool,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE proxy_config SET enabled = ?2, auto_failover_enabled = ?3, updated_at = datetime('now') WHERE app_type = ?1",
            rusqlite::params![
                app_type,
                if enabled { 1 } else { 0 },
                if auto_failover_enabled { 1 } else { 0 },
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::error::AppError;

    #[tokio::test]
    async fn test_pricing_model_source_round_trip_and_validation() -> Result<(), AppError> {
        let db = Database::memory()?;

        let default = db.get_pricing_model_source(&AppType::Claude).await?;
        assert_eq!(default, "response");

        db.set_pricing_model_source(&AppType::Claude, "request")
            .await?;
        let updated = db.get_pricing_model_source(&AppType::Claude).await?;
        assert_eq!(updated, "request");

        let err = db
            .set_pricing_model_source(&AppType::Claude, "invalid")
            .await
            .unwrap_err();
        // AppError::localized returns AppError::Localized variant
        assert!(matches!(
            err,
            AppError::Localized {
                key: "error.invalidPricingMode",
                ..
            }
        ));

        Ok(())
    }
}
