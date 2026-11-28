//! 代理功能数据访问层
//!
//! 处理代理配置、Provider健康状态和使用统计的数据库操作

use crate::app_config::AppType;
use crate::error::AppError;
use crate::proxy::types::*;
use rusqlite::OptionalExtension;

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

/// Claude Desktop 的应用级配置。`proxy_config` 表的 CHECK 不收 `claude-desktop`，加一行就要
/// 重建表、升数据库版本，所以这一份整体存成 `settings` 表里的一条 JSON；读写都走下面几个
/// 函数，调用方按应用名取，不用管存在哪。
const DESKTOP_APP_TYPE: &str = "claude-desktop";
pub(crate) const DESKTOP_PROXY_CONFIG_KEY: &str = "claude_desktop_proxy_config";
pub(crate) const DESKTOP_PRICING_SOURCE_KEY: &str = "claude_desktop_pricing_model_source";

/// 还没存过时的默认值：重试、超时、熔断参数和 Claude Code 那一行的 seed 一致。
fn desktop_default_proxy_config() -> AppProxyConfig {
    AppProxyConfig {
        app_type: DESKTOP_APP_TYPE.to_string(),
        enabled: false,
        auto_failover_enabled: false,
        max_retries: 6,
        streaming_first_byte_timeout: 90,
        streaming_idle_timeout: 180,
        non_streaming_timeout: 600,
        circuit_failure_threshold: 8,
        circuit_success_threshold: 3,
        circuit_timeout_seconds: 90,
        circuit_error_rate_threshold: 0.7,
        circuit_min_requests: 15,
    }
}

fn decode_desktop_proxy_config(raw: &str) -> serde_json::Result<AppProxyConfig> {
    use serde_json::{Map, Value};

    match serde_json::from_str(raw) {
        Err(error) if error.is_data() => {}
        result => return result,
    }

    let mut saved: Map<String, Value> = serde_json::from_str(raw)?;
    // 只补缺失字段，保留旧版已经保存的开关、超时和熔断设置。
    if let Value::Object(defaults) = serde_json::to_value(desktop_default_proxy_config())? {
        for (key, value) in defaults {
            saved.entry(key).or_insert(value);
        }
    }
    serde_json::from_value(Value::Object(saved))
}

/// settings 表里存的那条 JSON 换成配置；没存过、解析不了都按默认值。
fn desktop_proxy_config_from(raw: Option<String>) -> AppProxyConfig {
    let Some(raw) = raw else {
        return desktop_default_proxy_config();
    };
    match decode_desktop_proxy_config(&raw) {
        Ok(mut config) => {
            config.app_type = DESKTOP_APP_TYPE.to_string();
            config
        }
        Err(error) => {
            log::warn!("Claude Desktop 的路由配置解析不了，按默认值处理: {error}");
            desktop_default_proxy_config()
        }
    }
}

impl Database {
    // ==================== Claude Desktop（存在 settings 表） ====================

    fn desktop_proxy_config(&self) -> Result<AppProxyConfig, AppError> {
        Ok(desktop_proxy_config_from(
            self.get_setting(DESKTOP_PROXY_CONFIG_KEY)?,
        ))
    }

