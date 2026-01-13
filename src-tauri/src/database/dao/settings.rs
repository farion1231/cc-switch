//! 通用设置数据访问对象
//!
//! 提供键值对形式的通用设置存储。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

impl Database {
    /// 获取设置值
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![key])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    /// 设置值
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // --- Config Snippets 辅助方法 ---

    /// 获取通用配置片段
    pub fn get_config_snippet(&self, app_type: &str) -> Result<Option<String>, AppError> {
        self.get_setting(&format!("common_config_{app_type}"))
    }

    /// 设置通用配置片段
    pub fn set_config_snippet(
        &self,
        app_type: &str,
        snippet: Option<String>,
    ) -> Result<(), AppError> {
        let key = format!("common_config_{app_type}");
        if let Some(value) = snippet {
            self.set_setting(&key, &value)
        } else {
            // 如果为 None 则删除
            let conn = lock_conn!(self.conn);
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
                .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        }
    }

    // --- 全局出站代理 ---

    /// 全局代理 URL 的存储键名
    const GLOBAL_PROXY_URL_KEY: &'static str = "global_proxy_url";

    /// 获取全局出站代理 URL
    ///
    /// 返回 None 表示未配置或已清除代理（直连）
    /// 返回 Some(url) 表示已配置代理
    pub fn get_global_proxy_url(&self) -> Result<Option<String>, AppError> {
        self.get_setting(Self::GLOBAL_PROXY_URL_KEY)
    }

    /// 设置全局出站代理 URL
    ///
    /// - 传入非空字符串：启用代理
    /// - 传入空字符串或 None：清除代理设置（直连）
    pub fn set_global_proxy_url(&self, url: Option<&str>) -> Result<(), AppError> {
        match url {
            Some(u) if !u.trim().is_empty() => {
                self.set_setting(Self::GLOBAL_PROXY_URL_KEY, u.trim())
            }
            _ => {
                // 清除代理设置
                let conn = lock_conn!(self.conn);
                conn.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    params![Self::GLOBAL_PROXY_URL_KEY],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            }
        }
    }

    // --- 代理接管状态管理（已废弃，使用 proxy_config.enabled 替代）---

    /// 获取指定应用的代理接管状态
    ///
    /// **已废弃**: 请使用 `proxy_config.enabled` 字段替代
    /// 此方法仅用于数据库迁移时读取旧数据
    #[deprecated(since = "3.9.0", note = "使用 get_proxy_config_for_app().enabled 替代")]
    pub fn get_proxy_takeover_enabled(&self, app_type: &str) -> Result<bool, AppError> {
        let key = format!("proxy_takeover_{app_type}");
        match self.get_setting(&key)? {
            Some(value) => Ok(value == "true"),
            None => Ok(false),
        }
    }

    /// 设置指定应用的代理接管状态
    ///
    /// **已废弃**: 请使用 `proxy_config.enabled` 字段替代
    #[deprecated(
        since = "3.9.0",
        note = "使用 update_proxy_config_for_app() 修改 enabled 字段"
    )]
    pub fn set_proxy_takeover_enabled(
        &self,
        app_type: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        let key = format!("proxy_takeover_{app_type}");
        let value = if enabled { "true" } else { "false" };
        self.set_setting(&key, value)
    }

    /// 检查是否有任一应用开启了代理接管
    ///
    /// **已废弃**: 请使用 `is_live_takeover_active()` 替代
    #[deprecated(since = "3.9.0", note = "使用 is_live_takeover_active() 替代")]
    pub fn has_any_proxy_takeover(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key LIKE 'proxy_takeover_%' AND value = 'true'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    /// 清除所有代理接管状态（将所有 proxy_takeover_* 设置为 false）
    ///
    /// **已废弃**: settings 表不再用于存储代理状态
    #[deprecated(
        since = "3.9.0",
        note = "使用 update_proxy_config_for_app() 清除各应用的 enabled 字段"
    )]
    pub fn clear_all_proxy_takeover(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE settings SET value = 'false' WHERE key LIKE 'proxy_takeover_%'",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        log::info!("已清除所有代理接管状态");
        Ok(())
    }

    // --- 整流器配置 ---

    /// 获取整流器配置
    ///
    /// 返回整流器配置，如果不存在则返回默认值（全部启用）
    pub fn get_rectifier_config(&self) -> Result<crate::proxy::types::RectifierConfig, AppError> {
        match self.get_setting("rectifier_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Database(format!("解析整流器配置失败: {e}"))),
            None => Ok(crate::proxy::types::RectifierConfig::default()),
        }
    }

    /// 更新整流器配置
    pub fn set_rectifier_config(
        &self,
        config: &crate::proxy::types::RectifierConfig,
    ) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Database(format!("序列化整流器配置失败: {e}")))?;
        self.set_setting("rectifier_config", &json)
    }
}
