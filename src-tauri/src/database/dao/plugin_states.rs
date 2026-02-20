//! Claude 插件状态数据访问对象

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    pub plugin_id: String,
    pub enabled: bool,
    pub install_path: String,
    pub scope: String,
    pub version: Option<String>,
}

impl Database {
    /// 获取所有插件状态
    pub fn get_all_plugin_states(&self) -> Result<Vec<PluginState>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT plugin_id, enabled, install_path, scope, version
                 FROM plugin_states
                 ORDER BY plugin_id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let iter = stmt
            .query_map([], |row| {
                Ok(PluginState {
                    plugin_id: row.get(0)?,
                    enabled: row.get(1)?,
                    install_path: row.get(2)?,
                    scope: row.get(3)?,
                    version: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// 插入或更新插件记录（不覆盖 enabled 状态）
    pub fn upsert_plugin_state(
        &self,
        plugin_id: &str,
        install_path: &str,
        version: Option<&str>,
        scope: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO plugin_states (plugin_id, install_path, version, scope, enabled)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(plugin_id) DO UPDATE SET
                install_path = excluded.install_path,
                version      = excluded.version,
                scope        = excluded.scope,
                updated_at   = datetime('now')",
            params![plugin_id, install_path, version, scope],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 设置插件启用/禁用状态
    pub fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let rows = conn
            .execute(
                "UPDATE plugin_states SET enabled = ?1, updated_at = datetime('now')
                 WHERE plugin_id = ?2",
                params![enabled, plugin_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(rows > 0)
    }

    /// 删除插件记录
    pub fn remove_plugin_state(&self, plugin_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM plugin_states WHERE plugin_id = ?1",
            params![plugin_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 获取 enabledPlugins map（用于写入 config.json）
    pub fn get_enabled_plugins_map(
        &self,
    ) -> Result<IndexMap<String, bool>, AppError> {
        let states = self.get_all_plugin_states()?;
        let map = states
            .into_iter()
            .map(|s| (s.plugin_id, s.enabled))
            .collect();
        Ok(map)
    }
}