    fn save_desktop_proxy_config(&self, config: &AppProxyConfig) -> Result<(), AppError> {
        let raw = serde_json::to_string(config)
            .map_err(|e| AppError::Database(format!("序列化 Claude Desktop 路由配置失败: {e}")))?;
        self.set_setting(DESKTOP_PROXY_CONFIG_KEY, &raw)
    }

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
        if *app_type == AppType::ClaudeDesktop {
            // Desktop 的设置存在 settings 表，不进 proxy_config 的缓存（那份缓存只在
            // proxy_config 表写入时失效）。键不存在时回退到 Claude 的设置，避免升级后用户之前
            // 的「按请求模型」配置对 Desktop 悄悄失效。
            if let Some(value) = self.get_setting(DESKTOP_PRICING_SOURCE_KEY)? {
                if let Ok(trimmed) = validate_pricing_source(&value) {
                    return Ok(trimmed.to_string());
                }
            }
            return self.get_pricing_model_source_sync(&AppType::Claude);
        }
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
        if *app_type == AppType::ClaudeDesktop {
            return self.set_setting(DESKTOP_PRICING_SOURCE_KEY, trimmed);
        }

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
        if *app_type == AppType::ClaudeDesktop {
            // 走 settings 表的缓存（settings 写入时失效），热路径上不等数据库锁。
            if let Some(raw) = self.read_cache.settings.get(DESKTOP_PROXY_CONFIG_KEY) {
                return Ok(desktop_proxy_config_from(raw));
            }
            return self.run_blocking(|db| db.desktop_proxy_config()).await;
        }
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
        if *app_type == AppType::ClaudeDesktop {
            return self.desktop_proxy_config();
        }
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
        if config.app_type == DESKTOP_APP_TYPE {
            return self.save_desktop_proxy_config(&config);
        }
        let conn = lock_conn!(self.conn);
        let previous_failover = conn
            .query_row(
                "SELECT auto_failover_enabled != 0 FROM proxy_config WHERE app_type = ?1",
                [&config.app_type],
                |row| row.get::<_, bool>(0),
            )
            .optional()?;

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
        if let (true, Ok(app)) = (
            previous_failover.is_some_and(|previous| previous != config.auto_failover_enabled),
            config.app_type.parse(),
        ) {
            self.advance_route_revision(&app);
        }

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
        if *app_type == AppType::ClaudeDesktop {
            return self
                .desktop_proxy_config()
                .map(|config| (config.enabled, config.auto_failover_enabled))
                .unwrap_or((false, false));
        }
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
        if *app_type == AppType::ClaudeDesktop {
            let mut config = self.desktop_proxy_config()?;
            config.enabled = enabled;
            config.auto_failover_enabled = auto_failover_enabled;
            return self.save_desktop_proxy_config(&config);
        }
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
    use super::{
        desktop_default_proxy_config, DESKTOP_PRICING_SOURCE_KEY, DESKTOP_PROXY_CONFIG_KEY,
    };
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

    /// Claude Desktop 没有 `proxy_config` 行（表的 CHECK 不收），整份配置存在 settings 表里，
    /// 但读写接口和其它应用一样。
    #[tokio::test]
    async fn claude_desktop_config_round_trips_without_a_proxy_config_row() -> Result<(), AppError>
    {
        let db = Database::memory()?;

        let mut config = db.get_proxy_config_for_app(&AppType::ClaudeDesktop).await?;
        assert_eq!(config.app_type, "claude-desktop");
        assert!(!config.enabled && !config.auto_failover_enabled);
        assert_eq!(
            db.get_proxy_flags_sync(&AppType::ClaudeDesktop),
            (false, false)
        );

        config.auto_failover_enabled = true;
        config.max_retries = 2;
        db.update_proxy_config_for_app(config).await?;
        let saved = db.get_proxy_config_for_app(&AppType::ClaudeDesktop).await?;
        assert!(saved.auto_failover_enabled);
        assert_eq!(saved.max_retries, 2);

        // 只改两个开关，其余参数留着
        db.set_proxy_flags_sync(&AppType::ClaudeDesktop, true, false)?;
        assert_eq!(
            db.get_proxy_flags_sync(&AppType::ClaudeDesktop),
            (true, false)
        );
        assert_eq!(
            db.get_proxy_config_for_app(&AppType::ClaudeDesktop)
                .await?
                .max_retries,
            2
        );

        db.set_pricing_model_source(&AppType::Claude, "request")
            .await?;
        assert_eq!(
            db.get_pricing_model_source(&AppType::ClaudeDesktop).await?,
            "request"
        );

        db.set_pricing_model_source(&AppType::ClaudeDesktop, "response")
            .await?;
        assert_eq!(
            db.get_pricing_model_source(&AppType::ClaudeDesktop).await?,
            "response"
        );
        assert_eq!(
            db.get_pricing_model_source(&AppType::Claude).await?,
            "request"
        );

        // 其它应用的行不受影响，表里也没有多出 Desktop 的行
        assert!(
            !db.get_proxy_config_for_app(&AppType::Claude)
                .await?
                .auto_failover_enabled
        );
        let conn = db.conn.lock().unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'claude-desktop'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
        Ok(())
    }

    #[tokio::test]
    async fn claude_desktop_config_defaults_only_missing_fields() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.set_setting(
            DESKTOP_PROXY_CONFIG_KEY,
            r#"{"enabled":true,"autoFailoverEnabled":true,"maxRetries":0,"streamingIdleTimeout":37,"circuitFailureThreshold":2}"#,
        )?;

        let config = db.get_proxy_config_for_app(&AppType::ClaudeDesktop).await?;
        let defaults = desktop_default_proxy_config();
        assert!(config.enabled && config.auto_failover_enabled);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.streaming_idle_timeout, 37);
        assert_eq!(config.circuit_failure_threshold, 2);
        assert_eq!(config.app_type, defaults.app_type);
        assert_eq!(
            config.circuit_timeout_seconds,
            defaults.circuit_timeout_seconds
        );
        Ok(())
    }

    #[tokio::test]
    async fn claude_desktop_pricing_source_trims_and_preserves_database_errors(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        db.set_setting(DESKTOP_PRICING_SOURCE_KEY, "\trequest \n")?;
        assert_eq!(
            db.get_pricing_model_source(&AppType::ClaudeDesktop).await?,
            "request"
        );

        db.set_setting(DESKTOP_PRICING_SOURCE_KEY, "unknown")?;
        db.conn.lock().unwrap().execute(
            "ALTER TABLE proxy_config RENAME COLUMN pricing_model_source TO old_pricing_model_source",
            [],
        )?;
        assert!(matches!(
            db.get_pricing_model_source(&AppType::ClaudeDesktop).await,
            Err(AppError::Database(_))
        ));
        Ok(())
    }

    /// Desktop 的配置存在 settings 表，不走 proxy_config 的缓存：读过一次之后，热路径上的
    /// 读取不等数据库锁；改了之后马上读到新值（settings 表写入让缓存失效）。
    #[tokio::test]
    async fn claude_desktop_reads_are_cached_and_see_their_own_writes() -> Result<(), AppError> {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let db = Arc::new(Database::memory()?);
        let desktop = AppType::ClaudeDesktop;
        let mut config = db.get_proxy_config_for_app(&desktop).await?;
        config.max_retries = 1;
        db.update_proxy_config_for_app(config).await?;
        db.set_pricing_model_source(&desktop, "request").await?;
        // 写入后立刻读到新值，同时把缓存热起来
        assert_eq!(db.get_proxy_config_for_app(&desktop).await?.max_retries, 1);
        assert_eq!(db.get_pricing_model_source_sync(&desktop)?, "request");

        let guard = db.conn.lock().unwrap();
        let worker_db = db.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let config = worker_db
                        .get_proxy_config_for_app(&AppType::ClaudeDesktop)
                        .await
                        .unwrap();
                    assert_eq!(config.max_retries, 1);
                });
            assert_eq!(
                worker_db
                    .get_pricing_model_source_sync(&AppType::ClaudeDesktop)
                    .unwrap(),
                "request"
            );
            let _ = tx.send(());
        });
        let completed = rx.recv_timeout(Duration::from_secs(2));
        drop(guard);
        worker.join().unwrap();
        completed.expect("warm Claude Desktop reads acquired the SQLite mutex");

        // 再改一次，缓存不会留着旧值
        let mut config = db.get_proxy_config_for_app(&desktop).await?;
        config.max_retries = 4;
        db.update_proxy_config_for_app(config).await?;
        db.set_pricing_model_source(&desktop, "response").await?;
        assert_eq!(db.get_proxy_config_for_app(&desktop).await?.max_retries, 4);
        assert_eq!(db.get_pricing_model_source_sync(&desktop)?, "response");
        Ok(())
    }
}
